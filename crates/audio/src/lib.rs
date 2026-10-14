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
    Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};
use rusty_opus::OpusDecoder;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Time, TimeBase};
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
    crossfade: AtomicU32,
    pos: AtomicU32,
    paused: AtomicBool,
    broken: AtomicBool,
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
                            if st.end() < len && st.end() < st.want + AHEAD / 2 {
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
                if !tail && end - pos < AHEAD / 2 && st.len.is_some_and(|l| end < l) {
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
            self.net.2.notify_one();
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

type Decode = Box<dyn FnMut(Option<&Packet>, &mut Vec<f32>) -> Result<(), SymError> + Send>;

struct Source {
    track: Track,
    demux: Box<dyn FormatReader>,
    tid: u32,
    net: Arc<Net>,
    dec: Decode,
    rs: Option<Async<f32>>,
    orate: u32,
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
            let mut task = JoinSet::new();
            task.spawn_on(fetch(net.clone(), http.clone(), url, Some(head)), rt);
            let (n, pos, _task) = (net.clone(), 0, task);
            let src = Box::new(Http { net: n, pos, _task });
            let mss = MediaSourceStream::new(src, Default::default());
            let probe = symphonia::default::get_probe();
            let demux = probe.probe(&Hint::new(), mss, Default::default(), Default::default())?;
            let t = demux.default_track(TrackType::Audio);
            let a = t.and_then(|t| Some((t, t.codec_params.as_ref()?.audio()?)));
            let (t, p) = a.ok_or("The stream has no audio.")?;
            let (tid, tb, frames, p) = (t.id, t.time_base, t.num_frames, p.clone());
            let head: Vec<u8> = p.extra_data.clone().unwrap_or_default().into();
            let sr = p.sample_rate.ok_or("unknown sample rate")?;
            let params = Some(p).filter(|p| p.codec != CODEC_ID_OPUS);
            let tb = tb.or(TimeBase::try_from_recip(sr)).ok_or("no time base")?;
            let base = [0.0, offset.trunc() as f64][usize::from(track.transcoded)];
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
                tid,
                net,
                dec,
                rs: None,
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

    fn seek(&mut self, secs: f32) -> Res<()> {
        let target = secs as f64 + self.delay;
        let preroll = if self.delay > 0.0 { 0.08 } else { 0.0 };
        let time = Time::try_from_secs_f64((target - preroll).max(0.0)).ok_or("invalid seek")?;
        let st = self.net.st();
        let len = st.len.unwrap_or(0);
        let need = len.min((len as f64 * secs as f64 / self.dur) as u64 + (256 << 10));
        let have = usize::from(st.start == 0 && st.end() >= need);
        drop(st);
        let mode = [SeekMode::Coarse, SeekMode::Accurate][have];
        let ts = self.tb.calc_timestamp(time).ok_or("invalid seek")?;
        let track_id = self.tid;
        let to = self
            .demux
            .seek(mode, SeekTo::Timestamp { ts, track_id })?
            .actual_ts;
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

    fn fill(&mut self, out: &mut [f32], rate: u32) -> Res<usize> {
        if rate != self.orate {
            (self.orate, self.rs) = (rate, None);
        }
        if self.orate as f64 == self.rate {
            self.rs = None;
        } else if self.rs.is_none() {
            let p = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
                .interpolation(SincInterpolationType::Linear);
            let ratio = self.orate as f64 / self.rate;
            let rs = Async::new_sinc(ratio, 1.0, &p, CHUNK, 2, FixedAsync::Output)?;
            (self.lead, self.owed) = (rs.output_delay(), 0.0);
            self.rs = Some(rs);
        }
        let (gain, mut k) = (self.track.gain, 0);
        while k < out.len() {
            if self.out.is_empty() {
                let need = self.rs.as_ref().map_or(CHUNK, |r| r.input_frames_next()) * 2;
                if self.pcm.len() - self.off < need {
                    self.pcm.drain(..self.off);
                    self.off = 0;
                }
                while !self.eof && self.pcm.len() < need {
                    let p = match self.demux.next_packet() {
                        Err(SymError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => None,
                        r => r?,
                    };
                    let Some(p) = p else {
                        self.eof = true;
                        break;
                    };
                    if p.track_id != self.tid {
                        continue;
                    }
                    let n = self.pcm.len();
                    match (self.dec)(Some(&p), &mut self.pcm) {
                        Err(SymError::DecodeError(_)) => {}
                        r => r?,
                    }
                    let k = (self.skip * 2).min(self.pcm.len() - n);
                    self.pcm.drain(n..n + k);
                    self.skip -= k / 2;
                }
                if let Some(rs) = &mut self.rs {
                    let avail = (self.pcm.len() - self.off) / 2;
                    if self.eof && avail == 0 && self.owed < 1.0 {
                        break;
                    }
                    let partial = avail < need / 2;
                    let idx = partial.then(|| Indexing::new().partial_len(avail));
                    self.out.resize(CHUNK * 2, 0.0);
                    let (nin, nout) = rs.process_into_buffer(
                        &InterleavedSlice::new(&self.pcm[self.off..], 2, avail)?,
                        &mut InterleavedSlice::new_mut(&mut self.out, 2, CHUNK)?,
                        idx.as_ref(),
                    )?;
                    let nin = nin.min(avail);
                    self.owed += nin as f64 * rs.resample_ratio();
                    (self.off, self.done) = (self.off + nin * 2, self.done + nin as u64);
                    let (drop, cap) = (self.lead.min(nout), self.owed.round().max(0.0) as usize);
                    let keep = (nout - drop).min(if partial { cap } else { nout });
                    (self.lead, self.owed) = (self.lead - drop, self.owed - keep as f64);
                    self.out.truncate((drop + keep) * 2);
                    self.out.drain(..drop * 2);
                    continue;
                }
            }
            let src = if self.out.is_empty() {
                &self.pcm[self.off..]
            } else {
                &self.out
            };
            let m = src.len().min(out.len() - k);
            if m == 0 {
                break;
            }
            for (o, s) in out[k..k + m].iter_mut().zip(src) {
                *o = s * gain;
            }
            k += m;
            if self.out.is_empty() {
                (self.off, self.done) = (self.off + m, self.done + m as u64 / 2);
            } else {
                self.out.drain(..m);
            }
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
        let open = move |t: Track| thread::spawn(move || Source::open(t, &io, r, 0.0));
        self.pre = t.map(|t| (t.clone(), open(t)));
    }

    fn seek(&mut self, io: &Io, secs: f32) -> Result<(), String> {
        self.cur = match self.inc.take().or(self.cur.take()) {
            Some(c) if c.track.transcoded => Some(Source::open(c.track, io, self.rate, secs)?),
            Some(mut c) => Some(c.seek(secs).map(|()| c).map_err(|e| e.to_string())?),
            None => None,
        };
        Ok(())
    }

    fn step(&mut self, out: &mut [f32], cf: f32) -> Res<(usize, Option<Event>)> {
        let rate = self.rate;
        let Some(cur) = &mut self.cur else {
            return Ok((0, None));
        };
        let rem = cur.dur - cur.pos() as f64;
        let ready = self.pre.as_ref().is_some_and(|p| p.1.is_finished());
        let mut ev = None;
        if cf > 0.0 && self.inc.is_none() && rem <= (cf as f64).min(cur.dur / 2.0) && ready {
            match self.pre.take().map(|p| p.1.join().expect("open thread")) {
                Some(Ok(s)) => {
                    ev = Some(Event::Started(s.track.id.clone()));
                    self.fade = (0, (rem.max(0.05) * rate as f64) as usize);
                    self.inc = Some(s);
                }
                e => ev = e.and_then(Result::err).map(Event::Error),
            }
        }
        let n = cur.fill(out, rate)?;
        out[n * 2..].fill(0.0);
        if let Some(inc) = &mut self.inc {
            self.b.resize(out.len(), 0.0);
            let m = inc.fill(&mut self.b, rate)?;
            self.b[m * 2..].fill(0.0);
            let (pos, len) = self.fade;
            for (i, (o, b)) in out.chunks_mut(2).zip(self.b.chunks(2)).enumerate() {
                let (s, c) = (((pos + i) as f32 / len as f32).min(1.0) * FRAC_PI_2).sin_cos();
                o.iter_mut().zip(b).for_each(|(o, b)| *o = *o * c + b * s);
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
                let m = s.fill(&mut out[n * 2..], rate)?;
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
pub fn render(io: Io, tracks: Vec<Track>, rate: u32, at: f32, cf: f32) -> Res<Vec<f32>> {
    let (mut mix, mut it) = (Mixer::default(), tracks.into_iter());
    mix.rate = rate;
    mix.cur = Some(Source::open(it.next().ok_or("no tracks")?, &io, rate, at)?);
    mix.queue(&io, it.next());
    let (mut out, mut buf) = (Vec::new(), vec![0.0; CHUNK * 2]);
    while mix.cur.is_some() {
        while mix.pre.as_ref().is_some_and(|p| !p.1.is_finished()) {
            thread::sleep(Duration::from_millis(1));
        }
        let (n, ev) = mix.step(&mut buf, cf)?;
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
) -> Res<(cpal::Stream, Producer<f32>, u32)> {
    let host = cpal::default_host();
    let dev = match id {
        "" => host.default_output_device(),
        id => host.device_by_id(&id.parse()?),
    }
    .ok_or("The audio output device is not available. Select a device in Settings.")?;
    let sup = dev
        .supported_output_configs()
        .into_iter()
        .flatten()
        .find(|c| c.channels() == 2 && (c.min_sample_rate()..=c.max_sample_rate()).contains(&want))
        .map(|c| c.with_sample_rate(want))
        .map_or_else(|| dev.default_output_config(), Ok)?;
    let rate = sup.sample_rate();
    let buffer_size = match *sup.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed((rate / 20).clamp(min, max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    };
    let (prod, mut cons) = RingBuffer::<f32>::new(rate as usize);
    let (sh, etx, me, half) = (sh.clone(), tx.clone(), thread::current(), rate as usize / 2);
    let (sh2, me2) = (sh.clone(), me.clone());
    let (mut read, mut vol, ramp) = (0u64, 0f32, 1000.0 / rate as f32);
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: rate,
        buffer_size,
    };
    let stream = dev.build_output_stream(
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
        move |e| {
            if !sh2.broken.swap(true, Relaxed) && !sh2.paused.load(Relaxed) {
                let _ = etx.send(Event::Error(e.to_string()));
            }
            me2.unpark();
        },
        None,
    )?;
    Ok((stream, prod, rate))
}

fn engine(rx: Receiver<Cmd>, tx: UnboundedSender<Event>, sh: Arc<Shared>, io: Io) {
    let mut mix = Mixer::default();
    let mut out: Option<(cpal::Stream, Producer<f32>)> = None;
    let (mut written, mut running, mut idle, mut dev) =
        (0u64, false, None::<Instant>, String::new());
    let mut buf = vec![0f32; CHUNK * 2];
    let mut marks = VecDeque::<(u64, Option<f32>, Option<Event>)>::new();
    let fail = |e: String| drop(tx.send(Event::Error(e)));
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
                    let same = |s: &mut Source| !t.transcoded && s.track.url == t.url;
                    let hit = was.iter_mut().find_map(|s| s.take_if(|s| same(s)));
                    mix.old = was.into_iter().flatten().next();
                    (mix.inc, play) = (None, Some((t, at, pre, hit)));
                }
                Cmd::Next(t) if mix.pre.as_ref().is_some_and(|p| Some(&p.0) == t.as_ref()) => {}
                Cmd::Next(t) => mix.queue(&io, t),
                Cmd::Gain(g) => mix
                    .inc
                    .iter_mut()
                    .chain(&mut mix.cur)
                    .take(1)
                    .for_each(|c| c.track.gain = g),
                Cmd::Device(d) => (dev, out) = (d, None),
                Cmd::Seek(s) => mix.seek(&io, s).unwrap_or_else(fail),
                Cmd::Stop => {
                    (mix.cur, mix.inc) = (None, None);
                    sh.pos.store(0, Relaxed);
                }
            }
        }
        let (paused, fresh) = (sh.paused.load(Relaxed), play.is_some());
        if sh.broken.load(Relaxed) {
            out = None;
            sh.broken.store(false, Relaxed);
            if !paused {
                (mix.cur, mix.inc) = (None, None);
            }
        }
        if let Some((t, at, pre, hit)) = play {
            let s = match (hit, pre) {
                (Some(mut s), _) => {
                    s.track = t;
                    s.seek(at).map(|()| s).map_err(|e| e.to_string())
                }
                (None, Some(h)) => h.join().expect("open thread"),
                (None, None) => Source::open(t, &io, mix.rate, at),
            };
            mix.cur = s.map_err(fail).ok();
            if mix.cur.as_ref().is_some_and(|c| c.rate != mix.rate as f64) {
                out = None;
            }
        }
        if out.is_none()
            && !paused
            && let Some(c) = &mix.cur
        {
            match open(&dev, c.rate as u32, &sh, &tx) {
                Ok((s, p, rate)) => {
                    (written, running, idle, out, mix.rate) = (0, false, None, Some((s, p)), rate);
                    sh.read.store(0, Relaxed);
                    sh.flush.store(0, Relaxed);
                    marks.clear();
                    if !fresh {
                        mix.seek(&io, get(&sh.pos)).unwrap_or_else(fail);
                    }
                }
                Err(e) => {
                    (mix.cur, mix.inc) = (None, None);
                    fail(e.to_string());
                }
            }
        }
        if let Some((_, prod)) = &mut out {
            while !paused && mix.cur.is_some() && prod.slots() >= buf.len() {
                let (n, ev) = mix.step(&mut buf, get(&sh.crossfade)).unwrap_or_else(|e| {
                    (mix.cur, mix.inc) = (None, None);
                    (0, Some(Event::Error(e.to_string())))
                });
                prod.push_entire_slice(&buf[..n * 2])
                    .expect("ring buffer has room");
                written += n as u64 * 2;
                let pos = mix.inc.as_ref().or(mix.cur.as_ref()).map(Source::pos);
                marks.push_back((written, pos, ev));
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
            match want {
                true => s.play().map_err(|e| e.to_string()).unwrap_or_else(fail),
                false if s.pause().is_err() => out = None,
                false => {}
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
