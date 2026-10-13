use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::f32::consts::FRAC_PI_2;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use reqwest::Url;
use reqwest::header::RANGE;
use rtrb::{Producer, RingBuffer};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Adjustable, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use rusty_opus::OpusDecoder;
use symphonia::core::codecs::audio::AudioCodecParameters;
use symphonia::core::codecs::audio::well_known::{CODEC_ID_FLAC, CODEC_ID_OPUS};
use symphonia::core::errors::{Error as SymError, SeekErrorKind};
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint};
use symphonia::core::io::{MediaSource, MediaSourceStream, ReadBytes};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Time, TimeBase, Timestamp};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

const CHUNK: usize = 512;
const CLOSE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: String,
    pub url: String,
    pub gain: f32,
    pub transcoded: bool,
    pub duration: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Started(String),
    Ended,
    Error(String),
}

enum Cmd {
    Play(Track, f32),
    Next(Option<Track>),
    Seek(f32),
    Gain(f32),
    Device(String),
    Stop,
}

pub fn devices() -> Result<Vec<(String, String)>, String> {
    cpal::default_host()
        .output_devices()
        .map_err(|e| e.to_string())?
        .map(|d| Ok((d.id().map_err(|e| e.to_string())?.to_string(), d.to_string())))
        .collect()
}

#[derive(Default)]
struct Shared {
    volume: AtomicU32,
    speed: AtomicU32,
    crossfade: AtomicU32,
    pos: AtomicU32,
    paused: AtomicBool,
    read: AtomicU64,
    flush: AtomicU64,
}

pub struct Player {
    tx: Sender<Cmd>,
    sh: Arc<Shared>,
    th: Thread,
}

impl Player {
    pub fn new(rt: Handle, http: reqwest::Client) -> (Player, UnboundedReceiver<Event>) {
        let (tx, rx) = channel();
        let (etx, erx) = unbounded_channel();
        let sh = Arc::new(Shared::default());
        sh.volume.store(1f32.to_bits(), Relaxed);
        sh.speed.store(1f32.to_bits(), Relaxed);
        let esh = sh.clone();
        let th = thread::spawn(move || engine(rx, etx, esh, (rt, http))).thread().clone();
        (Player { tx, sh, th }, erx)
    }

    fn send(&self, cmd: Cmd) {
        self.tx.send(cmd).expect("audio engine is running");
        self.th.unpark();
    }

    pub fn play(&self, track: Track, at: f32) {
        self.sh.paused.store(false, Relaxed);
        self.sh.pos.store(at.max(0.0).to_bits(), Relaxed);
        self.send(Cmd::Play(track, at.max(0.0)));
    }

    pub fn queue_next(&self, track: Option<Track>) {
        self.send(Cmd::Next(track));
    }

    pub fn set_paused(&self, paused: bool) {
        self.sh.paused.store(paused, Relaxed);
        self.th.unpark();
    }

    pub fn seek(&self, secs: f32) {
        self.sh.pos.store(secs.max(0.0).to_bits(), Relaxed);
        self.send(Cmd::Seek(secs.max(0.0)));
    }

    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }

    pub fn set_gain(&self, gain: f32) {
        self.send(Cmd::Gain(gain));
    }

    pub fn set_device(&self, id: &str) {
        self.send(Cmd::Device(id.to_string()));
    }

    pub fn set_volume(&self, v: f32) {
        self.sh.volume.store(v.clamp(0.0, 1.0).to_bits(), Relaxed);
    }

    pub fn set_speed(&self, s: f32) {
        assert!((0.5..=3.5).contains(&s), "speed {s} is out of range");
        self.sh.speed.store(s.to_bits(), Relaxed);
    }

    pub fn set_crossfade(&self, secs: f32) {
        self.sh.crossfade.store(secs.max(0.0).to_bits(), Relaxed);
    }

    pub fn position(&self) -> f32 {
        f32::from_bits(self.sh.pos.load(Relaxed))
    }

    pub fn is_paused(&self) -> bool {
        self.sh.paused.load(Relaxed)
    }
}

type Io = (Handle, reqwest::Client);

const TAIL: u64 = 64 * 1024;
const AHEAD: u64 = 4 << 20;
const BACK: u64 = 1 << 20;

#[derive(Default)]
struct Net0 {
    buf: BTreeMap<u64, Vec<u8>>,
    size: u64,
    have: Vec<(u64, u64)>,
    len: Option<u64>,
    want: u64,
    jump: bool,
    cur: (u64, u64),
    rate: f64,
    ttfb: f64,
    err: Option<String>,
    eof: bool,
}

#[derive(Default)]
struct Net(Mutex<Net0>, Condvar, tokio::sync::Notify);

impl Net0 {
    fn add(&mut self, at: u64, data: &[u8]) {
        let (s, e) = self.free(at, at + data.len() as u64);
        if s >= e {
            return;
        }
        self.buf.insert(s, data[(s - at) as usize..(e - at) as usize].to_vec());
        self.size += e - s;
        let i = self.have.partition_point(|r| r.1 < s);
        let j = self.have.partition_point(|r| r.0 <= e);
        let r = self.have[i..j].iter().fold((s, e), |a, r| (a.0.min(r.0), a.1.max(r.1)));
        self.have.splice(i..j, [r]);
        if self.size > 2 * AHEAD {
            let (lo, hi) = (self.want.saturating_sub(BACK), self.want + AHEAD + BACK);
            let tail = self.len.map_or(u64::MAX, |l| l.saturating_sub(TAIL));
            self.buf.retain(|&k, v| k >= tail || (k + v.len() as u64 > lo && k < hi));
            self.size = self.buf.values().map(|v| v.len() as u64).sum();
            self.have = self.buf.iter().fold(Vec::new(), |mut h, (&k, v)| {
                match h.last_mut() {
                    Some((_, e)) if *e == k => *e += v.len() as u64,
                    _ => h.push((k, k + v.len() as u64)),
                }
                h
            });
        }
    }

    fn free(&self, from: u64, to: u64) -> (u64, u64) {
        let s = self.have.iter().find(|r| r.0 <= from && from < r.1).map_or(from, |r| r.1);
        (s, self.have.iter().find(|r| r.0 > s).map_or(to, |r| r.0.min(to)))
    }

    fn gap(&self) -> Option<(u64, u64)> {
        let len = self.len?;
        Some(self.free(self.want, len)).filter(|g| g.0 < len && g.0 < self.want + AHEAD)
    }
}

fn range(r: &reqwest::Response) -> Option<(u64, u64)> {
    let v = r.headers().get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    let (span, len) = v.strip_prefix("bytes ")?.split_once('/')?;
    Some((span.split_once('-')?.0.parse().ok()?, len.parse().ok()?))
}

async fn fetch(
    net: Arc<Net>,
    http: reqwest::Client,
    url: Url,
    mut first: Option<reqwest::Response>,
) {
    let res = async {
        loop {
            let mut resp = match first.take() {
                Some(r) => r,
                None => {
                    let (s, e) = loop {
                        {
                            let mut st = net.0.lock().expect("net lock");
                            st.jump = false;
                            let g = st.gap();
                            st.cur = g.unwrap_or_default();
                            match g {
                                Some(g) => break g,
                                None if st.len.is_none() => return Ok(()),
                                None => {}
                            }
                        }
                        net.2.notified().await;
                    };
                    let t = Instant::now();
                    let r = http.get(url.clone()).header(RANGE, format!("bytes={s}-{}", e - 1));
                    let r = r.send().await?.error_for_status()?;
                    net.0.lock().expect("net lock").ttfb = t.elapsed().as_secs_f64();
                    r
                }
            };
            let (mut at, t, mut n) = (range(&resp).map_or(0, |r| r.0), Instant::now(), 0);
            loop {
                let Some(c) = resp.chunk().await? else {
                    let mut st = net.0.lock().expect("net lock");
                    st.eof |= st.len.is_none();
                    break;
                };
                let mut st = net.0.lock().expect("net lock");
                st.add(at, &c);
                (at, n) = (at + c.len() as u64, n + c.len());
                (st.cur.0, st.rate) = (at, n as f64 / (t.elapsed().as_secs_f64() + st.ttfb));
                net.1.notify_all();
                let far = st.len.is_some() && at > st.want + AHEAD;
                if std::mem::take(&mut st.jump) || at >= st.cur.1 || far {
                    break;
                }
            }
            if net.0.lock().expect("net lock").eof {
                break;
            }
        }
        Ok::<_, reqwest::Error>(())
    };
    let res = res.await;
    let mut st = net.0.lock().expect("net lock");
    if let Err(e) = res {
        st.err = Some(e.without_url().to_string());
    }
    net.1.notify_all();
}

struct Http {
    net: Arc<Net>,
    pos: u64,
    task: tokio::task::AbortHandle,
}

impl Read for Http {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut st = self.net.0.lock().expect("net lock");
        st.want = self.pos;
        loop {
            let idle = st.cur == (0, 0);
            if let Some(&(_, end)) = st.have.iter().find(|r| r.0 <= self.pos && self.pos < r.1) {
                let (k, v) =
                    st.buf.range(..=self.pos).next_back().expect("a chunk covers the range");
                let d = &v[(self.pos - k) as usize..];
                let n = buf.len().min(d.len());
                buf[..n].copy_from_slice(&d[..n]);
                self.pos += n as u64;
                if idle && end - self.pos < AHEAD / 2 && st.len.is_some_and(|l| end < l) {
                    self.net.2.notify_one();
                }
                return Ok(n);
            }
            if let Some(e) = &st.err {
                return Err(std::io::Error::other(e.clone()));
            }
            if st.eof || st.len.is_some_and(|l| self.pos >= l) {
                return Ok(0);
            }
            let (c, e) = st.cur;
            let ahead = self.pos.wrapping_sub(c) as f64;
            st.jump |= self.pos >= e || ahead > st.rate * st.ttfb + 65536.0;
            if idle {
                self.net.2.notify_one();
            }
            st = self.net.1.wait(st).expect("net lock");
        }
    }
}

impl Seek for Http {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let len = || self.net.0.lock().expect("net lock").len.ok_or(ErrorKind::Unsupported);
        self.pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(d) => self.pos.saturating_add_signed(d),
            SeekFrom::End(d) => len()?.saturating_add_signed(d),
        };
        Ok(self.pos)
    }
}

impl MediaSource for Http {
    fn is_seekable(&self) -> bool {
        self.byte_len().is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.net.0.lock().expect("net lock").len
    }
}

impl Drop for Http {
    fn drop(&mut self) {
        self.task.abort();
    }
}

trait Demux: Send + Sync {
    fn next(&mut self) -> Result<Option<Packet>, SymError>;
    fn seek(&mut self, ts: Timestamp, mode: SeekMode) -> Result<Timestamp, SymError>;
}

impl Demux for (Box<dyn FormatReader>, u32) {
    fn next(&mut self) -> Result<Option<Packet>, SymError> {
        loop {
            match self.0.next_packet()? {
                Some(p) if p.track_id != self.1 => {}
                p => return Ok(p),
            }
        }
    }

    fn seek(&mut self, ts: Timestamp, mode: SeekMode) -> Result<Timestamp, SymError> {
        Ok(self.0.seek(mode, SeekTo::Timestamp { ts, track_id: self.1 })?.actual_ts)
    }
}

struct Page {
    pos: u64,
    flags: u8,
    gran: u64,
    serial: u32,
    lace: Vec<u8>,
    data: Vec<u8>,
}

struct Raw {
    r: MediaSourceStream<'static>,
    block: Option<u64>,
    serial: u32,
    data: u64,
    len: u64,
    end: u64,
    pk: VecDeque<Packet>,
    part: Option<Vec<u8>>,
    prev: Option<u64>,
    big: u64,
}

impl Raw {
    fn open(
        mut r: MediaSourceStream<'static>,
        flac: bool,
    ) -> Result<(Raw, Vec<u8>, u32), Box<dyn Error>> {
        let len = r.byte_len().unwrap_or(0);
        let raw = |r, block, serial, end| Raw {
            data: 0,
            r,
            block,
            serial,
            len,
            end,
            pk: VecDeque::new(),
            part: None,
            prev: Some(0),
            big: 8192,
        };
        if flac {
            let mut h = [0u8; 4];
            r.read_exact(&mut h)?;
            let mut si = vec![0u8; 34];
            loop {
                r.read_exact(&mut h)?;
                match h[0] & 127 {
                    0 => r.read_exact(&mut si)?,
                    _ => {
                        _ = r.seek(SeekFrom::Current(
                            u32::from_be_bytes([0, h[1], h[2], h[3]]) as i64
                        ))?
                    }
                }
                if h[0] & 128 != 0 {
                    break;
                }
            }
            let sr = u32::from_be_bytes([0, si[10], si[11], si[12]]) >> 4;
            let end = u64::from_be_bytes([0, 0, 0, si[13] & 15, si[14], si[15], si[16], si[17]]);
            let block = u16::from_be_bytes([si[2], si[3]]) as u64;
            let data = r.pos();
            return Ok((Raw { data, ..raw(r, Some(block), 0, end) }, si, sr));
        }
        let head = Raw::page(&mut r)?.ok_or("The Ogg stream is empty.")?;
        loop {
            let p = Raw::page(&mut r)?.ok_or("The Ogg stream has no audio.")?;
            if p.serial == head.serial && p.lace.iter().any(|&l| l < 255) {
                break;
            }
        }
        let (data, mut end) = (r.pos(), u64::MAX);
        if len > 0 {
            let mut tail = Vec::new();
            r.seek(SeekFrom::Start(len.saturating_sub(TAIL)))?;
            r.read_to_end(&mut tail)?;
            let ok = |p: &[u8]| {
                p.len() >= 27 && p[..4] == *b"OggS" && p[14..18] == head.serial.to_le_bytes()
            };
            let i = (0..tail.len())
                .rev()
                .find(|&i| ok(&tail[i..]))
                .ok_or("The Ogg stream has no end.")?;
            end = u64::from_le_bytes(tail[i + 6..i + 14].try_into()?);
            r.seek(SeekFrom::Start(data))?;
        }
        Ok((Raw { data, ..raw(r, None, head.serial, end) }, head.data, 48_000))
    }

    fn page(r: &mut MediaSourceStream<'static>) -> std::io::Result<Option<Page>> {
        let mut w = 0u32;
        let res: std::io::Result<Page> = (|| loop {
            w = w << 8 | r.read_byte()? as u32;
            if w != u32::from_be_bytes(*b"OggS") {
                continue;
            }
            let pos = r.pos() - 4;
            let mut h = [0u8; 23];
            r.read_exact(&mut h)?;
            if h[0] != 0 || h[1] > 7 {
                r.seek(SeekFrom::Start(pos + 1))?;
                w = 0;
                continue;
            }
            let mut lace = vec![0; h[22] as usize];
            r.read_exact(&mut lace)?;
            let mut data = vec![0; lace.iter().map(|&l| l as usize).sum()];
            r.read_exact(&mut data)?;
            let gran = u64::from_le_bytes(h[2..10].try_into().expect("8 bytes"));
            let serial = u32::from_le_bytes(h[10..14].try_into().expect("4 bytes"));
            return Ok(Page { pos, flags: h[1], gran, serial, lace, data });
        })();
        match res {
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(None),
            r => r.map(Some),
        }
    }

    fn feed(&mut self, p: Page) {
        let mut part = if p.flags & 1 != 0 { self.part.take() } else { Some(Vec::new()) };
        let (mut off, mut got) = (0, Vec::new());
        for &l in &p.lace {
            let s = &p.data[off..off + l as usize];
            off += l as usize;
            part.iter_mut().for_each(|v| v.extend_from_slice(s));
            if l < 255 {
                got.extend(part.replace(Vec::new()));
            }
        }
        self.part = part;
        if p.gran == u64::MAX || got.is_empty() {
            return;
        }
        let dur = |d: &[u8]| {
            let Some(&toc) = d.first() else { return 0 };
            let c = toc as usize >> 3;
            let f = match c {
                0..=11 => [480, 960, 1920, 2880][c & 3],
                12..=15 => [480, 960][c & 1],
                _ => [120, 240, 480, 960][c & 3],
            };
            f * match toc & 3 {
                0 => 1,
                1 | 2 => 2,
                _ => d.get(1).map_or(0, |b| b & 63) as u64,
            }
        };
        let sum: u64 = got.iter().map(|d| dur(d)).sum();
        let eos = p.flags & 4 != 0;
        let mut t = self.prev.filter(|_| eos).unwrap_or(p.gran.saturating_sub(sum));
        for d in got {
            let n = dur(&d);
            let mut pk = Packet::new(0, Timestamp::new(t as i64), n.into(), d);
            pk.trim_end = if eos { (t + n).saturating_sub(p.gran).min(n) } else { 0 }.into();
            self.pk.push_back(pk);
            t += n;
        }
        self.prev = Some(p.gran);
    }

    fn frame(&mut self, block: u64) -> std::io::Result<Option<u64>> {
        let head = |b: &[u8]| -> Option<(u64, u64)> {
            let bad = b[1] & 0xFE != 0xF8 || b[2] >> 4 == 0 || b[2] & 15 == 15 || b[3] >> 4 > 10;
            let lead = b[4].leading_ones() as usize;
            if bad || b[3] & 1 != 0 || (b[3] >> 1) & 7 == 3 || lead == 1 || lead > 7 {
                return None;
            }
            let k = lead.max(1);
            let mut num = (b[4] & (0x7F >> lead)) as u64;
            for &c in &b[5..4 + k] {
                if c & 0xC0 != 0x80 {
                    return None;
                }
                num = num << 6 | (c & 0x3F) as u64;
            }
            let i = 4 + k;
            let n = match b[2] >> 4 {
                1 => 192,
                c @ 2..=5 => 576 << (c - 2),
                6 => b[i] as u64 + 1,
                7 => u16::from_be_bytes([b[i], b[i + 1]]) as u64 + 1,
                c => 256 << (c - 8),
            };
            let i =
                i + match b[2] >> 4 {
                    6 => 1,
                    7 => 2,
                    _ => 0,
                } + match b[2] & 15 {
                    12 => 1,
                    13 | 14 => 2,
                    _ => 0,
                };
            let crc = b[..i].iter().fold(0u8, |c, &x| {
                (0..8).fold(c ^ x, |c, _| if c & 0x80 != 0 { c << 1 ^ 7 } else { c << 1 })
            });
            (crc == b[i]).then_some((if b[1] & 1 == 0 { num * block } else { num }, n))
        };
        let (at, mut buf, mut i) = (self.r.pos(), Vec::new(), 0);
        let mut first: Option<(usize, u64, u64)> = None;
        loop {
            if buf.len() < i + 16 {
                if (&mut self.r).take(16384).read_to_end(&mut buf)? == 0 {
                    break;
                }
                continue;
            }
            let Some(j) = buf[i..buf.len() - 15].iter().position(|&b| b == 0xFF) else {
                i = buf.len() - 15;
                continue;
            };
            i += j;
            if let Some((s, n)) = head(&buf[i..]) {
                match first {
                    Some((j, s0, n0)) if s == s0 + n0 => {
                        self.r.seek(SeekFrom::Start(at + i as u64))?;
                        buf.truncate(i);
                        first = Some((j, s0, n0));
                        break;
                    }
                    Some(_) if self.prev.is_some() => {}
                    _ if self.prev.is_none_or(|e| e == s) => first = Some((i, s, n)),
                    _ => {}
                }
            }
            i += 1;
        }
        Ok(first.map(|(j, s, n)| {
            self.big = self.big.max((buf.len() - j) as u64);
            self.pk.push_back(Packet::new(0, Timestamp::new(s as i64), n.into(), buf.split_off(j)));
            self.prev = Some(s + n);
            s + n
        }))
    }

    fn unit(&mut self) -> Result<Option<u64>, SymError> {
        if let Some(b) = self.block {
            return Ok(self.frame(b)?);
        }
        while let Some(p) = Raw::page(&mut self.r)? {
            if p.serial == self.serial {
                let g = p.gran;
                self.big = self.big.max(self.r.pos() - p.pos);
                self.feed(p);
                return Ok(Some(g));
            }
        }
        Ok(None)
    }
}

impl Demux for Raw {
    fn next(&mut self) -> Result<Option<Packet>, SymError> {
        loop {
            if let Some(p) = self.pk.pop_front() {
                return Ok(Some(p));
            }
            if self.unit()?.is_none() {
                return Ok(None);
            }
        }
    }

    fn seek(&mut self, ts: Timestamp, _: SeekMode) -> Result<Timestamp, SymError> {
        let t = (ts.get().max(0) as u64).min(self.end.saturating_sub(1));
        let (mut lo, mut hi) = ((self.data, 0), (self.len, self.end.max(1)));
        'probe: for _ in 0..32 {
            let bps = (hi.0 - lo.0) as f64 / (hi.1 - lo.1).max(1) as f64;
            let est = lo.0 + ((t - lo.1) as f64 * bps) as u64;
            let at = est.saturating_sub((hi.0 - lo.0) / 64 + 2 * self.big).max(lo.0);
            self.r.seek(SeekFrom::Start(at))?;
            (self.pk, self.part, self.prev) = (VecDeque::new(), None, None);
            let mut g = loop {
                let Some(g) = self.unit()? else {
                    hi.0 = at;
                    continue 'probe;
                };
                let Some(start) = self.pk.front().map(|p| p.pts.get() as u64) else { continue };
                if start > t && at > lo.0 {
                    hi = (at, start);
                    continue 'probe;
                }
                break g;
            };
            while g <= t {
                if (t - g) as f64 * bps > 256.0 * 1024.0 {
                    lo = (self.r.pos(), g);
                    continue 'probe;
                }
                self.pk.clear();
                match self.unit()? {
                    Some(n) if n != u64::MAX => g = n,
                    Some(_) => {}
                    None => break,
                }
            }
            return Ok(self.pk.front().map_or(Timestamp::new(t as i64), |p| p.pts));
        }
        Err(SymError::SeekError(SeekErrorKind::OutOfRange))
    }
}

type Decode = Box<dyn FnMut(Option<&Packet>, &mut Vec<f32>) -> Result<(), SymError> + Send>;

struct Source {
    track: Track,
    demux: Box<dyn Demux>,
    net: Arc<Net>,
    dec: Decode,
    rs: Async<f32>,
    orate: u32,
    rel: f64,
    direct: bool,
    tb: TimeBase,
    rate: f64,
    delay: f64,
    dur: f64,
    base: f64,
    done: u64,
    skip: usize,
    lead: usize,
    owed: f64,
    eof: bool,
    pcm: Vec<f32>,
    out: Vec<f32>,
}

impl Source {
    fn open(track: Track, (rt, http): &Io, rate: u32, offset: f32) -> Result<Source, String> {
        let url = match track.transcoded && offset >= 1.0 {
            true => format!("{}&timeOffset={}", track.url, offset as u32),
            false => track.url.clone(),
        };
        let url: Url = url.parse().map_err(|_| "invalid stream url".to_string())?;
        let safe = url.as_str().to_string();
        (|| -> Result<Source, Box<dyn Error>> {
            let net = Arc::new(Net::default());
            let get = |r: Option<String>| match r {
                Some(r) => http.get(url.clone()).header(RANGE, r).send(),
                None => http.get(url.clone()).send(),
            };
            let task = rt.block_on(async {
                let t = Instant::now();
                let head = match track.transcoded {
                    true => get(None).await?.error_for_status()?,
                    false => {
                        let (head, tail) = tokio::try_join!(
                            get(Some("bytes=0-".into())),
                            get(Some(format!("bytes=-{TAIL}")))
                        )?;
                        let (head, tail) = (head.error_for_status()?, tail.error_for_status()?);
                        let ttfb = t.elapsed().as_secs_f64();
                        let len = range(&head).map(|r| r.1).or(head.content_length());
                        let len = len.ok_or("The server did not send the stream length.")?;
                        let at = range(&tail).map_or(0, |r| r.0);
                        let body = tail.bytes().await?;
                        let mut st = net.0.lock().expect("net lock");
                        (st.ttfb, st.len) = (ttfb, Some(len));
                        st.add(at, &body);
                        head
                    }
                };
                {
                    let mut st = net.0.lock().expect("net lock");
                    st.cur = (0, st.gap().map_or(u64::MAX, |g| g.1));
                }
                let (net, http, url) = (net.clone(), http.clone(), url.clone());
                Ok::<_, Box<dyn Error>>(rt.spawn(fetch(net, http, url, Some(head))).abort_handle())
            })?;
            let mut src: Box<dyn MediaSource> = Box::new(Http { net: net.clone(), pos: 0, task });
            let mut h = [0u8; 42];
            let ok = src.read_exact(&mut h).is_ok();
            let ogg = ok && h[..4] == *b"OggS" && h[28..36] == *b"OpusHead";
            let flac = ok
                && h[..4] == *b"fLaC"
                && h[4] & 127 == 0
                && (h[21] & 15 | h[22] | h[23] | h[24] | h[25]) != 0;
            src.seek(SeekFrom::Start(0))?;
            let mss = MediaSourceStream::new(src, Default::default());
            let le = |b: &[u8], i: usize| {
                b.get(i..i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
            };
            let (demux, params, head, sr, tb, frames): (Box<dyn Demux>, _, _, _, _, _) = match ogg
                || flac
            {
                true => {
                    let (o, head, sr) = Raw::open(mss, flac)?;
                    let tb = TimeBase::try_from_recip(sr).ok_or("no time base")?;
                    let (frames, params) = match flac {
                        true => {
                            let mut p = AudioCodecParameters::new();
                            p.for_codec(CODEC_ID_FLAC).with_extra_data(head.clone().into());
                            (Some(o.end), Some(p))
                        }
                        false => (
                            (o.end != u64::MAX).then(|| o.end.saturating_sub(le(&head, 10) as u64)),
                            None,
                        ),
                    };
                    (Box::new(o), params, head, sr, tb, frames)
                }
                false => {
                    let format = symphonia::default::get_probe().probe(
                        &Hint::new(),
                        mss,
                        Default::default(),
                        Default::default(),
                    )?;
                    let t = format.default_track(TrackType::Audio).ok_or("no audio track")?;
                    let p = t
                        .codec_params
                        .as_ref()
                        .and_then(|p| p.audio())
                        .ok_or("no audio codec")?
                        .clone();
                    let sr = p.sample_rate.ok_or("unknown sample rate")?;
                    let tb = t.time_base.or(TimeBase::try_from_recip(sr)).ok_or("no time base")?;
                    let (id, frames, head) =
                        (t.id, t.num_frames, p.extra_data.clone().unwrap_or_default().into());
                    (
                        Box::new((format, id)),
                        Some(p).filter(|p| p.codec != CODEC_ID_OPUS),
                        head,
                        sr,
                        tb,
                        frames,
                    )
                }
            };
            let base = if track.transcoded { offset.trunc() as f64 } else { 0.0 };
            let dur = frames.map_or(track.duration as f64, |n| base + n as f64 / sr as f64);
            let (opus, pre) = (params.is_none(), le(&head, 10));
            let dec: Decode = match params {
                None => {
                    if head.get(9).is_some_and(|&c| c > 2) {
                        Err("multichannel opus unsupported")?
                    }
                    let gain = 10f32.powf(le(&head, 16) as i16 as f32 / 5120.0);
                    let (mut d, mut buf) = (OpusDecoder::new(48_000, 2)?, vec![0f32; 5760 * 2]);
                    Box::new(move |p, out| {
                        let Some(p) = p else {
                            d = OpusDecoder::new(48_000, 2)
                                .map_err(|_| SymError::DecodeError("opus"))?;
                            return Ok(());
                        };
                        let n = d
                            .decode(&p.data, 5760, &mut buf)
                            .map_err(|_| SymError::DecodeError("opus"))?;
                        let e = n.saturating_sub(p.trim_end.get() as usize);
                        out.extend(
                            buf[(p.trim_start.get() as usize).min(e) * 2..e * 2]
                                .iter()
                                .map(|s| s * gain),
                        );
                        Ok(())
                    })
                }
                Some(p) => {
                    let mut d = symphonia::default::get_codecs()
                        .make_audio_decoder(&p, &Default::default())?;
                    let mut buf: Vec<f32> = Vec::new();
                    Box::new(move |p, out| {
                        let Some(p) = p else {
                            d.reset();
                            return Ok(());
                        };
                        let b = d.decode(p)?;
                        let ch = b.spec().channels().count().max(1);
                        b.copy_to_vec_interleaved(&mut buf);
                        out.extend(buf.chunks_exact(ch).flat_map(|f| [f[0], f[ch.min(2) - 1]]));
                        Ok(())
                    })
                }
            };
            let orate = if rate == 0 { sr } else { rate };
            let rs = Source::resampler(orate, sr as f64)?;
            let mut s = Source {
                lead: rs.output_delay(),
                track,
                demux,
                net,
                dec,
                rs,
                orate,
                rel: 1.0,
                direct: false,
                tb,
                rate: sr as f64,
                delay: if opus { pre as f64 / 48_000.0 } else { 0.0 },
                dur,
                base,
                done: 0,
                skip: if opus { pre as usize } else { 0 },
                owed: 0.0,
                eof: false,
                pcm: Vec::new(),
                out: Vec::new(),
            };
            if !s.track.transcoded && offset > 0.0 {
                s.seek(offset)?;
            }
            Ok(s)
        })()
        .map_err(|e| e.to_string().replace(&safe, "<stream>"))
    }

    fn resampler(out: u32, sr: f64) -> Result<Async<f32>, Box<dyn Error>> {
        let params = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
            .interpolation(SincInterpolationType::Linear);
        Ok(Async::new_sinc(out as f64 / sr, 3.5, &params, CHUNK, 2, FixedAsync::Output)?)
    }

    fn speed(&mut self, rel: f64, ramp: bool) -> Result<(), Box<dyn Error>> {
        self.rel = rel;
        Ok(self.rs.set_resample_ratio_relative(rel, ramp)?)
    }

    fn retarget(&mut self, rate: u32) -> Result<(), String> {
        if rate != self.orate {
            self.rs = Source::resampler(rate, self.rate).map_err(|e| e.to_string())?;
            (self.orate, self.lead, self.owed, self.direct) =
                (rate, self.rs.output_delay(), 0.0, false);
        }
        Ok(())
    }

    fn seek(&mut self, secs: f32) -> Result<(), Box<dyn Error>> {
        let target = secs as f64 + self.delay;
        let preroll = if self.delay > 0.0 { 0.08 } else { 0.0 };
        let time = Time::try_from_secs_f64((target - preroll).max(0.0)).ok_or("invalid seek")?;
        let mode = {
            let st = self.net.0.lock().expect("net lock");
            let len = st.len.unwrap_or(0);
            let need = len.min((len as f64 * secs as f64 / self.dur) as u64 + (256 << 10));
            match st.have.first().is_some_and(|r| r.0 == 0 && r.1 >= need) {
                true => SeekMode::Accurate,
                false => SeekMode::Coarse,
            }
        };
        let to = self.demux.seek(self.tb.calc_timestamp(time).ok_or("invalid seek")?, mode)?;
        self.skip = ((target - self.tb.calc_time_saturating(to).as_secs_f64()) * self.rate).max(0.0)
            as usize;
        (self.dec)(None, &mut self.pcm)?;
        self.rs.reset();
        self.pcm.clear();
        self.out.clear();
        (self.lead, self.base, self.done, self.owed, self.eof) =
            (self.rs.output_delay(), secs as f64, 0, 0.0, false);
        Ok(())
    }

    fn pos(&self) -> f32 {
        (self.base + self.done as f64 / self.rate) as f32
    }

    fn fill(&mut self, out: &mut [f32]) -> Result<usize, Box<dyn Error>> {
        let direct = self.rel == 1.0 && self.orate as f64 == self.rate;
        if direct != self.direct {
            self.rs.reset();
            self.rs.set_resample_ratio_relative(self.rel, false)?;
            (self.direct, self.lead, self.owed) = (direct, self.rs.output_delay(), 0.0);
        }
        while self.out.len() < out.len() && !(self.eof && self.pcm.is_empty() && self.owed < 1.0) {
            let need = if direct { CHUNK } else { self.rs.input_frames_next() };
            while !self.eof && self.pcm.len() < need * 2 {
                match self.demux.next() {
                    Ok(Some(p)) => {
                        let n = self.pcm.len();
                        match (self.dec)(Some(&p), &mut self.pcm) {
                            Err(SymError::DecodeError(_)) => {}
                            r => r?,
                        }
                        let k = (self.skip * 2).min(self.pcm.len() - n);
                        self.pcm.drain(n..n + k);
                        self.skip -= k / 2;
                    }
                    Ok(None) => self.eof = true,
                    Err(SymError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                        self.eof = true
                    }
                    Err(e) => Err(e)?,
                }
            }
            if direct {
                let k = self.pcm.len().min(CHUNK * 2);
                self.out.extend(self.pcm.drain(..k));
                self.done += k as u64 / 2;
                continue;
            }
            let (avail, n) = ((self.pcm.len() / 2).min(need), self.out.len());
            self.out.resize(n + CHUNK * 2, 0.0);
            let idx = Indexing {
                input_offset: 0,
                output_offset: 0,
                partial_len: (avail < need).then_some(avail),
                active_channels_mask: None,
            };
            let input = InterleavedSlice::new(&self.pcm, 2, self.pcm.len() / 2)?;
            let (nin, nout) = self.rs.process_into_buffer(
                &input,
                &mut InterleavedSlice::new_mut(&mut self.out[n..], 2, CHUNK)?,
                Some(&idx),
            )?;
            let nin = nin.min(avail);
            self.owed += nin as f64 * self.rs.resample_ratio();
            self.pcm.drain(..nin * 2);
            self.done += nin as u64;
            let drop = self.lead.min(nout);
            let keep = if avail < need {
                (nout - drop).min(self.owed.round().max(0.0) as usize)
            } else {
                nout - drop
            };
            (self.lead, self.owed) = (self.lead - drop, self.owed - keep as f64);
            self.out.truncate(n + (drop + keep) * 2);
            self.out.drain(n..n + drop * 2);
        }
        let k = self.out.len().min(out.len());
        out[..k].iter_mut().zip(self.out.drain(..k)).for_each(|(o, s)| *o = s * self.track.gain);
        Ok(k / 2)
    }
}

struct Mixer {
    io: Io,
    rate: u32,
    cur: Option<Source>,
    inc: Option<Source>,
    old: Option<Source>,
    fade: (usize, usize),
    next: Option<Track>,
    pre: Option<(Track, JoinHandle<Result<Source, String>>)>,
    b: Vec<f32>,
}

impl Mixer {
    fn new(io: Io, rate: u32) -> Mixer {
        Mixer {
            io,
            rate,
            cur: None,
            inc: None,
            old: None,
            fade: (0, 1),
            next: None,
            pre: None,
            b: vec![0.0; CHUNK * 2],
        }
    }

    fn take_pre(
        pre: &mut Option<(Track, JoinHandle<Result<Source, String>>)>,
        rate: u32,
    ) -> Option<Result<Source, String>> {
        let s = pre.take()?.1.join().expect("open thread");
        Some(s.and_then(|mut s| s.retarget(rate).map(|()| s)))
    }

    fn seek(&mut self, secs: f32) -> Result<(), String> {
        if let Some(i) = self.inc.take() {
            self.cur = Some(i);
        }
        match self.cur.take() {
            Some(c) if c.track.transcoded => {
                self.cur = Some(Source::open(c.track, &self.io, self.rate, secs)?)
            }
            Some(mut c) => {
                c.seek(secs).map_err(|e| e.to_string())?;
                self.cur = Some(c);
            }
            None => {}
        }
        Ok(())
    }

    fn step(
        &mut self,
        out: &mut [f32],
        speed: f32,
        cf: f32,
    ) -> Result<(usize, Option<Event>), Box<dyn Error>> {
        let rel = 1.0 / speed as f64;
        self.cur.iter_mut().chain(self.inc.iter_mut()).try_for_each(|s| s.speed(rel, true))?;
        let Some(cur) = &mut self.cur else { return Ok((0, None)) };
        let rem = cur.dur - cur.pos() as f64;
        let cf = (cf as f64).min(cur.dur / 2.0);
        if self.pre.is_none()
            && let Some(t) = self.next.take()
        {
            let (io, r) = (self.io.clone(), self.rate);
            self.pre = Some((t.clone(), thread::spawn(move || Source::open(t, &io, r, 0.0))));
        }
        let mut ev = None;
        if cf > 0.0
            && self.inc.is_none()
            && rem <= cf
            && self.pre.as_ref().is_some_and(|p| p.1.is_finished())
        {
            match Mixer::take_pre(&mut self.pre, self.rate) {
                Some(Ok(mut s)) => {
                    s.speed(rel, false)?;
                    ev = Some(Event::Started(s.track.id.clone()));
                    self.fade = (0, (rem.max(0.05) * rel * self.rate as f64) as usize);
                    self.inc = Some(s);
                }
                e => ev = e.and_then(Result::err).map(Event::Error),
            }
        }
        let n = cur.fill(out)?;
        out[n * 2..].fill(0.0);
        if let Some(inc) = &mut self.inc {
            let m = inc.fill(&mut self.b)?;
            self.b[m * 2..].fill(0.0);
            let (pos, len) = self.fade;
            out.as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(self.b.as_chunks::<2>().0.iter())
                .enumerate()
                .for_each(|(i, (o, b))| {
                    let (s, c) = (((pos + i) as f32 / len as f32).min(1.0) * FRAC_PI_2).sin_cos();
                    o[0] = o[0] * c + b[0] * s;
                    o[1] = o[1] * c + b[1] * s;
                });
            self.fade.0 += out.len() / 2;
            if self.fade.0 >= len || n < out.len() / 2 {
                self.cur = self.inc.take();
            }
            return Ok((n.max(m), ev));
        }
        if n == out.len() / 2 {
            return Ok((n, ev));
        }
        let next = Mixer::take_pre(&mut self.pre, self.rate)
            .or_else(|| self.next.take().map(|t| Source::open(t, &self.io, self.rate, 0.0)));
        self.cur = None;
        match next {
            Some(Ok(mut s)) => {
                s.speed(rel, false)?;
                let m = s.fill(&mut out[n * 2..])?;
                let id = s.track.id.clone();
                self.cur = Some(s);
                Ok((n + m, Some(Event::Started(id))))
            }
            Some(Err(e)) => Ok((n, Some(Event::Error(e)))),
            None => Ok((n, Some(Event::Ended))),
        }
    }
}

#[doc(hidden)]
pub fn render(
    io: Io,
    tracks: Vec<Track>,
    rate: u32,
    seek: f32,
    crossfade: f32,
    speed: f32,
) -> Result<Vec<f32>, String> {
    let mut mix = Mixer::new(io, rate);
    let mut it = tracks.into_iter();
    mix.cur = Some(Source::open(it.next().ok_or("no tracks")?, &mix.io, rate, seek)?);
    mix.next = it.next();
    let (mut out, mut buf) = (Vec::new(), vec![0.0; CHUNK * 2]);
    while mix.cur.is_some() {
        while mix.pre.as_ref().is_some_and(|p| !p.1.is_finished()) {
            thread::sleep(Duration::from_millis(1));
        }
        let (n, ev) = mix.step(&mut buf, speed, crossfade).map_err(|e| e.to_string())?;
        match ev {
            Some(Event::Error(e)) => return Err(e),
            Some(Event::Started(_)) => mix.next = it.next(),
            _ => {}
        }
        out.extend_from_slice(&buf[..n * 2]);
    }
    Ok(out)
}

fn engine(rx: Receiver<Cmd>, tx: UnboundedSender<Event>, sh: Arc<Shared>, io: Io) {
    let me = thread::current();
    let open = |id: &str, want: u32| -> Result<(cpal::Stream, Producer<f32>, u32), String> {
        let host = cpal::default_host();
        let dev = match id {
            "" => host.default_output_device(),
            id => host.device_by_id(&id.parse().map_err(|e: cpal::Error| e.to_string())?),
        }
        .ok_or("The audio output device is not available. Select a device in Settings.")?;
        let sup = dev
            .supported_output_configs()
            .into_iter()
            .flatten()
            .find(|c| {
                c.channels() == 2 && (c.min_sample_rate()..=c.max_sample_rate()).contains(&want)
            })
            .map(|c| c.with_sample_rate(want));
        let sup = sup.map_or_else(|| dev.default_output_config(), Ok).map_err(|e| e.to_string())?;
        let rate = sup.sample_rate();
        let buffer_size = match *sup.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                cpal::BufferSize::Fixed((rate / 40).clamp(min, max))
            }
            cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
        };
        let (prod, mut cons) = RingBuffer::<f32>::new(rate as usize / 2);
        let (csh, etx, me, half) = (sh.clone(), tx.clone(), me.clone(), rate as usize / 4);
        let (mut read, mut vol, ramp) = (0u64, 0f32, 1000.0 / rate as f32);
        let config = cpal::StreamConfig { channels: 2, sample_rate: rate, buffer_size };
        let stream = dev
            .build_output_stream(
                config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let stale = csh.flush.load(Relaxed).saturating_sub(read) as usize;
                    if stale > 0
                        && let Ok(c) = cons.read_chunk(stale.min(cons.slots()))
                    {
                        read += c.len() as u64;
                        c.commit_all();
                    }
                    let n = cons.pop_partial_slice(out).0.len();
                    out[n..].fill(0.0);
                    let target = f32::from_bits(csh.volume.load(Relaxed)).powi(3);
                    out.as_chunks_mut::<2>().0.iter_mut().for_each(|f| {
                        vol += (target - vol) * ramp;
                        f[0] *= vol;
                        f[1] *= vol;
                    });
                    read += n as u64;
                    csh.read.store(read, Relaxed);
                    if cons.slots() < half {
                        me.unpark();
                    }
                },
                move |e| drop(etx.send(Event::Error(e.to_string()))),
                None,
            )
            .map_err(|e| e.to_string())?;
        Ok((stream, prod, rate))
    };
    let mut mix = Mixer::new(io, 0);
    let mut out: Option<(cpal::Stream, Producer<f32>)> = None;
    let (mut written, mut running, mut idle, mut dev) =
        (0u64, false, None::<Instant>, String::new());
    let mut buf = vec![0f32; CHUNK * 2];
    let (mut marks, mut events) = (VecDeque::<(u64, f32)>::new(), VecDeque::<(u64, Event)>::new());
    loop {
        let mut play = None;
        loop {
            let cmd = match rx.try_recv() {
                Ok(c) => c,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            };
            if !matches!(cmd, Cmd::Next(_) | Cmd::Gain(_)) {
                sh.flush.store(written, Relaxed);
                marks.clear();
                events.clear();
            }
            match cmd {
                Cmd::Play(t, at) => {
                    let pre = mix.pre.take_if(|p| at == 0.0 && p.0 == t).map(|p| p.1);
                    let mut was = [mix.cur.take(), mix.old.take()];
                    let hit = was
                        .iter_mut()
                        .find(|s| {
                            s.as_ref().is_some_and(|s| {
                                !t.transcoded && s.track.id == t.id && s.track.url == t.url
                            })
                        })
                        .and_then(Option::take);
                    mix.old = was.into_iter().flatten().next();
                    (mix.inc, play) = (None, Some((t, at, pre, hit)));
                }
                Cmd::Next(t) if mix.pre.as_ref().is_some_and(|p| Some(&p.0) == t.as_ref()) => {}
                Cmd::Next(t) => (mix.next, mix.pre) = (t, None),
                Cmd::Gain(g) => mix.cur.iter_mut().for_each(|c| c.track.gain = g),
                Cmd::Device(d) => (dev, out) = (d, None),
                Cmd::Seek(s) => {
                    if let Err(e) = mix.seek(s) {
                        (mix.cur, mix.inc) = (None, None);
                        let _ = tx.send(Event::Error(e));
                    }
                }
                Cmd::Stop => {
                    (mix.cur, mix.inc) = (None, None);
                    sh.pos.store(0, Relaxed);
                }
            }
        }
        let paused = sh.paused.load(Relaxed);
        let fresh = play.is_some();
        if let Some((t, at, pre, hit)) = play {
            let s = match (hit, pre) {
                (Some(mut s), _) => {
                    s.seek(at).map(|()| (s.track = t, s).1).map_err(|e| e.to_string())
                }
                (None, Some(h)) => h.join().expect("open thread"),
                (None, None) => Source::open(t, &mix.io, mix.rate, at),
            };
            match s {
                Ok(s) => mix.cur = Some(s),
                Err(e) => drop(tx.send(Event::Error(e))),
            }
            if mix.cur.as_ref().is_some_and(|c| c.rate != mix.rate as f64) {
                out = None;
            }
        }
        if out.is_none() && !paused && mix.cur.is_some() {
            let want = mix.cur.as_ref().map_or(0, |c| c.rate as u32);
            let pos = f32::from_bits(sh.pos.load(Relaxed));
            let r = open(&dev, want).and_then(|(s, p, rate)| {
                mix.rate = rate;
                mix.cur.iter_mut().chain(mix.inc.iter_mut()).try_for_each(|c| c.retarget(rate))?;
                if !fresh {
                    mix.seek(pos)?;
                }
                Ok((s, p))
            });
            match r {
                Ok(o) => {
                    (written, running, idle) = (0, false, None);
                    sh.read.store(0, Relaxed);
                    sh.flush.store(0, Relaxed);
                    marks.clear();
                    events.clear();
                    out = Some(o);
                }
                Err(e) => {
                    (mix.cur, mix.inc) = (None, None);
                    let _ = tx.send(Event::Error(e));
                }
            }
        }
        if out.is_some()
            && let Some(c) = &mut mix.cur
            && let Err(e) = c.retarget(mix.rate)
        {
            mix.cur = None;
            let _ = tx.send(Event::Error(e));
        }
        if let Some((_, prod)) = &mut out {
            while !paused && mix.cur.is_some() && prod.slots() >= buf.len() {
                let (speed, cf) = (
                    f32::from_bits(sh.speed.load(Relaxed)),
                    f32::from_bits(sh.crossfade.load(Relaxed)),
                );
                let (n, ev) = mix.step(&mut buf, speed, cf).unwrap_or_else(|e| {
                    (mix.cur, mix.inc) = (None, None);
                    (0, Some(Event::Error(e.to_string())))
                });
                prod.push_entire_slice(&buf[..n * 2]).expect("ring buffer has room");
                written += n as u64 * 2;
                marks.extend(mix.inc.as_ref().or(mix.cur.as_ref()).map(|s| (written, s.pos())));
                events.extend(ev.map(|e| (written, e)));
            }
        }
        let read = sh.read.load(Relaxed);
        while let Some((_, p)) = marks.front().filter(|m| m.0 <= read) {
            sh.pos.store(p.to_bits(), Relaxed);
            marks.pop_front();
        }
        while events.front().is_some_and(|e| e.0 <= read) {
            let _ = events.pop_front().map(|e| tx.send(e.1));
        }
        let want = !paused && (mix.cur.is_some() || !events.is_empty());
        if let Some((s, _)) = &out
            && want != running
        {
            running = want;
            if let Err(e) = if want { s.play() } else { s.pause() } {
                let _ = tx.send(Event::Error(e.to_string()));
            }
        }
        let left = idle.get_or_insert_with(Instant::now).elapsed();
        match (&out, want) {
            (Some(_), true) => {
                idle = None;
                thread::park();
            }
            (Some(_), false) if left < CLOSE => thread::park_timeout(CLOSE - left),
            _ => {
                (out, idle) = (None, None);
                thread::park();
            }
        }
    }
}
