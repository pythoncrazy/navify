use std::collections::VecDeque;
use std::error::Error;
use std::f32::consts::FRAC_PI_2;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::{Client, Response, Url};
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
use tokio::task::JoinSet;

const CHUNK: usize = 512;
const CLOSE: Duration = Duration::from_secs(30);
const TAIL: u64 = 64 * 1024;
const AHEAD: u64 = 4 << 20;
const BACK: u64 = 1 << 20;

type Io = (Handle, Client);
type Res<T> = Result<T, Box<dyn Error>>;

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
        .map(|d| {
            Ok((
                d.id().map_err(|e| e.to_string())?.to_string(),
                d.to_string(),
            ))
        })
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

fn get(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Relaxed))
}

pub struct Player {
    tx: Sender<Cmd>,
    sh: Arc<Shared>,
    th: Thread,
}

impl Player {
    pub fn new(rt: Handle, http: Client) -> (Player, UnboundedReceiver<Event>) {
        let (tx, rx) = channel();
        let (etx, erx) = unbounded_channel();
        let sh = Arc::new(Shared::default());
        sh.volume.store(1f32.to_bits(), Relaxed);
        sh.speed.store(1f32.to_bits(), Relaxed);
        let esh = sh.clone();
        let th = thread::spawn(move || engine(rx, etx, esh, (rt, http)));
        let th = th.thread().clone();
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
        get(&self.sh.pos)
    }
}

#[derive(Default)]
struct Net0 {
    start: u64,
    data: Vec<u8>,
    tail: (u64, Vec<u8>),
    len: Option<u64>,
    want: u64,
    jump: bool,
    busy: bool,
    rate: f64,
    ttfb: f64,
    err: Option<String>,
    eof: bool,
}

impl Net0 {
    fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }
}

#[derive(Default)]
struct Net(Mutex<Net0>, Condvar, tokio::sync::Notify);

impl Net {
    fn st(&self) -> MutexGuard<'_, Net0> {
        self.0.lock().expect("net lock")
    }
}

fn range(r: &Response) -> Option<(u64, u64)> {
    let v = r.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    let (span, len) = v.strip_prefix("bytes ")?.split_once('-')?;
    Some((span.parse().ok()?, len.split_once('/')?.1.parse().ok()?))
}

async fn fetch(net: Arc<Net>, http: Client, url: Url, mut first: Option<Response>) {
    let res = async {
        loop {
            let mut resp = match first.take() {
                Some(r) => r,
                None => {
                    let from = loop {
                        {
                            let mut st = net.st();
                            let Some(len) = st.len else { return Ok(()) };
                            if std::mem::take(&mut st.jump) {
                                (st.start, st.data) = (st.want, Vec::new());
                            }
                            st.busy = st.end() < len && st.end() < st.want + AHEAD;
                            if st.busy {
                                break st.end();
                            }
                        }
                        net.2.notified().await;
                    };
                    let (t, r) = (Instant::now(), http.get(url.clone()));
                    let r = r.header(RANGE, format!("bytes={from}-")).send().await?;
                    net.st().ttfb = t.elapsed().as_secs_f64();
                    r.error_for_status()?
                }
            };
            let (t, mut n) = (Instant::now(), 0);
            while let Some(c) = resp.chunk().await? {
                let mut st = net.st();
                st.data.extend_from_slice(&c);
                n += c.len();
                st.rate = n as f64 / (t.elapsed().as_secs_f64() + st.ttfb);
                let cut = st.want.saturating_sub(BACK).saturating_sub(st.start);
                let cut = cut.min(st.data.len() as u64);
                if cut > AHEAD {
                    st.data.drain(..cut as usize);
                    st.start += cut;
                }
                net.1.notify_all();
                if st.jump || st.len.is_some_and(|_| st.end() > st.want + AHEAD) {
                    break;
                }
            }
            let mut st = net.st();
            st.eof |= st.len.is_none();
            if st.eof {
                return Ok::<_, reqwest::Error>(());
            }
        }
    }
    .await;
    net.st().err = res.err().map(|e| e.without_url().to_string());
    net.1.notify_all();
}

struct Http {
    net: Arc<Net>,
    pos: u64,
    _task: JoinSet<()>,
}

impl Read for Http {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let (pos, mut st) = (self.pos, self.net.st());
        let tail = (st.tail.0..st.tail.0 + st.tail.1.len() as u64).contains(&pos);
        if !tail {
            st.want = pos;
        }
        loop {
            let (at, d) = if tail {
                (st.tail.0, &st.tail.1)
            } else {
                (st.start, &st.data)
            };
            let d = pos.checked_sub(at).and_then(|i| d.get(i as usize..));
            if let Some(d) = d.filter(|d| !d.is_empty()) {
                let n = buf.len().min(d.len());
                buf[..n].copy_from_slice(&d[..n]);
                self.pos += n as u64;
                let end = st.end();
                if !tail && !st.busy && end - pos < AHEAD / 2 && st.len.is_some_and(|l| end < l) {
                    self.net.2.notify_one();
                }
                return Ok(n);
            }
            if let Some(e) = &st.err {
                return Err(std::io::Error::other(e.clone()));
            }
            if st.len.is_none() && pos < st.start {
                return Err(ErrorKind::Unsupported.into());
            }
            if st.eof || st.len.is_some_and(|l| pos >= l) {
                return Ok(0);
            }
            let far = pos.wrapping_sub(st.end()) as f64 > st.rate * st.ttfb + 65536.0;
            st.jump |= st.len.is_some() && far;
            if !st.busy {
                self.net.2.notify_one();
            }
            st = self.net.1.wait(st).expect("net lock");
        }
    }
}

impl Seek for Http {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let len = self.byte_len().ok_or(ErrorKind::Unsupported);
        self.pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(d) => self.pos.saturating_add_signed(d),
            SeekFrom::End(d) => len?.saturating_add_signed(d),
        };
        Ok(self.pos)
    }
}

impl MediaSource for Http {
    fn is_seekable(&self) -> bool {
        self.byte_len().is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.net.st().len
    }
}

trait Demux: Send {
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
        let to = SeekTo::Timestamp {
            ts,
            track_id: self.1,
        };
        Ok(self.0.seek(mode, to)?.actual_ts)
    }
}

type Page = (u8, u64, u32, Vec<u8>, Vec<u8>);

struct Raw {
    r: MediaSourceStream<'static>,
    block: Option<u64>,
    serial: u32,
    data: u64,
    end: u64,
    pk: VecDeque<Packet>,
    part: Option<Vec<u8>>,
    prev: Option<u64>,
    big: u64,
}

impl Raw {
    fn open(r: MediaSourceStream<'static>, flac: bool) -> Res<(Raw, Vec<u8>, u32)> {
        let len = r.byte_len().unwrap_or(0);
        let (pk, part, prev) = (VecDeque::new(), None, Some(0));
        let mut o = Raw {
            r,
            block: None,
            serial: 0,
            data: 0,
            end: u64::MAX,
            pk,
            part,
            prev,
            big: 8192,
        };
        let (head, sr) = if flac {
            let (mut h, mut si) = ([0u8; 4], vec![0u8; 34]);
            o.r.read_exact(&mut h)?;
            while h[0] & 128 == 0 {
                o.r.read_exact(&mut h)?;
                match h[0] & 127 {
                    0 => o.r.read_exact(&mut si)?,
                    _ => {
                        o.r.ignore_bytes((u32::from_be_bytes(h) & 0xFFFFFF).into())?
                    }
                }
            }
            o.end = u64::from_be_bytes([0, 0, 0, si[13] & 15, si[14], si[15], si[16], si[17]]);
            o.block = Some(u16::from_be_bytes([si[2], si[3]]).into());
            let sr = u32::from_be_bytes([0, si[10], si[11], si[12]]) >> 4;
            (si, sr)
        } else {
            let head = Raw::page(&mut o.r)?.ok_or("The Ogg stream is empty.")?;
            loop {
                let p = Raw::page(&mut o.r)?.ok_or("The Ogg stream has no audio.")?;
                if p.2 == head.2 && p.3.iter().any(|&l| l < 255) {
                    break;
                }
            }
            o.serial = head.2;
            if len > 0 {
                let (data, mut t) = (o.r.pos(), Vec::new());
                o.r.seek(SeekFrom::Start(len.saturating_sub(TAIL)))?;
                o.r.read_to_end(&mut t)?;
                let i = (0..t.len().saturating_sub(26))
                    .rev()
                    .find(|&i| {
                        t[i..i + 4] == *b"OggS" && t[i + 14..i + 18] == o.serial.to_le_bytes()
                    })
                    .ok_or("The Ogg stream has no end.")?;
                o.end = u64::from_le_bytes(t[i + 6..i + 14].try_into()?);
                o.r.seek(SeekFrom::Start(data))?;
            }
            (head.4, 48_000)
        };
        o.data = o.r.pos();
        Ok((o, head, sr))
    }

    fn page(r: &mut MediaSourceStream<'static>) -> std::io::Result<Option<Page>> {
        let mut w = 0u32;
        let res: std::io::Result<Page> = (|| loop {
            w = w << 8 | r.read_byte()? as u32;
            if w != u32::from_be_bytes(*b"OggS") {
                continue;
            }
            let mut h = [0u8; 23];
            r.read_exact(&mut h)?;
            if h[0] != 0 || h[1] > 7 {
                r.seek(SeekFrom::Current(-23))?;
                w = 0;
                continue;
            }
            let mut lace = vec![0; h[22] as usize];
            r.read_exact(&mut lace)?;
            let mut data = vec![0; lace.iter().map(|&l| l as usize).sum()];
            r.read_exact(&mut data)?;
            let gran = u64::from_le_bytes(h[2..10].try_into().expect("8 bytes"));
            let serial = u32::from_le_bytes(h[10..14].try_into().expect("4 bytes"));
            return Ok((h[1], gran, serial, lace, data));
        })();
        match res {
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(None),
            r => r.map(Some),
        }
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
                (c & 0xC0 == 0x80).then_some(())?;
                num = num << 6 | (c & 0x3F) as u64;
            }
            let i = 4 + k;
            let (n, x) = match b[2] >> 4 {
                1 => (192, 0),
                c @ 2..=5 => (576 << (c - 2), 0),
                6 => (b[i] as u64 + 1, 1),
                7 => (u16::from_be_bytes([b[i], b[i + 1]]) as u64 + 1, 2),
                c => (256 << (c - 8), 0),
            };
            let i = i + x + [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 0][b[2] as usize & 15];
            let crc = b[..i].iter().fold(0u8, |c, &x| {
                (0..8).fold(c ^ x, |c, _| (c << 1) ^ ((c >> 7) * 7))
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
                    Some((_, s0, n0)) if s == s0 + n0 => {
                        self.r.seek(SeekFrom::Start(at + i as u64))?;
                        buf.truncate(i);
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
            let data = buf.split_off(j);
            self.pk
                .push_back(Packet::new(0, Timestamp::new(s as i64), n.into(), data));
            self.prev = Some(s + n);
            s + n
        }))
    }

    fn unit(&mut self) -> Result<Option<u64>, SymError> {
        if let Some(b) = self.block {
            return Ok(self.frame(b)?);
        }
        let (flags, gran, lace, data) = loop {
            match Raw::page(&mut self.r)? {
                Some((f, g, s, l, d)) if s == self.serial => break (f, g, l, d),
                Some(_) => {}
                None => return Ok(None),
            }
        };
        self.big = self.big.max((27 + lace.len() + data.len()) as u64);
        let mut part = if flags & 1 != 0 {
            self.part.take()
        } else {
            Some(Vec::new())
        };
        let (mut off, mut got) = (0, Vec::new());
        for &l in &lace {
            if let Some(v) = &mut part {
                v.extend_from_slice(&data[off..off + l as usize]);
            }
            off += l as usize;
            if l < 255 {
                got.extend(part.replace(Vec::new()));
            }
        }
        self.part = part;
        if gran == u64::MAX || got.is_empty() {
            return Ok(Some(gran));
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
        let eos = flags & 4 != 0;
        let sum: u64 = got.iter().map(|d| dur(d)).sum();
        let mut t = self
            .prev
            .filter(|_| eos)
            .unwrap_or(gran.saturating_sub(sum));
        let end = if eos { gran } else { u64::MAX };
        for d in got {
            let n = dur(&d);
            let mut pk = Packet::new(0, Timestamp::new(t as i64), n.into(), d);
            pk.trim_end = (t + n).saturating_sub(end).min(n).into();
            self.pk.push_back(pk);
            t += n;
        }
        self.prev = Some(gran);
        Ok(Some(gran))
    }
}

impl Demux for Raw {
    fn next(&mut self) -> Result<Option<Packet>, SymError> {
        while self.pk.is_empty() && self.unit()?.is_some() {}
        Ok(self.pk.pop_front())
    }

    fn seek(&mut self, ts: Timestamp, _: SeekMode) -> Result<Timestamp, SymError> {
        let t = (ts.get().max(0) as u64).min(self.end.saturating_sub(1));
        let len = self.r.byte_len().unwrap_or(0);
        let (mut lo, mut hi) = ((self.data, 0), (len, self.end.max(1)));
        'probe: for _ in 0..32 {
            let bps = (hi.0 - lo.0) as f64 / (hi.1 - lo.1).max(1) as f64;
            let est = lo.0 + ((t - lo.1) as f64 * bps) as u64;
            let at = est
                .saturating_sub((hi.0 - lo.0) / 64 + 2 * self.big)
                .max(lo.0);
            self.r.seek(SeekFrom::Start(at))?;
            (self.pk, self.part, self.prev) = (VecDeque::new(), None, None);
            let mut g = loop {
                let Some(g) = self.unit()? else {
                    hi.0 = at;
                    continue 'probe;
                };
                let Some(start) = self.pk.front().map(|p| p.pts.get() as u64) else {
                    continue;
                };
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
    rs: Option<Async<f32>>,
    orate: u32,
    rel: f64,
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
    off: usize,
    out: Vec<f32>,
}

impl Source {
    fn open(track: Track, (rt, http): &Io, rate: u32, offset: f32) -> Result<Source, String> {
        let mut url = track.url.clone();
        if track.transcoded && offset >= 1.0 {
            url += &format!("&timeOffset={}", offset as u32);
        }
        (|| -> Res<Source> {
            let url: Url = url.parse().map_err(|_| "invalid stream url")?;
            let net = Arc::new(Net::default());
            let head = rt.block_on(async {
                let (t, get) = (Instant::now(), http.get(url.clone()));
                if track.transcoded {
                    return Ok::<_, Box<dyn Error>>(get.send().await?.error_for_status()?);
                }
                let tail = http
                    .get(url.clone())
                    .header(RANGE, format!("bytes=-{TAIL}"));
                let (head, tail) =
                    tokio::try_join!(get.header(RANGE, "bytes=0-").send(), tail.send())?;
                let (head, tail) = (head.error_for_status()?, tail.error_for_status()?);
                let ttfb = t.elapsed().as_secs_f64();
                let len = range(&head).map(|r| r.1).or(head.content_length());
                let at = range(&tail).map_or(0, |r| r.0);
                let tail = (at, tail.bytes().await?.to_vec());
                let mut st = net.st();
                (st.ttfb, st.tail) = (ttfb, tail);
                st.len = Some(len.ok_or("The server did not send the stream length.")?);
                Ok(head)
            })?;
            net.st().busy = true;
            let mut task = JoinSet::new();
            task.spawn_on(fetch(net.clone(), http.clone(), url, Some(head)), rt);
            let mut src = Http {
                net: net.clone(),
                pos: 0,
                _task: task,
            };
            let mut h = [0u8; 42];
            let ok = src.read_exact(&mut h).is_ok();
            src.pos = 0;
            let ogg = ok && h[..4] == *b"OggS" && h[28..36] == *b"OpusHead";
            let flac = ok
                && h[..4] == *b"fLaC"
                && h[4] & 127 == 0
                && (h[21] & 15 | h[22] | h[23] | h[24] | h[25]) != 0;
            let mss = MediaSourceStream::new(Box::new(src), Default::default());
            let (demux, params, head, sr, tb, frames): (Box<dyn Demux>, _, Vec<u8>, _, _, _) =
                if ogg || flac {
                    let (o, head, sr) = Raw::open(mss, flac)?;
                    let mut p = AudioCodecParameters::new();
                    p.for_codec(CODEC_ID_FLAC)
                        .with_extra_data(head.clone().into());
                    let frames = (o.end != u64::MAX).then_some(o.end);
                    (Box::new(o), flac.then_some(p), head, sr, None, frames)
                } else {
                    let probe = symphonia::default::get_probe();
                    let format =
                        probe.probe(&Hint::new(), mss, Default::default(), Default::default())?;
                    let t = format.default_track(TrackType::Audio);
                    let a = t.and_then(|t| Some((t, t.codec_params.as_ref()?.audio()?)));
                    let (t, p) = a.ok_or("The stream has no audio.")?;
                    let (id, tb, frames, p) = (t.id, t.time_base, t.num_frames, p.clone());
                    let head = p.extra_data.clone().unwrap_or_default().into();
                    let sr = p.sample_rate.ok_or("unknown sample rate")?;
                    let p = Some(p).filter(|p| p.codec != CODEC_ID_OPUS);
                    (Box::new((format, id)), p, head, sr, tb, frames)
                };
            let tb = tb.or(TimeBase::try_from_recip(sr)).ok_or("no time base")?;
            let base = if track.transcoded {
                offset.trunc() as f64
            } else {
                0.0
            };
            let opus = params.is_none();
            if opus && (head.len() < 19 || head[9] > 2) {
                Err("The Opus stream has more than two channels or a bad header.")?
            }
            let le = |i: usize| u16::from_le_bytes([head[i], head[i + 1]]);
            let (pre, sr) = (if opus { le(10) } else { 0 }, sr as f64);
            let dur = frames.map_or(track.duration as f64, |n| {
                base + n.saturating_sub(pre.into()) as f64 / sr
            });
            let dec: Decode = match params {
                None => {
                    let gain = 10f32.powf(le(16) as i16 as f32 / 5120.0);
                    let (mut d, mut buf) = (OpusDecoder::new(48_000, 2)?, vec![0f32; 5760 * 2]);
                    Box::new(move |p, out| {
                        let e = |_| SymError::DecodeError("opus");
                        let Some(p) = p else {
                            d = OpusDecoder::new(48_000, 2).map_err(e)?;
                            return Ok(());
                        };
                        let n = d.decode(&p.data, 5760, &mut buf).map_err(e)?;
                        let e = n.saturating_sub(p.trim_end.get() as usize);
                        let s = (p.trim_start.get() as usize).min(e);
                        out.extend(buf[s * 2..e * 2].iter().map(|s| s * gain));
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
            let mut s = Source {
                orate: if rate == 0 { sr as u32 } else { rate },
                track,
                demux,
                net,
                dec,
                rs: None,
                rel: 1.0,
                tb,
                rate: sr,
                delay: pre as f64 / 48_000.0,
                dur,
                base,
                done: 0,
                skip: pre as usize,
                lead: 0,
                owed: 0.0,
                eof: false,
                pcm: Vec::new(),
                off: 0,
                out: Vec::new(),
            };
            if !s.track.transcoded && offset > 0.0 {
                s.seek(offset)?;
            }
            Ok(s)
        })()
        .map_err(|e| e.to_string().replace(&url, "<stream>"))
    }

    fn tune(&mut self, rate: u32, rel: f64) -> Res<()> {
        if rate != self.orate {
            (self.orate, self.rs) = (rate, None);
        }
        if rel != self.rel {
            self.rel = rel;
            self.rs
                .iter_mut()
                .try_for_each(|r| r.set_resample_ratio_relative(rel, true))?;
        }
        Ok(())
    }

    fn seek(&mut self, secs: f32) -> Res<()> {
        let target = secs as f64 + self.delay;
        let preroll = if self.delay > 0.0 { 0.08 } else { 0.0 };
        let time = Time::try_from_secs_f64((target - preroll).max(0.0)).ok_or("invalid seek")?;
        let mode = {
            let st = self.net.st();
            let len = st.len.unwrap_or(0);
            let need = len.min((len as f64 * secs as f64 / self.dur) as u64 + (256 << 10));
            if st.start == 0 && st.end() >= need {
                SeekMode::Accurate
            } else {
                SeekMode::Coarse
            }
        };
        let to = self
            .demux
            .seek(self.tb.calc_timestamp(time).ok_or("invalid seek")?, mode)?;
        let at = self.tb.calc_time_saturating(to).as_secs_f64();
        self.skip = ((target - at) * self.rate).max(0.0) as usize;
        (self.dec)(None, &mut self.pcm)?;
        (self.rs, self.off, self.base, self.done, self.eof) = (None, 0, secs as f64, 0, false);
        self.pcm.clear();
        self.out.clear();
        Ok(())
    }

    fn pos(&self) -> f32 {
        (self.base + self.done as f64 / self.rate) as f32
    }

    fn fill(&mut self, out: &mut [f32]) -> Res<usize> {
        if self.rel == 1.0 && self.orate as f64 == self.rate {
            self.rs = None;
        } else if self.rs.is_none() {
            let p = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
                .interpolation(SincInterpolationType::Linear);
            let ratio = self.orate as f64 / self.rate;
            let mut rs = Async::new_sinc(ratio, 3.5, &p, CHUNK, 2, FixedAsync::Output)?;
            rs.set_resample_ratio_relative(self.rel, false)?;
            (self.lead, self.owed) = (rs.output_delay(), 0.0);
            self.rs = Some(rs);
        }
        let (gain, mut k) = (self.track.gain, 0);
        while k < out.len() {
            if !self.out.is_empty() {
                let m = self.out.len().min(out.len() - k);
                out[k..k + m]
                    .iter_mut()
                    .zip(self.out.drain(..m))
                    .for_each(|(o, s)| *o = s * gain);
                k += m;
                continue;
            }
            let need = self.rs.as_ref().map_or(CHUNK, |r| r.input_frames_next()) * 2;
            if self.pcm.len() - self.off < need {
                self.pcm.drain(..self.off);
                self.off = 0;
            }
            while !self.eof && self.pcm.len() < need {
                let p = match self.demux.next() {
                    Err(SymError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => None,
                    r => r?,
                };
                let Some(p) = p else {
                    self.eof = true;
                    break;
                };
                let n = self.pcm.len();
                match (self.dec)(Some(&p), &mut self.pcm) {
                    Err(SymError::DecodeError(_)) => {}
                    r => r?,
                }
                let k = (self.skip * 2).min(self.pcm.len() - n);
                self.pcm.drain(n..n + k);
                self.skip -= k / 2;
            }
            let (pcm, avail) = (&self.pcm[self.off..], (self.pcm.len() - self.off) / 2);
            let Some(rs) = &mut self.rs else {
                let m = (avail * 2).min(out.len() - k);
                if m == 0 {
                    break;
                }
                out[k..k + m]
                    .iter_mut()
                    .zip(pcm)
                    .for_each(|(o, s)| *o = s * gain);
                (k, self.off, self.done) = (k + m, self.off + m, self.done + m as u64 / 2);
                continue;
            };
            if self.eof && avail == 0 && self.owed < 1.0 {
                break;
            }
            let partial = avail < need / 2;
            let idx = Indexing {
                partial_len: partial.then_some(avail),
                ..Default::default()
            };
            self.out.resize(CHUNK * 2, 0.0);
            let (nin, nout) = rs.process_into_buffer(
                &InterleavedSlice::new(pcm, 2, avail)?,
                &mut InterleavedSlice::new_mut(&mut self.out, 2, CHUNK)?,
                Some(&idx),
            )?;
            let nin = nin.min(avail);
            self.owed += nin as f64 * rs.resample_ratio();
            (self.off, self.done) = (self.off + nin * 2, self.done + nin as u64);
            let drop = self.lead.min(nout);
            let keep = match partial {
                true => (nout - drop).min(self.owed.round().max(0.0) as usize),
                false => nout - drop,
            };
            (self.lead, self.owed) = (self.lead - drop, self.owed - keep as f64);
            self.out.truncate((drop + keep) * 2);
            self.out.drain(..drop * 2);
        }
        Ok(k / 2)
    }
}

#[derive(Default)]
struct Mixer {
    rate: u32,
    cur: Option<Source>,
    inc: Option<Source>,
    old: Option<Source>,
    fade: (usize, usize),
    pre: Option<(Track, JoinHandle<Result<Source, String>>)>,
    b: Vec<f32>,
}

impl Mixer {
    fn queue(&mut self, io: &Io, t: Option<Track>) {
        let (io, r) = (io.clone(), self.rate);
        self.pre = t.map(|t| {
            (
                t.clone(),
                thread::spawn(move || Source::open(t, &io, r, 0.0)),
            )
        });
    }

    fn seek(&mut self, io: &Io, secs: f32) -> Result<(), String> {
        self.cur = match self.inc.take().or(self.cur.take()) {
            Some(c) if c.track.transcoded => Some(Source::open(c.track, io, self.rate, secs)?),
            Some(mut c) => Some(c.seek(secs).map(|()| c).map_err(|e| e.to_string())?),
            None => None,
        };
        Ok(())
    }

    fn step(&mut self, out: &mut [f32], speed: f32, cf: f32) -> Res<(usize, Option<Event>)> {
        let rel = 1.0 / speed as f64;
        for s in self.cur.iter_mut().chain(self.inc.iter_mut()) {
            s.tune(self.rate, rel)?;
        }
        let Some(cur) = &mut self.cur else {
            return Ok((0, None));
        };
        let rem = cur.dur - cur.pos() as f64;
        let ready = self.pre.as_ref().is_some_and(|p| p.1.is_finished());
        let mut ev = None;
        if cf > 0.0 && self.inc.is_none() && rem <= (cf as f64).min(cur.dur / 2.0) && ready {
            match self.pre.take().map(|p| p.1.join().expect("open thread")) {
                Some(Ok(mut s)) => {
                    s.tune(self.rate, rel)?;
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
            self.b.resize(out.len(), 0.0);
            let m = inc.fill(&mut self.b)?;
            self.b[m * 2..].fill(0.0);
            let (pos, len) = self.fade;
            let frames = out
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(self.b.as_chunks::<2>().0);
            for (i, (o, b)) in frames.enumerate() {
                let (s, c) = (((pos + i) as f32 / len as f32).min(1.0) * FRAC_PI_2).sin_cos();
                *o = [o[0] * c + b[0] * s, o[1] * c + b[1] * s];
            }
            self.fade.0 += out.len() / 2;
            if self.fade.0 >= len || n < out.len() / 2 {
                self.cur = self.inc.take();
            }
            return Ok((n.max(m), ev));
        }
        if n == out.len() / 2 {
            return Ok((n, ev));
        }
        self.cur = None;
        match self.pre.take().map(|p| p.1.join().expect("open thread")) {
            Some(Ok(mut s)) => {
                s.tune(self.rate, rel)?;
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
    at: f32,
    cf: f32,
    speed: f32,
) -> Res<Vec<f32>> {
    let (mut mix, mut it) = (
        Mixer {
            rate,
            ..Default::default()
        },
        tracks.into_iter(),
    );
    mix.cur = Some(Source::open(it.next().ok_or("no tracks")?, &io, rate, at)?);
    mix.queue(&io, it.next());
    let (mut out, mut buf) = (Vec::new(), vec![0.0; CHUNK * 2]);
    while mix.cur.is_some() {
        while mix.pre.as_ref().is_some_and(|p| !p.1.is_finished()) {
            thread::sleep(Duration::from_millis(1));
        }
        let (n, ev) = mix.step(&mut buf, speed, cf)?;
        match ev {
            Some(Event::Error(e)) => Err(e)?,
            Some(Event::Started(_)) => mix.queue(&io, it.next()),
            _ => {}
        }
        out.extend_from_slice(&buf[..n * 2]);
    }
    Ok(out)
}

fn open(
    id: &str,
    want: u32,
    sh: &Arc<Shared>,
    tx: &UnboundedSender<Event>,
) -> Result<(cpal::Stream, Producer<f32>, u32), String> {
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
        .find(|c| c.channels() == 2 && (c.min_sample_rate()..=c.max_sample_rate()).contains(&want))
        .map(|c| c.with_sample_rate(want))
        .map_or_else(|| dev.default_output_config(), Ok)
        .map_err(|e| e.to_string())?;
    let rate = sup.sample_rate();
    let buffer_size = match *sup.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed((rate / 20).clamp(min, max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    };
    let (prod, mut cons) = RingBuffer::<f32>::new(rate as usize);
    let (sh, etx, me, half) = (sh.clone(), tx.clone(), thread::current(), rate as usize / 2);
    let (mut read, mut vol, ramp) = (0u64, 0f32, 1000.0 / rate as f32);
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: rate,
        buffer_size,
    };
    let stream = dev
        .build_output_stream(
            config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let stale = sh.flush.load(Relaxed).saturating_sub(read) as usize;
                if stale > 0
                    && let Ok(c) = cons.read_chunk(stale.min(cons.slots()))
                {
                    read += c.len() as u64;
                    c.commit_all();
                }
                let n = cons.pop_partial_slice(out).0.len();
                out[n..].fill(0.0);
                let target = get(&sh.volume).powi(3);
                for f in out.as_chunks_mut::<2>().0 {
                    vol += (target - vol) * ramp;
                    *f = [f[0] * vol, f[1] * vol];
                }
                read += n as u64;
                sh.read.store(read, Relaxed);
                if cons.slots() < half {
                    me.unpark();
                }
            },
            move |e| drop(etx.send(Event::Error(e.to_string()))),
            None,
        )
        .map_err(|e| e.to_string())?;
    Ok((stream, prod, rate))
}

fn engine(rx: Receiver<Cmd>, tx: UnboundedSender<Event>, sh: Arc<Shared>, io: Io) {
    let mut mix = Mixer::default();
    let mut out: Option<(cpal::Stream, Producer<f32>)> = None;
    let (mut written, mut running, mut idle, mut dev) =
        (0u64, false, None::<Instant>, String::new());
    let mut buf = vec![0f32; CHUNK * 2];
    let mut marks = VecDeque::<(u64, Option<f32>, Option<Event>)>::new();
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
            }
            match cmd {
                Cmd::Play(t, at) => {
                    let pre = mix.pre.take_if(|p| at == 0.0 && p.0 == t).map(|p| p.1);
                    let mut was = [mix.cur.take(), mix.old.take()];
                    let hit = was
                        .iter_mut()
                        .find_map(|s| s.take_if(|s| !t.transcoded && s.track.url == t.url));
                    mix.old = was.into_iter().flatten().next();
                    (mix.inc, play) = (None, Some((t, at, pre, hit)));
                }
                Cmd::Next(t) if mix.pre.as_ref().is_some_and(|p| Some(&p.0) == t.as_ref()) => {}
                Cmd::Next(t) => mix.queue(&io, t),
                Cmd::Gain(g) => mix.cur.iter_mut().for_each(|c| c.track.gain = g),
                Cmd::Device(d) => (dev, out) = (d, None),
                Cmd::Seek(s) => {
                    if let Err(e) = mix.seek(&io, s) {
                        let _ = tx.send(Event::Error(e));
                    }
                }
                Cmd::Stop => {
                    (mix.cur, mix.inc) = (None, None);
                    sh.pos.store(0, Relaxed);
                }
            }
        }
        let (paused, fresh) = (sh.paused.load(Relaxed), play.is_some());
        if let Some((t, at, pre, hit)) = play {
            let s = match (hit, pre) {
                (Some(mut s), _) => s
                    .seek(at)
                    .map(|()| (s.track = t, s).1)
                    .map_err(|e| e.to_string()),
                (None, Some(h)) => h.join().expect("open thread"),
                (None, None) => Source::open(t, &io, mix.rate, at),
            };
            match s {
                Ok(s) => mix.cur = Some(s),
                Err(e) => drop(tx.send(Event::Error(e))),
            }
            if mix.cur.as_ref().is_some_and(|c| c.rate != mix.rate as f64) {
                out = None;
            }
        }
        if out.is_none()
            && !paused
            && let Some(c) = &mix.cur
        {
            let pos = get(&sh.pos);
            let r = open(&dev, c.rate as u32, &sh, &tx).and_then(|(s, p, rate)| {
                mix.rate = rate;
                if !fresh {
                    mix.seek(&io, pos)?;
                }
                Ok((s, p))
            });
            match r {
                Ok(o) => {
                    (written, running, idle, out) = (0, false, None, Some(o));
                    sh.read.store(0, Relaxed);
                    sh.flush.store(0, Relaxed);
                    marks.clear();
                }
                Err(e) => {
                    (mix.cur, mix.inc) = (None, None);
                    let _ = tx.send(Event::Error(e));
                }
            }
        }
        if let Some((_, prod)) = &mut out {
            while !paused && mix.cur.is_some() && prod.slots() >= buf.len() {
                let (speed, cf) = (get(&sh.speed), get(&sh.crossfade));
                let (n, ev) = mix.step(&mut buf, speed, cf).unwrap_or_else(|e| {
                    (mix.cur, mix.inc) = (None, None);
                    (0, Some(Event::Error(e.to_string())))
                });
                prod.push_entire_slice(&buf[..n * 2])
                    .expect("ring buffer has room");
                written += n as u64 * 2;
                marks.push_back((
                    written,
                    mix.inc.as_ref().or(mix.cur.as_ref()).map(Source::pos),
                    ev,
                ));
            }
        }
        let read = sh.read.load(Relaxed);
        while marks.front().is_some_and(|m| m.0 <= read) {
            let (_, p, e) = marks.pop_front().expect("a mark");
            if let Some(p) = p {
                sh.pos.store(p.to_bits(), Relaxed);
            }
            if let Some(e) = e {
                let _ = tx.send(e);
            }
        }
        let want = !paused && (mix.cur.is_some() || !marks.is_empty());
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
