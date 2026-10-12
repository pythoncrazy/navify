use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use iced::{Color, Task, widget::image};

use crate::api::{Album, Artist, Info, Playlist, Session, Song};
use crate::config::{self, Config};

pub const PAGE: usize = 100;
const STALE: Duration = Duration::from_secs(300);
const COVER_CAP: usize = 64 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Pager<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
}

#[derive(Debug)]
pub struct Home {
    pub recent: Vec<Album>,
    pub added: Vec<Album>,
    pub frequent: Vec<Album>,
    pub random: Vec<Album>,
    pub playlists: Vec<Playlist>,
    pub starred: (Vec<Artist>, Vec<Album>, Vec<Song>),
}

#[derive(Debug)]
pub struct Lib {
    pub playlists: Vec<Playlist>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
}

#[derive(Debug)]
pub struct SearchRes {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

pub type AnyData = Arc<dyn Any + Send + Sync>;

#[derive(Clone, Debug)]
pub enum MenuKind {
    Song(Song),
    SongIn(Song, usize),
    Album(Album),
    Artist(Artist),
    Playlist(Playlist),
    Speed,
    User,
}

#[derive(Clone, Debug)]
pub struct Menu {
    pub x: f32,
    pub y: f32,
    pub kind: MenuKind,
    pub sub: u8,
}

#[derive(Clone, Debug)]
pub enum Set {
    Rg(String),
    Preamp(f32),
    Xfade(f32),
    Trans(u8),
    Bitrate(u32),
    Qsync(bool),
    Discord(bool),
    Speed(f32),
}

#[derive(Clone, Debug)]
pub enum Message {
    Loaded(String, Result<AnyData, String>),
    Cover(String, u32, Option<(u32, u32, Color, Vec<u8>)>),
    Nav(String),
    Back,
    Fwd,
    Scrolled(f32, f32, f32),
    Search(String),
    SearchGo(u64),
    Play(Vec<Song>, usize, String),
    PlayQueue(usize),
    Toggle,
    Prev,
    Next(bool),
    Ended,
    Seek(f32),
    Shuffle,
    Repeat,
    Vol(f32),
    Mute,
    Star(String, bool),
    Rate(String, u8),
    QNext(Vec<Song>),
    QEnd(Vec<Song>),
    QDel(usize),
    QClear,
    Menu(Box<MenuKind>),
    MenuSub(u8),
    MenuClose,
    PlAdd(String, Vec<String>),
    PlNew(String),
    PlDel(String),
    PlRemove(usize),
    Radio(String),
    Toast(u64, String),
    Panel(u8),
    LibFilter(u8),
    LibCollapse,
    NewPlaylist,
    LField(u8, String),
    LRemember(bool),
    LSubmit,
    LoggedIn(Result<Session, String>),
    LogOut,
    UseServer(String),
    DropServer(String),
    Set(Set),
    ClearCache,
    Focus(bool),
    Cursor(f32, f32),
    Close,
    Noop,
    Quit,
    Shot(iced::window::Screenshot),
    ShotTake,
    SaveQ(u64),
    Tick,
    Rename(String, String),
    Modal(String, bool),
    RestoreQueue(Vec<Song>, String, u64),
    NativeFailed,
    PlayCtx(String, bool),
    PlayCtxDone(Vec<Song>, String, bool),
    AlbumQueue(String),
    Select(String),
    Popular,
    FocusSearch,
    VolUp,
    VolDown,
    Resize(f32, f32),
    SeekPreview(f32),
    SeekRelease,
    #[cfg(feature = "audio")]
    Audio(navify_audio::Event),
}

#[derive(Debug, Default)]
pub struct Covers {
    pub map: HashMap<(String, u32), (u64, image::Handle, usize)>,
    order: VecDeque<((String, u32), u64)>,
    pub bytes: usize,
    tick: u64,
    pub color: HashMap<String, Color>,
    pub want: HashSet<(String, u32)>,
}

impl Covers {
    pub fn get(&self, k: &(String, u32)) -> Option<image::Handle> {
        self.map.get(k).map(|(_, h, _)| h.clone())
    }

    pub fn insert(&mut self, k: (String, u32), w: u32, h: u32, c: Color, px: Vec<u8>) {
        self.tick += 1;
        let size = (w * h * 4) as usize;
        if k.1 == 640 {
            self.color.insert(k.0.clone(), c);
        }
        let tk = self.tick;
        self.map.insert(k.clone(), (tk, image::Handle::from_rgba(w, h, px), size));
        self.bytes += size;
        self.order.push_back((k.clone(), tk));
        while self.bytes > COVER_CAP {
            let Some((oldest, ot)) = self.order.pop_front() else { break };
            if let Some((t, _, sz)) = self.map.get(&oldest)
                && *t == ot
            {
                self.bytes = self.bytes.saturating_sub(*sz);
                self.map.remove(&oldest);
            }
        }
        self.want.remove(&k);
    }
}

#[derive(Default)]
pub struct Login {
    pub url: String,
    pub user: String,
    pub pass: String,
    pub remember: bool,
    pub busy: bool,
    pub error: String,
}

pub struct App {
    pub session: Option<Session>,
    pub cfg: Config,
    pub route: String,
    pub history: Vec<String>,
    pub hi: usize,
    pub cache: HashMap<String, (Instant, AnyData)>,
    pub pending: HashSet<String>,
    pub scrolls: HashMap<String, (f32, f32)>,
    pub covers: Covers,
    pub cover_tasks: usize,
    #[cfg(feature = "audio")]
    pub player: Option<navify_audio::Player>,
    pub queue: Vec<Song>,
    pub index: usize,
    pub shuffle: Option<Vec<usize>>,
    pub repeat: u8,
    pub source: String,
    pub playing: bool,
    pub position: f32,
    pub scrobbled: bool,
    pub starred: HashMap<String, bool>,
    pub ratings: HashMap<String, u8>,
    pub search_q: String,
    pub search_seq: u64,
    pub menu: Option<Menu>,
    pub toast: Option<(u64, String)>,
    pub toast_seq: u64,
    pub save_seq: u64,
    pub panel: u8,
    pub lib_filter: u8,
    pub lib_collapsed: bool,
    pub volume: f32,
    pub muted: bool,
    pub login: Login,
    pub visible: bool,
    pub pos: (f32, f32),
    pub modal: Option<(String, bool)>,
    pub shot: Option<String>,
    pub shot_ready: usize,
    pub new_pl: String,
    pub new_pl_ids: Vec<String>,
    pub sel: Option<String>,
    pub popular_all: bool,
    pub seek_preview: Option<f32>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn shot_key(shot: Option<&str>) -> Option<String> {
    let k = shot?.split('|').next()?;
    (!k.is_empty() && k != "login" && k != "*").then(|| k.to_string())
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self::new();
        let creds = (|| {
            Some((
                std::env::var("NAVIFY_URL").ok()?,
                std::env::var("NAVIFY_USER").ok()?,
                std::env::var("NAVIFY_PASS").ok()?,
            ))
        })();
        let t = if app.session.is_none() && creds.is_some() {
            let (u, n, p) = creds.unwrap();
            Task::perform(
                async move { Session::login(&u, &n, &p).await.map_err(|e| e.to_string()) },
                Message::LoggedIn,
            )
        } else if app.session.is_some() || app.route != "login" {
            let r = app.route.clone();
            app.route = String::new();
            app.nav(r)
        } else if app.shot.is_some() {
            Task::perform(async { tokio::time::sleep(Duration::from_millis(800)).await }, |_| {
                Message::ShotTake
            })
        } else {
            Task::none()
        };
        (app, t)
    }

    fn shot_setups(&self) -> Vec<String> {
        self.shot
            .as_deref()
            .and_then(|s| s.split('|').nth(1))
            .map(|x| x.split(',').map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn any_songs(&self) -> Vec<Song> {
        for (_, v) in self.cache.values() {
            if let Some(p) = v.downcast_ref::<Pager<Song>>()
                && !p.items.is_empty()
            {
                return p.items.clone();
            }
            if let Some((_, s)) = v.downcast_ref::<(Album, Vec<Song>)>()
                && !s.is_empty()
            {
                return s.clone();
            }
            if let Some((_, s)) = v.downcast_ref::<(Playlist, Vec<Song>)>()
                && !s.is_empty()
            {
                return s.clone();
            }
        }
        Vec::new()
    }

    fn apply_shot_setup(&mut self) {
        for s in self.shot_setups() {
            match s.as_str() {
                "panel1" => self.panel = 1,
                "panel2" => self.panel = 2,
                "queue" => {
                    self.queue = self.any_songs().into_iter().take(20).collect();
                    self.index = 0;
                    self.source = "Liked Songs".into();
                    self.panel = 2;
                }
                "menu" => {
                    if let Some(sg) = self.any_songs().into_iter().next() {
                        self.menu =
                            Some(Menu { x: 480.0, y: 300.0, kind: MenuKind::Song(sg), sub: 0 });
                    }
                }
                "toast" => {
                    self.toast = Some((u64::MAX, "Added to queue".into()));
                }
                _ => {}
            }
        }
    }

    fn maybe_shot(&mut self) -> Task<Message> {
        if self.shot_ready == 1 && self.covers.want.is_empty() && self.pending.is_empty() {
            self.shot_ready = 2;
            return Task::perform(
                async { tokio::time::sleep(Duration::from_millis(500)).await },
                |_| Message::ShotTake,
            );
        }
        Task::none()
    }

    pub fn new() -> Self {
        let cfg = Config::load();
        let shot = std::env::var("NAVIFY_SHOT").ok().filter(|s| !s.is_empty());
        let (session, mut route) = cfg
            .active
            .as_ref()
            .and_then(|id| cfg.servers.iter().find(|s| &s.id == id))
            .map(|s| (Some(Session::new(s.clone())), "home".to_string()))
            .unwrap_or((None, "login".to_string()));
        if let Some(k) = shot_key(shot.as_deref()) {
            route = if k == "login" { "login".into() } else { k };
        }
        let mut app = Self {
            session,
            volume: cfg.settings.volume,
            repeat: 0,
            cfg,
            route,
            history: vec!["home".into()],
            hi: 0,
            cache: HashMap::new(),
            pending: HashSet::new(),
            scrolls: HashMap::new(),
            covers: Covers::default(),
            cover_tasks: 0,
            #[cfg(feature = "audio")]
            player: {
                let (p, rx) = navify_audio::Player::new();
                *AUDIO_RX.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
                Some(p)
            },
            queue: Vec::new(),
            index: 0,
            shuffle: None,
            source: String::new(),
            playing: false,
            position: 0.0,
            scrobbled: false,
            starred: HashMap::new(),
            ratings: HashMap::new(),
            search_q: String::new(),
            search_seq: 0,
            menu: None,
            toast: None,
            toast_seq: 0,
            save_seq: 0,
            panel: 0,
            lib_filter: 0,
            lib_collapsed: false,
            muted: false,
            login: Login::default(),
            visible: true,
            pos: (0.0, 0.0),
            modal: None,
            shot,
            shot_ready: 0,
            new_pl: String::new(),
            new_pl_ids: Vec::new(),
            sel: None,
            popular_all: false,
            seek_preview: None,
        };
        if app.route == "login" {
            app.history = vec!["login".into()];
        }
        app
    }

    pub fn current_song(&self) -> Option<&Song> {
        self.queue.get(self.real_index())
    }

    pub fn real_index(&self) -> usize {
        self.shuffle.as_ref().and_then(|o| o.get(self.index).copied()).unwrap_or(self.index)
    }

    fn request(&mut self, key: &str) -> Task<Message> {
        match self.cache.get(key) {
            Some((t, _)) if t.elapsed() < STALE => return Task::none(),
            _ => {}
        }
        if !self.pending.insert(key.to_string()) {
            return Task::none();
        }
        let Some(s) = self.session.clone() else { return Task::none() };
        fetch(s, key.to_string(), 0)
    }

    fn request_page(&mut self, key: &str, offset: usize) -> Task<Message> {
        let pkey = format!("{key}#{offset}");
        if !self.pending.insert(pkey) {
            return Task::none();
        }
        let Some(s) = self.session.clone() else { return Task::none() };
        fetch(s, key.to_string(), offset)
    }

    fn toast(&mut self, text: &str) -> Task<Message> {
        self.toast_seq += 1;
        let n = self.toast_seq;
        self.toast = Some((n, text.to_string()));
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
            },
            move |_| Message::Toast(n, String::new()),
        )
    }

    fn debounce_save_queue(&mut self) -> Task<Message> {
        if !self.cfg.settings.queue_sync {
            return Task::none();
        }
        self.save_seq += 1;
        let n = self.save_seq;
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
            },
            move |_| Message::SaveQ(n),
        )
    }

    fn persist(&self) -> Task<Message> {
        let c = self.cfg.clone();
        Task::perform(
            async move {
                let _ = c.save();
            },
            |_| Message::Noop,
        )
    }

    pub fn gain(&self, song: &Song) -> f32 {
        let g = match self.cfg.settings.replaygain.as_str() {
            "track" => song.rg.track_gain.or(song.rg.album_gain),
            "album" => song.rg.album_gain.or(song.rg.track_gain),
            _ => None,
        };
        match g {
            Some(db) => {
                let lin = 10f32.powf((db + self.cfg.settings.preamp_db) / 20.0);
                let peak = song.rg.track_peak.or(song.rg.album_peak).unwrap_or(1.0).max(1e-3);
                lin.min(1.0 / peak)
            }
            None => 1.0,
        }
    }

    fn track(&self, song: &Song) -> Track {
        let (format, br) = transcode(&self.cfg.settings);
        let (url, transcoded) = self
            .session
            .as_ref()
            .map_or((String::new(), false), |s| s.stream_url(song, format.as_deref(), br));
        Track {
            id: song.id.clone(),
            url,
            gain: self.gain(song),
            transcoded,
            duration: song.duration,
        }
    }

    fn next_index(&self) -> Option<usize> {
        if self.queue.is_empty() {
            return None;
        }
        let len = self.queue.len();
        if self.index + 1 < len {
            Some(self.index + 1)
        } else if self.repeat == 1 {
            Some(0)
        } else {
            None
        }
    }

    fn play_index(&mut self, i: usize) -> Task<Message> {
        if self.queue.is_empty() {
            return Task::none();
        }
        self.index = i.min(self.queue.len() - 1);
        let song = self.queue[self.real_index()].clone();
        self.position = 0.0;
        self.scrobbled = false;
        self.playing = true;
        let t = self.track(&song);
        let next = self.next_index().map(|n| self.track(&self.queue[self.real_at(n)].clone()));
        #[cfg(feature = "audio")]
        {
            let p = self.player.get_or_insert_with(|| navify_audio::Player::new().0);
            p.set_volume(if self.muted { 0.0 } else { self.volume });
            p.set_speed(self.cfg.settings.speed);
            p.set_crossfade(self.cfg.settings.crossfade);
            p.play(t);
            p.queue_next(next);
        }
        #[cfg(not(feature = "audio"))]
        {
            let _ = t;
            let _ = next;
        }
        let lyr = if self.route == "lyrics" {
            self.request(&format!("lyrics:{}", song.id))
        } else {
            Task::none()
        };
        if let Some(s) = self.session.clone() {
            let id = song.id.clone();
            Task::batch([
                lyr,
                Task::perform(
                    async move {
                        let _ = s.scrobble(&id, false).await;
                    },
                    |_| Message::Noop,
                ),
                self.debounce_save_queue(),
            ])
        } else {
            Task::none()
        }
    }

    pub(crate) fn real_at(&self, i: usize) -> usize {
        self.shuffle.as_ref().and_then(|o| o.get(i).copied()).unwrap_or(i)
    }

    fn want_covers(&mut self, ids: impl IntoIterator<Item = (String, u32)>) -> Task<Message> {
        let Some(s) = self.session.clone() else { return Task::none() };
        let tasks: Vec<Task<Message>> = ids
            .into_iter()
            .filter(|(id, size)| {
                !id.is_empty()
                    && !self.covers.map.contains_key(&(id.clone(), *size))
                    && self.covers.want.insert((id.clone(), *size))
            })
            .map(|(id, size)| {
                let s = s.clone();
                let id2 = id.clone();
                Task::perform(
                    async move {
                        let b = s.cover(&id2, size).await.ok()?;
                        let (w, h, c, px) = decode(&b, size).await?;
                        Some((w, h, c, px))
                    },
                    move |r| Message::Cover(id, size, r),
                )
            })
            .collect();
        Task::batch(tasks)
    }
}

#[cfg(feature = "audio")]
use navify_audio::Track;
#[cfg(not(feature = "audio"))]
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
struct Track {
    #[allow(dead_code)]
    id: String,
    url: String,
    gain: f32,
    transcoded: bool,
    duration: f32,
}

async fn decode(b: &[u8], size: u32) -> Option<(u32, u32, Color, Vec<u8>)> {
    let img = ::image::load_from_memory(b).ok()?;
    let img = if img.width() > size || img.height() > size {
        img.resize(size, size, ::image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let (w, h) = (img.width(), img.height());
    let px = img.to_rgba8().into_raw();
    let c = if size == 640 { dominant(&img) } else { Color::BLACK };
    Some((w, h, c, px))
}

fn dominant(img: &::image::DynamicImage) -> Color {
    let small = img.resize_exact(16, 16, ::image::imageops::FilterType::Triangle).to_rgba8();
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    for p in small.pixels() {
        r += p[0] as u64;
        g += p[1] as u64;
        b += p[2] as u64;
    }
    let n = 256.0 * 256.0;
    let (mut r, mut g, mut b) = (r as f32 / n, g as f32 / n, b as f32 / n);
    let max = r.max(g).max(b).max(1.0);
    if max < 200.0 {
        let k = 200.0 / max;
        (r, g, b) = (r * k, g * k, b * k);
    }
    Color::from_rgb(r / 255.0 * 0.55, g / 255.0 * 0.55, b / 255.0 * 0.55)
}

fn transcode(s: &config::Settings) -> (Option<String>, u32) {
    match s.transcode_format.as_str() {
        "original" | "" => (None, s.transcode_max_bitrate),
        f => (Some(f.to_string()), s.transcode_max_bitrate),
    }
}

fn fetch(s: Session, key: String, offset: usize) -> Task<Message> {
    let key2 = key.clone();
    Task::perform(
        async move {
            let k = key.as_str();
            let (kind, arg) = k.split_once(':').unwrap_or((k, ""));
            let v: AnyData = match kind {
                "home" => {
                    let (r, a, f, x, p, st) = tokio::try_join!(
                        s.albums("recent", &[], 0, 12),
                        s.albums("newest", &[], 0, 12),
                        s.albums("frequent", &[], 0, 12),
                        s.albums("random", &[], 0, 12),
                        s.playlists(),
                        s.starred(),
                    )?;
                    Arc::new(Home {
                        recent: r.0,
                        added: a.0,
                        frequent: f.0,
                        random: x.0,
                        playlists: p,
                        starred: st,
                    })
                }
                "lib" => {
                    let (p, st) = tokio::try_join!(s.playlists(), s.starred())?;
                    Arc::new(Lib { playlists: p, artists: st.0, albums: st.1 })
                }
                "album" => Arc::new(s.album(arg).await?),
                "artist" => Arc::new(s.artist(arg).await?),
                "playlist" => Arc::new(s.playlist(arg).await?),
                "liked" => {
                    let (v, t) = s.songs("starred", &[], offset, PAGE).await?;
                    Arc::new(Pager { items: v, total: t, offset })
                }
                "albums" => {
                    let (v, t) = s
                        .albums(if arg.is_empty() { "newest" } else { arg }, &[], offset, PAGE)
                        .await?;
                    Arc::new(Pager { items: v, total: t, offset })
                }
                "artists" => {
                    let (v, t) = s.artists(offset, PAGE).await?;
                    Arc::new(Pager { items: v, total: t, offset })
                }
                "songs" => {
                    let (v, t) = s
                        .songs(if arg.is_empty() { "name" } else { arg }, &[], offset, PAGE)
                        .await?;
                    Arc::new(Pager { items: v, total: t, offset })
                }
                "genres" => Arc::new(s.genres().await?),
                "genre" => {
                    let (v, t) =
                        s.albums("newest", &[("genre", arg.to_string())], offset, PAGE).await?;
                    Arc::new(Pager { items: v, total: t, offset })
                }
                "folders" => Arc::new(s.indexes().await?),
                "folder" => Arc::new(s.directory(arg).await?),
                "lyrics" => Arc::new(s.lyrics(arg).await?),
                "search" => Arc::new({
                    let (a, b, c) = s.search(arg, 20).await?;
                    SearchRes { artists: a, albums: b, songs: c }
                }),
                _ => return Err(anyhow::anyhow!("unknown key {key}")),
            };
            Ok::<AnyData, anyhow::Error>(v)
        },
        move |r: Result<AnyData, anyhow::Error>| {
            Message::Loaded(key2, r.map_err(|e| e.to_string()))
        },
    )
}

impl App {
    fn star_task(&self, ids: &[String], on: bool) -> Task<Message> {
        let Some(s) = self.session.clone() else { return Task::none() };
        let ids: Vec<String> = ids.to_vec();
        Task::perform(async move { s.star(&ids, on).await.map_err(|e| e.to_string()) }, move |r| {
            match r {
                Ok(()) => Message::Noop,
                Err(e) => Message::Toast(0, format!("Could not update the favorites. {e}")),
            }
        })
    }

    fn nav(&mut self, route: String) -> Task<Message> {
        if route == self.route && !route.starts_with("search") {
            return Task::none();
        }
        self.history.truncate(self.hi + 1);
        self.history.push(route.clone());
        self.hi = self.history.len() - 1;
        let prev = std::mem::replace(&mut self.route, route.clone());
        if self.scrolls.contains_key(&prev) {
            let _ = prev;
        }
        self.menu = None;
        self.popular_all = false;
        let mut tasks = vec![self.request("lib")];
        match route.as_str() {
            "settings" | "lyrics" | "login" => {}
            _ => {
                tasks.insert(0, self.request(&route));
            }
        }
        if route == "search" || route.starts_with("search") {
            tasks.push(self.request("genres"));
        }
        if route == "lyrics"
            && let Some(s) = self.current_song()
        {
            tasks.push(self.request(&format!("lyrics:{}", s.id)));
        }
        let target = self.scrolls.get(&route).map(|(o, _)| *o).unwrap_or(0.0);
        tasks.push(iced::widget::operation::scroll_to(
            crate::view::SCROLL,
            iced::widget::operation::AbsoluteOffset::<Option<f32>> { x: None, y: Some(target) },
        ));
        Task::batch(tasks)
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Loaded(key, r) => {
                self.pending.retain(|p| p != &key && !p.starts_with(&format!("{key}#")));
                let Ok(v) = r else {
                    let e = r.err().unwrap_or_default();
                    if self.shot.is_some() {
                        eprintln!("LOAD-ERR {key} {e}");
                    }
                    return self.toast(&e);
                };
                if let Some(p) = key.rsplit('#').next().and_then(|o| o.parse::<usize>().ok()) {
                    let _ = p;
                }
                let v = self.merge(&key, v);
                self.cache.insert(key.clone(), (Instant::now(), v));
                let ids: Vec<(String, u32)> = self.visible_covers();
                let sk = shot_key(self.shot.as_deref());
                if self.shot_ready == 0 && sk.as_deref() == Some(key.as_str()) {
                    self.shot_ready = 1;
                    self.apply_shot_setup();
                }
                let wc = self.want_covers(ids);
                Task::batch([wc, self.maybe_shot()])
            }
            Message::Cover(id, size, r) => {
                self.covers.want.remove(&(id.clone(), size));
                if let Some((w, h, c, px)) = r {
                    self.covers.insert((id, size), w, h, c, px);
                }
                self.maybe_shot()
            }
            Message::Nav(r) => self.nav(r),
            Message::Back => {
                if self.hi > 0 {
                    self.hi -= 1;
                    let r = self.history[self.hi].clone();
                    self.route = r.clone();
                    self.menu = None;
                    return self.request(&r);
                }
                Task::none()
            }
            Message::Fwd => {
                if self.hi + 1 < self.history.len() {
                    self.hi += 1;
                    let r = self.history[self.hi].clone();
                    self.route = r.clone();
                    self.menu = None;
                    return self.request(&r);
                }
                Task::none()
            }
            Message::Scrolled(off, view, content) => {
                self.scrolls.insert(self.route.clone(), (off, view));
                let mut t = Task::none();
                if content > 0.0 && off + view * 3.0 >= content {
                    let key = self.route.clone();
                    if let Some(len) = self.page_len(&key) {
                        t = self.request_page(&key, len);
                    }
                }
                let ids = self.visible_covers();
                Task::batch([t, self.want_covers(ids)])
            }
            Message::Search(q) => {
                self.search_q = q;
                self.search_seq += 1;
                let n = self.search_seq;
                Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    },
                    move |_| Message::SearchGo(n),
                )
            }
            Message::SearchGo(n) => {
                if n != self.search_seq {
                    return Task::none();
                }
                let q = self.search_q.trim().to_string();
                let r = if q.is_empty() { "search".into() } else { format!("search:{q}") };
                if r != self.route { self.nav(r) } else { self.request(&r) }
            }
            Message::Play(songs, i, source) => {
                self.queue = songs;
                self.source = source;
                self.shuffle = self.shuffle.as_ref().map(|_| {
                    let mut o: Vec<usize> = (0..self.queue.len()).collect();
                    shuffle(&mut o, i);
                    o
                });
                self.play_index(i)
            }
            Message::PlayQueue(i) => self.play_index(i),
            Message::Toggle => {
                self.playing = !self.playing;
                #[cfg(feature = "audio")]
                if let Some(p) = &self.player {
                    p.set_paused(!self.playing);
                }
                self.debounce_save_queue()
            }
            Message::Prev => {
                if self.position > 3.0 {
                    return self.update(Message::Seek(0.0));
                }
                if self.index > 0 {
                    return self.play_index(self.index - 1);
                }
                Task::none()
            }
            Message::Next(manual) => {
                if self.repeat == 2 && !manual {
                    return self.play_index(self.index);
                }
                match self.next_index() {
                    Some(n) => self.play_index(n),
                    None => {
                        self.playing = false;
                        #[cfg(feature = "audio")]
                        if let Some(p) = &self.player {
                            p.stop();
                        }
                        Task::none()
                    }
                }
            }
            Message::Ended => self.update(Message::Next(false)),
            Message::Seek(pos) => {
                self.position = pos;
                #[cfg(feature = "audio")]
                if let Some(p) = &self.player {
                    p.seek(pos);
                }
                Task::none()
            }
            Message::Shuffle => {
                self.shuffle = match self.shuffle {
                    Some(_) => None,
                    None => {
                        let cur = self.real_index();
                        let mut o: Vec<usize> = (0..self.queue.len()).collect();
                        shuffle(&mut o, cur);
                        self.index = 0;
                        Some(o)
                    }
                };
                if self.shuffle.is_none()
                    && let Some(i) = self
                        .queue
                        .iter()
                        .position(|s| self.current_song().is_some_and(|c| c.id == s.id))
                {
                    self.index = i;
                }
                #[cfg(feature = "audio")]
                if let Some(p) = &self.player {
                    p.queue_next(
                        self.next_index().map(|n| self.track(&self.queue[self.real_at(n)].clone())),
                    );
                }
                self.debounce_save_queue()
            }
            Message::Repeat => {
                self.repeat = (self.repeat + 1) % 3;
                Task::none()
            }
            Message::Vol(v) => {
                self.volume = v;
                self.cfg.settings.volume = v;
                self.muted = false;
                #[cfg(feature = "audio")]
                if let Some(p) = &self.player {
                    p.set_volume(v);
                }
                Task::none()
            }
            Message::Mute => {
                self.muted = !self.muted;
                #[cfg(feature = "audio")]
                if let Some(p) = &self.player {
                    p.set_volume(if self.muted { 0.0 } else { self.volume });
                }
                Task::none()
            }
            Message::Star(id, on) => {
                self.starred.insert(id.clone(), on);
                self.star_task(&[id], on)
            }
            Message::Rate(id, n) => {
                self.ratings.insert(id.clone(), n);
                self.menu = None;
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(async move { s.rate(&id, n).await.map_err(|e| e.to_string()) }, |r| {
                    match r {
                        Ok(()) => Message::Noop,
                        Err(e) => Message::Toast(0, format!("Could not set the rating. {e}")),
                    }
                })
            }
            Message::QNext(songs) => {
                let at = (self.index + 1).min(self.queue.len());
                self.insert_queue(songs, at);
                self.queue_next_refresh();
                self.debounce_save_queue()
            }
            Message::QEnd(songs) => {
                let at = self.queue.len();
                self.insert_queue(songs, at);
                self.queue_next_refresh();
                self.debounce_save_queue()
            }
            Message::QDel(i) => {
                let ri = self.real_at(i);
                if ri < self.queue.len() && i != self.index {
                    self.queue.remove(ri);
                    if let Some(o) = &mut self.shuffle {
                        o.retain(|&x| x != ri);
                        for x in o.iter_mut() {
                            if *x > ri {
                                *x -= 1;
                            }
                        }
                    } else if self.index > ri {
                        self.index -= 1;
                    }
                }
                self.queue_next_refresh();
                self.debounce_save_queue()
            }
            Message::QClear => {
                self.queue.clear();
                self.index = 0;
                self.playing = false;
                #[cfg(feature = "audio")]
                if let Some(p) = &self.player {
                    p.stop();
                }
                self.debounce_save_queue()
            }
            Message::Menu(kind) => {
                self.menu = Some(Menu { x: self.pos.0, y: self.pos.1, kind: *kind, sub: 0 });
                Task::none()
            }
            Message::MenuSub(n) => {
                if let Some(m) = &mut self.menu {
                    m.sub = n;
                }
                Task::none()
            }
            Message::MenuClose => {
                self.menu = None;
                Task::none()
            }
            Message::PlAdd(pid, ids) => {
                self.menu = None;
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move { s.playlist_update(&pid, None, None, &ids, &[]).await },
                    move |r| match r {
                        Ok(()) => Message::Toast(0, "Added to the playlist.".into()),
                        Err(e) => Message::Toast(0, format!("Could not update the playlist. {e}")),
                    },
                )
            }
            Message::PlNew(name) => {
                self.menu = None;
                self.modal = None;
                self.new_pl.clear();
                let ids = std::mem::take(&mut self.new_pl_ids);
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move {
                        let p = s.playlist_create(&name, &[]).await?;
                        if !ids.is_empty() {
                            let _ = s.playlist_update(&p.id, None, None, &ids, &[]).await;
                        }
                        Ok::<Playlist, anyhow::Error>(p)
                    },
                    |r| match r {
                        Ok(p) => Message::Nav(format!("playlist:{}", p.id)),
                        Err(e) => Message::Toast(0, format!("Could not create the playlist. {e}")),
                    },
                )
            }
            Message::PlDel(id) => {
                self.menu = None;
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move { s.playlist_delete(&id).await.map_err(|e| e.to_string()) },
                    move |r| match r {
                        Ok(()) => Message::Nav("home".into()),
                        Err(e) => Message::Toast(0, format!("Could not delete the playlist. {e}")),
                    },
                )
            }
            Message::PlRemove(idx) => {
                self.menu = None;
                let Some(pid) = self.route.strip_prefix("playlist:").map(str::to_string) else {
                    return Task::none();
                };
                if let Some((_, p)) = self.cache.get(&self.route)
                    && let Some((pl, tracks)) = p.downcast_ref::<(Playlist, Vec<Song>)>()
                {
                    let mut tracks = tracks.clone();
                    tracks.remove(idx);
                    self.cache.insert(
                        self.route.clone(),
                        (Instant::now(), Arc::new((pl.clone(), tracks))),
                    );
                }
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move { s.playlist_update(&pid, None, None, &[], &[idx]).await },
                    |r| match r {
                        Ok(()) => Message::Noop,
                        Err(e) => Message::Toast(0, format!("Could not update the playlist. {e}")),
                    },
                )
            }
            Message::Radio(id) => {
                self.menu = None;
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move { s.similar_songs(&id, 30).await.map_err(|e| e.to_string()) },
                    |r| match r {
                        Ok(v) if !v.is_empty() => Message::Play(v, 0, "Radio".into()),
                        Ok(_) => Message::Toast(0, "No similar songs were found.".into()),
                        Err(e) => Message::Toast(0, format!("Could not start the radio. {e}")),
                    },
                )
            }
            Message::Toast(n, text) => {
                if text.is_empty() {
                    if self.toast.as_ref().is_some_and(|(tn, _)| *tn == n || n == 0) {
                        self.toast = None;
                    }
                    Task::none()
                } else {
                    self.toast(&text)
                }
            }
            Message::Panel(n) => {
                self.panel = if self.panel == n { 0 } else { n };
                Task::none()
            }
            Message::LibFilter(n) => {
                self.lib_filter = if self.lib_filter == n { 0 } else { n };
                Task::none()
            }
            Message::LibCollapse => {
                self.lib_collapsed = !self.lib_collapsed;
                Task::none()
            }
            Message::NewPlaylist => {
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move {
                        s.playlist_create("New playlist", &[]).await.map_err(|e| e.to_string())
                    },
                    |r| match r {
                        Ok(p) => Message::Nav(format!("playlist:{}", p.id)),
                        Err(e) => Message::Toast(0, format!("Could not create the playlist. {e}")),
                    },
                )
            }
            Message::LField(i, v) => {
                match i {
                    0 => self.login.url = v,
                    1 => self.login.user = v,
                    2 => self.login.pass = v,
                    _ => self.new_pl = v,
                }
                Task::none()
            }
            Message::LRemember(b) => {
                self.login.remember = b;
                Task::none()
            }
            Message::LSubmit => {
                if self.login.busy || self.login.url.is_empty() || self.login.user.is_empty() {
                    return Task::none();
                }
                self.login.busy = true;
                self.login.error.clear();
                let (u, n, p) =
                    (self.login.url.clone(), self.login.user.clone(), self.login.pass.clone());
                Task::perform(
                    async move { Session::login(&u, &n, &p).await.map_err(|e| e.to_string()) },
                    Message::LoggedIn,
                )
            }
            Message::LoggedIn(r) => {
                self.login.busy = false;
                match r {
                    Ok(s) => {
                        if self.login.remember {
                            let _ = config::remember(&s.server.id, &self.login.pass);
                        }
                        self.cfg.servers.retain(|x| x.id != s.server.id);
                        self.cfg.servers.push(s.server.clone());
                        self.cfg.active = Some(s.server.id.clone());
                        self.session = Some(s.clone());
                        self.login = Login::default();
                        let t = self.persist();
                        let n = self
                            .nav(shot_key(self.shot.as_deref()).unwrap_or_else(|| "home".into()));
                        let s2 = s.clone();
                        let q =
                            Task::perform(async move { s2.play_queue().await.ok() }, move |r| {
                                match r {
                                    Some((songs, cur, pos)) => {
                                        Message::RestoreQueue(songs, cur, pos)
                                    }
                                    None => Message::Noop,
                                }
                            });
                        Task::batch([t, n, q])
                    }
                    Err(e) => {
                        self.login.error = e;
                        Task::none()
                    }
                }
            }
            Message::RestoreQueue(songs, cur, pos) => {
                if songs.is_empty() {
                    return Task::none();
                }
                self.position = pos as f32 / 1000.0;
                self.index = songs.iter().position(|s| s.id == cur).unwrap_or(0);
                self.queue = songs;
                Task::none()
            }
            Message::LogOut => {
                let id = self.cfg.active.take();
                if let Some(id) = id {
                    let _ = config::forget(&id);
                    self.cfg.servers.retain(|s| s.id != id);
                }
                self.session = None;
                self.queue.clear();
                self.playing = false;
                self.cache.clear();
                let t = self.persist();
                let n = self.nav("login".into());
                Task::batch([t, n])
            }
            Message::UseServer(id) => {
                let Some(srv) = self.cfg.servers.iter().find(|s| s.id == id) else {
                    return Task::none();
                };
                self.cfg.active = Some(id.clone());
                let sess = Session::new(srv.clone());
                let s = sess.clone();
                self.session = Some(sess);
                self.cache.clear();
                self.pending.clear();
                let t = self.persist();
                let n = self.nav("home".into());
                let check = Task::perform(
                    async move { s.rest::<serde_json::Value>("ping", &[], "").await.is_ok() },
                    |ok| if ok { Message::Noop } else { Message::NativeFailed },
                );
                Task::batch([t, n, check])
            }
            Message::NativeFailed => {
                if let Some(s) = &mut self.session {
                    s.server.native = false;
                }
                self.toast("The server session expired. Using the compatible API.")
            }
            Message::DropServer(id) => {
                self.cfg.servers.retain(|s| s.id != id);
                self.persist()
            }
            Message::Set(set) => {
                match set {
                    Set::Rg(v) => self.cfg.settings.replaygain = v,
                    Set::Preamp(v) => self.cfg.settings.preamp_db = v,
                    Set::Xfade(v) => self.cfg.settings.crossfade_secs = v,
                    Set::Trans(v) => {
                        self.cfg.settings.transcode_format =
                            ["original", "mp3", "opus", "aac"][v as usize].into()
                    }
                    Set::Bitrate(v) => self.cfg.settings.transcode_max_bitrate = v,
                    Set::Qsync(b) => self.cfg.settings.queue_sync = b,
                    Set::Discord(b) => self.cfg.settings.discord = b,
                    Set::Speed(v) => {
                        self.cfg.settings.speed = v;
                        #[cfg(feature = "audio")]
                        if let Some(p) = &self.player {
                            p.set_speed(v);
                        }
                    }
                }
                self.persist()
            }
            Message::ClearCache => {
                self.covers.map.clear();
                self.covers.bytes = 0;
                self.covers.color.clear();
                Task::perform(
                    async move {
                        let d = dirs::cache_dir()
                            .unwrap_or_else(|| "/tmp".into())
                            .join("navify/covers");
                        let _ = tokio::fs::remove_dir_all(d).await;
                    },
                    |_| Message::Noop,
                )
            }
            Message::Focus(v) => {
                self.visible = v;
                Task::none()
            }
            Message::Cursor(x, y) => {
                self.pos = (x, y);
                Task::none()
            }
            Message::Close => {
                let c = self.cfg.clone();
                let s = self.session.clone();
                let q = self.cfg.settings.queue_sync.then(|| {
                    (
                        self.queue.iter().map(|x| x.id.clone()).collect::<Vec<_>>(),
                        self.current_song().map(|x| x.id.clone()).unwrap_or_default(),
                        (self.position * 1000.0) as u64,
                    )
                });
                Task::perform(
                    async move {
                        let _ = c.save();
                        if let (Some(s), Some((ids, cur, pos))) = (s, q) {
                            let _ = s.save_play_queue(&ids, &cur, pos).await;
                        }
                    },
                    |_| Message::Quit,
                )
            }
            Message::Quit => iced::exit(),
            Message::Noop => Task::none(),
            Message::ShotTake => iced::window::latest()
                .and_then(|id| iced::window::screenshot(id).map(Message::Shot)),
            Message::Shot(shot) => {
                let name = self
                    .shot
                    .as_deref()
                    .and_then(|s| s.split('|').next())
                    .unwrap_or("shot")
                    .replace([':', '/'], "_");
                let size = shot.size;
                Task::perform(
                    async move {
                        let path = format!("/tmp/navify-shots/{name}.png");
                        let _ = ::image::save_buffer(
                            &path,
                            shot.as_ref(),
                            size.width,
                            size.height,
                            ::image::ExtendedColorType::Rgba8,
                        );
                    },
                    |_| Message::Quit,
                )
            }
            Message::SaveQ(n) => {
                if n != self.save_seq {
                    return Task::none();
                }
                let Some(s) = self.session.clone() else { return Task::none() };
                let ids: Vec<String> = self.queue.iter().map(|x| x.id.clone()).collect();
                let cur = self.current_song().map(|x| x.id.clone()).unwrap_or_default();
                let pos = (self.position * 1000.0) as u64;
                Task::perform(
                    async move {
                        let _ = s.save_play_queue(&ids, &cur, pos).await;
                    },
                    |_| Message::Noop,
                )
            }
            Message::PlayCtx(route, shuf) => {
                let Some(s) = self.session.clone() else { return Task::none() };
                let (kind, id) = route.split_once(':').unwrap_or((route.as_str(), ""));
                let (kind, id) = (kind.to_string(), id.to_string());
                Task::perform(
                    async move {
                        let (songs, src) = match kind.as_str() {
                            "album" => {
                                let (a, v) = s.album(&id).await?;
                                (v, a.name)
                            }
                            "playlist" => {
                                let (p, v) = s.playlist(&id).await?;
                                (v, p.name)
                            }
                            "artist" => {
                                let (a, _, t, _) = s.artist(&id).await?;
                                (t, a.name)
                            }
                            _ => (Vec::new(), String::new()),
                        };
                        Ok::<_, anyhow::Error>((songs, src))
                    },
                    move |r: Result<(Vec<Song>, String), anyhow::Error>| match r {
                        Ok((v, src)) if !v.is_empty() => Message::PlayCtxDone(v, src, shuf),
                        Ok(_) => Message::Toast(0, "Nothing to play.".into()),
                        Err(e) => Message::Toast(0, format!("Could not play. {e}")),
                    },
                )
            }
            Message::PlayCtxDone(songs, src, shuf) => {
                self.queue = songs;
                self.source = src;
                self.shuffle = if shuf {
                    let mut o: Vec<usize> = (0..self.queue.len()).collect();
                    shuffle(&mut o, 0);
                    Some(o)
                } else {
                    None
                };
                self.play_index(0)
            }
            Message::AlbumQueue(id) => {
                self.menu = None;
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move { s.album(&id).await.map(|(_, v)| v).map_err(|e| e.to_string()) },
                    |r| match r {
                        Ok(v) => Message::QEnd(v),
                        Err(e) => Message::Toast(0, format!("Could not add the album. {e}")),
                    },
                )
            }
            Message::Select(id) => {
                self.sel = Some(id);
                Task::none()
            }
            Message::FocusSearch => iced::widget::operation::focus("search"),
            Message::VolUp => self.update(Message::Vol((self.volume + 0.1).min(1.0))),
            Message::VolDown => self.update(Message::Vol((self.volume - 0.1).max(0.0))),
            Message::Resize(w, h) => {
                self.cfg.settings.window = (w, h);
                Task::none()
            }
            Message::Popular => {
                self.popular_all = true;
                Task::none()
            }
            Message::SeekPreview(v) => {
                self.seek_preview = Some(v);
                Task::none()
            }
            Message::SeekRelease => {
                if let Some(v) = self.seek_preview.take() {
                    return self.update(Message::Seek(v));
                }
                Task::none()
            }
            Message::Tick => {
                if self.playing {
                    #[cfg(not(feature = "audio"))]
                    {
                        self.position += 0.25;
                        if self.position >= self.current_dur() && self.current_dur() > 0.0 {
                            return self.update(Message::Ended);
                        }
                    }
                    #[cfg(feature = "audio")]
                    if let Some(p) = &self.player {
                        self.position = p.position();
                        self.playing = !p.is_paused();
                    }
                    if !self.scrobbled
                        && self.current_dur() > 0.0
                        && (self.position > self.current_dur() * 0.5 || self.position > 240.0)
                    {
                        self.scrobbled = true;
                        if let (Some(s), Some(song)) =
                            (self.session.clone(), self.current_song().cloned())
                        {
                            return Task::perform(
                                async move {
                                    let _ = s.scrobble(&song.id, true).await;
                                },
                                |_| Message::Noop,
                            );
                        }
                    }
                }
                Task::none()
            }
            Message::Modal(id, rename) => {
                self.new_pl_ids = match self.menu.take().map(|m| m.kind) {
                    Some(MenuKind::Song(s) | MenuKind::SongIn(s, _)) => vec![s.id],
                    _ => Vec::new(),
                };
                self.modal = Some((id, rename));
                self.new_pl.clear();
                Task::none()
            }
            Message::Rename(id, name) => {
                self.menu = None;
                self.modal = None;
                self.new_pl.clear();
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move { s.playlist_update(&id, Some(&name), None, &[], &[]).await },
                    |r| match r {
                        Ok(()) => Message::Noop,
                        Err(e) => Message::Toast(0, format!("Could not rename the playlist. {e}")),
                    },
                )
            }
            #[cfg(feature = "audio")]
            Message::Audio(e) => match e {
                navify_audio::Event::Started(_) => Task::none(),
                navify_audio::Event::Ended => self.update(Message::Ended),
                navify_audio::Event::Error(e) => self.toast(&format!("Playback failed. {e}")),
            },
        }
    }

    fn current_dur(&self) -> f32 {
        self.current_song().map_or(0.0, |s| s.duration)
    }

    fn insert_queue(&mut self, songs: Vec<Song>, at: usize) {
        let n = songs.len();
        if let Some(o) = &mut self.shuffle {
            let base = self.queue.len();
            self.queue.extend(songs);
            let mut extra: Vec<usize> = (base..base + n).collect();
            shuffle(&mut extra, usize::MAX);
            let pos = (self.index + 1).min(o.len());
            for (k, x) in extra.into_iter().enumerate() {
                o.insert(pos + k, x);
            }
        } else {
            let at = at.min(self.queue.len());
            if at <= self.index && !self.queue.is_empty() {
                self.index += n;
            }
            for (k, s) in songs.into_iter().enumerate() {
                self.queue.insert(at + k, s);
            }
        }
    }

    fn queue_next_refresh(&self) {
        #[cfg(feature = "audio")]
        if let Some(p) = &self.player {
            p.queue_next(
                self.next_index().map(|n| self.track(&self.queue[self.real_at(n)].clone())),
            );
        }
    }

    fn merge(&mut self, key: &str, v: AnyData) -> AnyData {
        if let Some(old) = self.cache.get(key) {
            macro_rules! mrg {
                ($t:ty) => {
                    if let (Some(a), Some(b)) =
                        (old.1.downcast_ref::<Pager<$t>>(), v.downcast_ref::<Pager<$t>>())
                    {
                        if b.offset > 0 {
                            let mut items = a.items.clone();
                            items.extend(b.items.iter().cloned());
                            return Arc::new(Pager { items, total: b.total, offset: 0 });
                        }
                    }
                };
            }
            mrg!(Album);
            mrg!(Artist);
            mrg!(Song);
        }
        v
    }

    fn page_len(&self, key: &str) -> Option<usize> {
        let (_, v) = self.cache.get(key)?;
        macro_rules! chk {
            ($t:ty) => {
                if let Some(p) = v.downcast_ref::<Pager<$t>>() {
                    return (p.items.len() < p.total).then_some(p.items.len());
                }
            };
        }
        chk!(Album);
        chk!(Artist);
        chk!(Song);
        None
    }

    fn visible_covers(&self) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = Vec::new();
        let (off, view) = self.scrolls.get(&self.route).copied().unwrap_or((0.0, 800.0));
        let lo = (off / 56.0).max(0.0) as usize;
        let hi = lo + (view / 56.0) as usize + 24;
        if let Some(s) = self.current_song() {
            out.push((s.cover.clone(), 128));
            out.push((s.cover.clone(), 640));
        }
        let (kind, _arg) = self.route.split_once(':').unwrap_or((self.route.as_str(), ""));
        let v = self.cache.get(&self.route).map(|(_, v)| v.clone());
        let Some(v) = v else {
            if self.route == "home" || kind == "search" {
                return out;
            }
            return out;
        };
        match kind {
            "album" => {
                if let Some((a, songs)) = v.downcast_ref::<(Album, Vec<Song>)>() {
                    out.push((a.cover.clone(), 640));
                    out.extend(
                        songs[lo.min(songs.len())..hi.min(songs.len())]
                            .iter()
                            .map(|s| (s.cover.clone(), 128)),
                    );
                }
            }
            "playlist" => {
                if let Some((p, songs)) = v.downcast_ref::<(Playlist, Vec<Song>)>() {
                    out.push((p.cover.clone(), 640));
                    out.extend(
                        songs[lo.min(songs.len())..hi.min(songs.len())]
                            .iter()
                            .map(|s| (s.cover.clone(), 128)),
                    );
                }
            }
            "artist" => {
                if let Some((a, albums, top, _)) =
                    v.downcast_ref::<(Artist, Vec<Album>, Vec<Song>, Info)>()
                {
                    out.push((a.cover.clone(), 640));
                    out.extend(albums.iter().take(20).map(|x| (x.cover.clone(), 300)));
                    out.extend(top.iter().take(10).map(|x| (x.cover.clone(), 128)));
                }
            }
            "liked" | "songs" => {
                if let Some(p) = v.downcast_ref::<Pager<Song>>() {
                    out.extend(
                        p.items[lo.min(p.items.len())..hi.min(p.items.len())]
                            .iter()
                            .map(|s| (s.cover.clone(), 128)),
                    );
                }
            }
            "albums" | "genre" => {
                if let Some(p) = v.downcast_ref::<Pager<Album>>() {
                    let (lo, hi) = grid_window(off, view, p.items.len());
                    out.extend(p.items[lo..hi].iter().map(|a| (a.cover.clone(), 300)));
                }
            }
            "artists" | "folders" => {
                if let Some(p) = v.downcast_ref::<Pager<Artist>>() {
                    let (lo, hi) = grid_window(off, view, p.items.len());
                    out.extend(p.items[lo..hi].iter().map(|a| (a.cover.clone(), 300)));
                } else if let Some(a) = v.downcast_ref::<Vec<Artist>>() {
                    let (lo, hi) = grid_window(off, view, a.len());
                    out.extend(a[lo..hi].iter().map(|x| (x.cover.clone(), 300)));
                }
            }
            "folder" => {
                if let Some(songs) = v.downcast_ref::<Vec<Song>>() {
                    out.extend(
                        songs[lo.min(songs.len())..hi.min(songs.len())]
                            .iter()
                            .map(|s| (s.cover.clone(), 128)),
                    );
                }
            }
            "search" => {
                if let Some(r) = v.downcast_ref::<SearchRes>() {
                    out.extend(r.artists.iter().map(|a| (a.cover.clone(), 300)));
                    out.extend(r.albums.iter().map(|a| (a.cover.clone(), 300)));
                    out.extend(r.songs.iter().take(8).map(|s| (s.cover.clone(), 128)));
                }
            }
            "home" => {
                if let Some(h) = v.downcast_ref::<Home>() {
                    for l in [&h.recent, &h.added, &h.frequent, &h.random] {
                        out.extend(l.iter().map(|a| (a.cover.clone(), 300)));
                    }
                    out.extend(h.playlists.iter().map(|p| (p.cover.clone(), 300)));
                }
            }
            "lib" => {}
            _ => {}
        }
        if let Some((_, v)) = self.cache.get("lib")
            && let Some(l) = v.downcast_ref::<Lib>()
        {
            out.extend(l.playlists.iter().take(30).map(|p| (p.cover.clone(), 128)));
            out.extend(l.albums.iter().take(30).map(|a| (a.cover.clone(), 128)));
        }
        out
    }
}

fn shuffle(order: &mut [usize], keep_first: usize) {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    let mut v: Vec<(u64, usize)> = order
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            h.write_usize(x);
            h.write_usize(i);
            (h.finish(), x)
        })
        .collect();
    v.sort_by_key(|(k, _)| *k);
    for (d, (_, x)) in order.iter_mut().zip(v) {
        *d = x;
    }
    if let Some(p) = order.iter().position(|&x| x == keep_first) {
        order.swap(0, p);
    }
}

fn grid_window(off: f32, view: f32, len: usize) -> (usize, usize) {
    let cols = 6;
    let row_h = 246.0;
    let first = ((off / row_h).max(0.0) as usize).saturating_sub(1) * cols;
    let last = (((off + view) / row_h) as usize + 3) * cols;
    (first.min(len), last.min(len))
}

#[cfg(feature = "audio")]
static AUDIO_RX: std::sync::Mutex<Option<std::sync::mpsc::Receiver<navify_audio::Event>>> =
    std::sync::Mutex::new(None);

impl App {
    pub fn view(&self) -> iced::Element<'_, Message> {
        crate::view::view(self)
    }

    pub fn title(&self) -> String {
        match self.current_song() {
            Some(s) if !self.queue.is_empty() => {
                format!("navify - {} \u{2022} {}", s.title, s.artist)
            }
            _ => "navify".into(),
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        use iced::event;
        let mut subs = vec![event::listen_with(|e, status, _id| match e {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(Message::Cursor(position.x, position.y))
            }
            iced::Event::Window(iced::window::Event::Focused) => Some(Message::Focus(true)),
            iced::Event::Window(iced::window::Event::Unfocused) => Some(Message::Focus(false)),
            iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::Close),
            iced::Event::Window(iced::window::Event::Resized(s)) => {
                Some(Message::Resize(s.width, s.height))
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. })
                if status == iced::event::Status::Ignored =>
            {
                use iced::keyboard::{Key, key::Named};
                if modifiers.command() {
                    match key {
                        Key::Character(c) => match c.to_lowercase().as_str() {
                            "k" | "l" => Some(Message::FocusSearch),
                            "s" => Some(Message::Shuffle),
                            "r" => Some(Message::Repeat),
                            _ => None,
                        },
                        Key::Named(Named::ArrowLeft) => Some(Message::Prev),
                        Key::Named(Named::ArrowRight) => Some(Message::Next(true)),
                        Key::Named(Named::ArrowUp) => Some(Message::VolUp),
                        Key::Named(Named::ArrowDown) => Some(Message::VolDown),
                        _ => None,
                    }
                } else {
                    match key {
                        Key::Named(Named::Space) => Some(Message::Toggle),
                        Key::Named(Named::ArrowLeft) if modifiers.alt() => Some(Message::Back),
                        Key::Named(Named::ArrowRight) if modifiers.alt() => Some(Message::Fwd),
                        Key::Named(Named::Escape) => Some(Message::MenuClose),
                        _ => None,
                    }
                }
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(b))
                if matches!(b, iced::mouse::Button::Back | iced::mouse::Button::Forward) =>
            {
                Some(if b == iced::mouse::Button::Back { Message::Back } else { Message::Fwd })
            }
            _ => None,
        })];
        if self.playing && self.visible {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::Tick),
            );
        }
        #[cfg(feature = "audio")]
        {
            fn audio_stream() -> impl iced::futures::Stream<Item = Message> {
                iced::stream::channel(8, |mut tx| async move {
                    let Some(rx) = AUDIO_RX.lock().ok().and_then(|mut r| r.take()) else {
                        return;
                    };
                    let (ftx, mut frx) = iced::futures::channel::mpsc::unbounded();
                    std::thread::spawn(move || {
                        while let Ok(e) = rx.recv() {
                            let _ = ftx.unbounded_send(e);
                        }
                    });
                    use iced::futures::{SinkExt, StreamExt};
                    while let Some(e) = frx.next().await {
                        let _ = tx.send(Message::Audio(e)).await;
                    }
                })
            }
            subs.push(iced::Subscription::run(audio_stream));
        }
        iced::Subscription::batch(subs)
    }
}
