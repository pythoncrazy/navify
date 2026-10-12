use std::collections::VecDeque;
use std::error::Error;
use std::f32::consts::FRAC_PI_2;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use opus_decoder::OpusDecoder;
use rtrb::{Producer, RingBuffer};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Adjustable, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use stream_download::http::reqwest::Url;
use stream_download::{Settings, StreamDownload, storage::memory::MemoryStorageProvider};
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Time, TimeBase};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

const CHUNK: usize = 512;
const CLOSE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default)]
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
    pub fn new(rt: Handle) -> (Player, UnboundedReceiver<Event>) {
        let (tx, rx) = channel();
        let (etx, erx) = unbounded_channel();
        let sh = Arc::new(Shared::default());
        sh.volume.store(1f32.to_bits(), Relaxed);
        sh.speed.store(1f32.to_bits(), Relaxed);
        let esh = sh.clone();
        let th = thread::spawn(move || engine(rx, etx, esh, rt)).thread().clone();
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

struct Http(StreamDownload<MemoryStorageProvider>, bool);

impl Read for Http {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for Http {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl MediaSource for Http {
    fn is_seekable(&self) -> bool {
        self.1
    }

    fn byte_len(&self) -> Option<u64> {
        self.0.content_length()
    }
}

type Decode = Box<dyn FnMut(Option<&Packet>, &mut Vec<f32>) -> Result<(), SymError> + Send>;

struct Source {
    track: Track,
    format: Box<dyn FormatReader>,
    dec: Decode,
    rs: Async<f32>,
    id: u32,
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
    fn open(track: Track, rt: &Handle, rate: u32, offset: f32) -> Result<Source, String> {
        let url = match track.transcoded && offset >= 1.0 {
            true => format!("{}&timeOffset={}", track.url, offset as u32),
            false => track.url.clone(),
        };
        let url: Url = url.parse().map_err(|_| "invalid stream url".to_string())?;
        let safe = url.as_str().to_string();
        (|| -> Result<Source, Box<dyn Error>> {
            let settings = Settings::default().prefetch_bytes(64 * 1024);
            let reader =
                rt.block_on(StreamDownload::new_http(url, MemoryStorageProvider, settings))?;
            let mss = MediaSourceStream::new(
                Box::new(Http(reader, !track.transcoded)),
                Default::default(),
            );
            let format = symphonia::default::get_probe().probe(
                &Hint::new(),
                mss,
                Default::default(),
                Default::default(),
            )?;
            let t = format.default_track(TrackType::Audio).ok_or("no audio track")?;
            let p =
                t.codec_params.as_ref().and_then(|p| p.audio()).ok_or("no audio codec")?.clone();
            let sr = p.sample_rate.ok_or("unknown sample rate")?;
            let (id, tb) =
                (t.id, t.time_base.or(TimeBase::try_from_recip(sr)).ok_or("no time base")?);
            let base = if track.transcoded { offset.trunc() as f64 } else { 0.0 };
            let dur = t.num_frames.map_or(track.duration as f64, |n| base + n as f64 / sr as f64);
            let head = p.extra_data.clone().unwrap_or_default();
            let le = |i: usize| head.get(i..i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
            let (opus, pre) = (p.codec == CODEC_ID_OPUS, le(10));
            let dec: Decode = if opus {
                if head.get(9).is_some_and(|&c| c > 2) {
                    Err("multichannel opus unsupported")?
                }
                let gain = 10f32.powf(le(16) as i16 as f32 / 5120.0);
                let (mut d, mut buf) = (OpusDecoder::new(48_000, 2)?, vec![0f32; 5760 * 2]);
                Box::new(move |p, out| {
                    let Some(p) = p else {
                        d.reset();
                        return Ok(());
                    };
                    let n = d
                        .decode_float(&p.data, &mut buf, false)
                        .map_err(|_| SymError::DecodeError("opus"))?;
                    let e = n.saturating_sub(p.trim_end.get() as usize);
                    out.extend(
                        buf[(p.trim_start.get() as usize).min(e) * 2..e * 2]
                            .iter()
                            .map(|s| s * gain),
                    );
                    Ok(())
                })
            } else {
                let mut d =
                    symphonia::default::get_codecs().make_audio_decoder(&p, &Default::default())?;
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
            };
            let params = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
                .interpolation(SincInterpolationType::Linear);
            let rs = Async::<f32>::new_sinc(
                rate as f64 / sr as f64,
                3.5,
                &params,
                CHUNK,
                2,
                FixedAsync::Output,
            )?;
            let mut s = Source {
                lead: rs.output_delay(),
                track,
                format,
                dec,
                rs,
                id,
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

    fn seek(&mut self, secs: f32) -> Result<(), Box<dyn Error>> {
        let target = secs as f64 + self.delay;
        let preroll = if self.delay > 0.0 { 0.08 } else { 0.0 };
        let time = Time::try_from_secs_f64((target - preroll).max(0.0)).ok_or("invalid seek")?;
        let to =
            self.format.seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(self.id) })?;
        self.skip = ((target - self.tb.calc_time_saturating(to.actual_ts).as_secs_f64())
            * self.rate)
            .max(0.0) as usize;
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
        while self.out.len() < out.len() && !(self.eof && self.pcm.is_empty() && self.owed < 1.0) {
            let need = self.rs.input_frames_next();
            while !self.eof && self.pcm.len() < need * 2 {
                match self.format.next_packet() {
                    Ok(Some(p)) if p.track_id == self.id => {
                        let n = self.pcm.len();
                        match (self.dec)(Some(&p), &mut self.pcm) {
                            Err(SymError::DecodeError(_)) => {}
                            r => r?,
                        }
                        let k = (self.skip * 2).min(self.pcm.len() - n);
                        self.pcm.drain(n..n + k);
                        self.skip -= k / 2;
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => self.eof = true,
                    Err(SymError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                        self.eof = true
                    }
                    Err(e) => Err(e)?,
                }
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
    rt: Handle,
    rate: u32,
    cur: Option<Source>,
    inc: Option<Source>,
    fade: (usize, usize),
    next: Option<Track>,
    pre: Option<(Track, JoinHandle<Result<Source, String>>)>,
    b: Vec<f32>,
}

impl Mixer {
    fn new(rt: Handle, rate: u32) -> Mixer {
        Mixer {
            rt,
            rate,
            cur: None,
            inc: None,
            fade: (0, 1),
            next: None,
            pre: None,
            b: vec![0.0; CHUNK * 2],
        }
    }

    fn reopen(&mut self, rate: u32, pos: f32) -> Result<(), String> {
        self.rate = rate;
        self.next = self.pre.take().map(|p| p.0).or(self.next.take());
        self.cur = self
            .inc
            .take()
            .or(self.cur.take())
            .map(|c| Source::open(c.track, &self.rt, rate, pos))
            .transpose()?;
        Ok(())
    }

    fn seek(&mut self, secs: f32) -> Result<(), String> {
        if let Some(i) = self.inc.take() {
            self.cur = Some(i);
        }
        match self.cur.take() {
            Some(c) if c.track.transcoded => {
                self.cur = Some(Source::open(c.track, &self.rt, self.rate, secs)?)
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
        self.cur
            .iter_mut()
            .chain(self.inc.iter_mut())
            .try_for_each(|s| s.rs.set_resample_ratio_relative(rel, true))?;
        let Some(cur) = &mut self.cur else { return Ok((0, None)) };
        let rem = cur.dur - cur.pos() as f64;
        let cf = (cf as f64).min(cur.dur / 2.0);
        if self.pre.is_none()
            && rem < 15.0 + cf
            && let Some(t) = self.next.take()
        {
            let (h, r) = (self.rt.clone(), self.rate);
            self.pre = Some((t.clone(), thread::spawn(move || Source::open(t, &h, r, 0.0))));
        }
        let mut ev = None;
        if cf > 0.0
            && self.inc.is_none()
            && rem <= cf
            && self.pre.as_ref().is_some_and(|p| p.1.is_finished())
        {
            match self.pre.take().map(|p| p.1.join().expect("open thread")) {
                Some(Ok(mut s)) => {
                    s.rs.set_resample_ratio_relative(rel, false)?;
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
            self.fade.0 += CHUNK;
            if self.fade.0 >= len || n < CHUNK {
                self.cur = self.inc.take();
            }
            return Ok((n.max(m), ev));
        }
        if n == out.len() / 2 {
            return Ok((n, ev));
        }
        let next = self
            .pre
            .take()
            .map(|p| p.1.join().expect("open thread"))
            .or_else(|| self.next.take().map(|t| Source::open(t, &self.rt, self.rate, 0.0)));
        self.cur = None;
        match next {
            Some(Ok(mut s)) => {
                s.rs.set_resample_ratio_relative(rel, false)?;
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
    rt: &Handle,
    tracks: Vec<Track>,
    rate: u32,
    seek: f32,
    crossfade: f32,
    speed: f32,
) -> Result<Vec<f32>, String> {
    let mut mix = Mixer::new(rt.clone(), rate);
    let mut it = tracks.into_iter();
    mix.cur = Some(Source::open(it.next().ok_or("no tracks")?, &mix.rt, rate, seek)?);
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

fn engine(rx: Receiver<Cmd>, tx: UnboundedSender<Event>, sh: Arc<Shared>, rt: Handle) {
    let me = thread::current();
    let open = |id: &str| -> Result<(cpal::Stream, Producer<f32>, u32), String> {
        let host = cpal::default_host();
        let dev = match id {
            "" => host.default_output_device(),
            id => host.device_by_id(&id.parse().map_err(|e: cpal::Error| e.to_string())?),
        }
        .ok_or("The audio output device is not available. Select a device in Settings.")?;
        let rate = dev.default_output_config().map_err(|e| e.to_string())?.sample_rate();
        let (prod, mut cons) = RingBuffer::<f32>::new(rate as usize / 2);
        let (csh, etx, me, half) = (sh.clone(), tx.clone(), me.clone(), rate as usize / 4);
        let (mut read, mut vol) = (0u64, 0f32);
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: rate,
            buffer_size: cpal::BufferSize::Default,
        };
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
                        vol += (target - vol) * 0.002;
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
    let mut mix = Mixer::new(rt, 0);
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
                Cmd::Play(t, at) => (mix.cur, mix.inc, play) = (None, None, Some((t, at))),
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
        if out.is_none() && !paused && (play.is_some() || mix.cur.is_some()) {
            let pos = f32::from_bits(sh.pos.load(Relaxed));
            match open(&dev).and_then(|(s, p, rate)| mix.reopen(rate, pos).map(|_| (s, p))) {
                Ok(o) => {
                    (written, running, idle) = (0, false, None);
                    sh.read.store(0, Relaxed);
                    sh.flush.store(0, Relaxed);
                    marks.clear();
                    events.clear();
                    out = Some(o);
                }
                Err(e) => {
                    (mix.cur, mix.inc, play) = (None, None, None);
                    let _ = tx.send(Event::Error(e));
                }
            }
        }
        if let Some((t, at)) = play {
            match Source::open(t, &mix.rt, mix.rate, at) {
                Ok(s) => mix.cur = Some(s),
                Err(e) => drop(tx.send(Event::Error(e))),
            }
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
