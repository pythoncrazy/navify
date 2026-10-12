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
pub const RIGHT_W: f32 = 320.0;
const STALE: Duration = Duration::from_secs(300);
const COVER_CAP: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
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
}

#[derive(Clone, Debug)]
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
    Shown(bool),
    Close,
    Noop,
    Quit,
    SaveQ(u64),
    Tick,
    Edit(String, String, String),
    Modal(String, u8),
    StarFailed(String, bool, String),
    PlCreated(Playlist),
    RestoreQueue(Vec<Song>, String, u64),
    NativeFailed,
    PlayCtx(String, bool),
    PlayCtxDone(Vec<Song>, String, bool),
    AlbumQueue(String),
    Select(String),
    Hover(String, bool),
    PlayAt(String, usize),
    Retry,
    Popular,
    FocusSearch,
    VolUp,
    VolDown,
    Resize(f32, f32),
    SeekPreview(f32),
    SeekRelease,
    Audio(navify_audio::Event),
    Media(souvlaki::MediaControlEvent),
    MediaInit(isize),
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
        self.color.insert(k.0.clone(), c);
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
    pub player: Option<navify_audio::Player>,
    media: Option<souvlaki::MediaControls>,
    discord: Option<std::sync::mpsc::Sender<Option<(String, String, i64)>>>,
    os: (String, bool),
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    pub queue: Vec<Song>,
    pub index: usize,
    pub shuffle: Option<Vec<usize>>,
    pub repeat: u8,
    pub source: String,
    pub playing: bool,
    pub position: f32,
    pub scrobbled: bool,
    pub loaded: bool,
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
    pub modal: Option<(String, u8)>,
    pub new_pl_desc: String,
    pub shot: Option<String>,
    pub new_pl: String,
    pub new_pl_ids: Vec<String>,
    pub sel: Option<String>,
    pub hover: Option<String>,
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
        let prune = Task::perform(
            async {
                let _ = tokio::task::spawn_blocking(crate::api::prune_covers).await;
            },
            |_| Message::Noop,
        );
        let media = if app.shot.is_some() {
            Task::none()
        } else if !cfg!(windows) {
            app.update(Message::MediaInit(0))
        } else {
            iced::window::latest()
                .and_then(|id| {
                    iced::window::run(id, |w| {
                        use iced::window::raw_window_handle::RawWindowHandle;
                        match w.window_handle().map(|h| h.as_raw()) {
                            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
                            _ => 0,
                        }
                    })
                })
                .map(Message::MediaInit)
        };
        let t = Task::batch([
            prune,
            media,
            if app.session.is_none()
                && !app.shot.as_deref().is_some_and(|s| s.starts_with("login"))
                && let Some((u, n, p)) = creds
            {
                Task::perform(
                    async move { Session::login(&u, &n, &p).await.map_err(|e| e.to_string()) },
                    Message::LoggedIn,
                )
            } else if app.session.is_some() || app.route != "login" {
                let r = app.route.clone();
                app.route = String::new();
                Task::batch([app.nav(r), app.restore()])
            } else {
                Task::none()
            },
        ]);
        (app, t)
    }

    fn any_songs(&self) -> Vec<Song> {
        self.cache
            .values()
            .filter_map(|(_, v)| song_list(v))
            .find(|(s, _)| !s.is_empty())
            .map_or_else(Vec::new, |(s, _)| s.to_vec())
    }

    fn apply_shot_setup(&mut self) -> Task<Message> {
        let mut task = Task::none();
        let setups = self.shot.take().and_then(|s| s.split_once('|').map(|(_, x)| x.to_string()));
        for s in setups.iter().flat_map(|x| x.split(',')) {
            match s {
                "queue" | "panel1" => {
                    self.queue = self.any_songs().into_iter().take(20).collect();
                    self.index = 0;
                    self.source = "Liked Songs".into();
                    self.panel = if s == "queue" { 2 } else { 1 };
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
                "lyrics" => {
                    if let Some(sg) = self.any_songs().into_iter().next() {
                        self.queue = vec![sg];
                        self.index = 0;
                        task = self.nav("lyrics".into());
                    }
                }
                _ => {}
            }
        }
        task
    }

    pub fn data<T: 'static>(&self, key: &str) -> Option<&T> {
        self.cache.get(key).and_then(|(_, v)| v.downcast_ref::<T>())
    }

    pub fn is_starred(&self, id: &str, server: bool) -> bool {
        self.starred.get(id).copied().unwrap_or(server)
    }

    pub fn lib_w(&self) -> f32 {
        if self.lib_collapsed { 72.0 } else { 300.0 }
    }

    pub fn main_w(&self) -> f32 {
        self.cfg.settings.window.0
            - 24.0
            - self.lib_w()
            - if self.panel > 0 { RIGHT_W + 8.0 } else { 0.0 }
    }

    pub fn grid(&self) -> (usize, f32) {
        let w = self.main_w() - 24.0;
        let n = ((w / 180.0) as usize).max(2);
        (n, w / n as f32)
    }

    fn songs_of(&self, key: &str) -> Option<(Vec<Song>, String)> {
        let (s, name) = song_list(&self.cache.get(key)?.1)?;
        Some((s.to_vec(), if key == "liked" { "Liked Songs" } else { name }.to_string()))
    }

    fn edit<T: Clone + Send + Sync + 'static>(&mut self, key: &str, f: impl FnOnce(&mut T)) {
        if let Some((t, v)) = self.cache.get(key)
            && let Some(x) = v.downcast_ref::<T>()
        {
            let (t, mut x) = (*t, x.clone());
            f(&mut x);
            self.cache.insert(key.to_string(), (t, Arc::new(x)));
        }
    }

    fn find_song(&self, id: &str) -> Option<Song> {
        self.cache
            .values()
            .filter_map(|(_, v)| song_list(v))
            .find_map(|(l, _)| l.iter().find(|s| s.id == id))
            .or_else(|| self.queue.iter().find(|s| s.id == id))
            .cloned()
    }

    fn find_album(&self, id: &str) -> Option<Album> {
        self.cache.values().find_map(|(_, v)| {
            [
                v.downcast_ref::<Pager<Album>>().map(|p| &p.items[..]),
                v.downcast_ref::<SearchRes>().map(|r| &r.albums[..]),
                v.downcast_ref::<(Artist, Vec<Album>, Vec<Song>, Info)>().map(|x| &x.1[..]),
                v.downcast_ref::<(Album, Vec<Song>)>().map(|x| std::slice::from_ref(&x.0)),
            ]
            .into_iter()
            .flatten()
            .chain(
                v.downcast_ref::<Home>()
                    .into_iter()
                    .flat_map(|h| [&h.recent[..], &h.added[..], &h.frequent[..], &h.random[..]]),
            )
            .flatten()
            .find(|a| a.id == id)
            .cloned()
        })
    }

    fn find_artist(&self, id: &str) -> Option<Artist> {
        self.cache.values().find_map(|(_, v)| {
            let a = v.downcast_ref::<(Artist, Vec<Album>, Vec<Song>, Info)>();
            [
                v.downcast_ref::<Pager<Artist>>().map(|p| &p.items[..]),
                v.downcast_ref::<SearchRes>().map(|r| &r.artists[..]),
                a.map(|x| std::slice::from_ref(&x.0)),
                a.map(|x| &x.3.similar[..]),
            ]
            .into_iter()
            .flatten()
            .flatten()
            .find(|a| a.id == id)
            .cloned()
        })
    }

    fn apply_star(&mut self, id: &str, on: bool) {
        if let Some(mut s) = self.find_song(id) {
            s.starred = on;
            self.edit::<Pager<Song>>("liked", |p| {
                let had = p.items.iter().any(|x| x.id == s.id);
                p.items.retain(|x| x.id != s.id);
                if on {
                    p.items.insert(0, s);
                }
                p.total = (p.total + on as usize).saturating_sub(had as usize);
            });
        } else if let Some(a) = self.find_album(id) {
            self.edit::<Lib>("lib", |l| {
                l.albums.retain(|x| x.id != a.id);
                l.albums.extend(on.then_some(a));
            });
        } else if let Some(a) = self.find_artist(id) {
            self.edit::<Lib>("lib", |l| {
                l.artists.retain(|x| x.id != a.id);
                l.artists.extend(on.then_some(a));
            });
        }
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
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *RX.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
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
            player: tokio::runtime::Handle::try_current().ok().map(|h| {
                let (p, mut rx) = navify_audio::Player::new(h.clone());
                let tx = tx.clone();
                h.spawn(async move {
                    while let Some(e) = rx.recv().await {
                        let _ = tx.send(Message::Audio(e));
                    }
                });
                p
            }),
            media: None,
            discord: None,
            os: Default::default(),
            tx,
            queue: Vec::new(),
            index: 0,
            shuffle: None,
            source: String::new(),
            playing: false,
            position: 0.0,
            scrobbled: false,
            loaded: false,
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
            muted: false,
            login: Login::default(),
            visible: true,
            modal: None,
            new_pl_desc: String::new(),
            lib_collapsed: shot.as_deref().is_some_and(|s| s.contains("collapsed")),
            shot,
            new_pl: String::new(),
            new_pl_ids: Vec::new(),
            sel: None,
            hover: None,
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
        if self.cache.get(key).is_some_and(|(t, v)| t.elapsed() < STALE && !v.is::<String>()) {
            return Task::none();
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

    fn play_index(&mut self, i: usize, at: f32) -> Task<Message> {
        if self.queue.is_empty() {
            return Task::none();
        }
        let done = if self.loaded { self.scrobble_due(self.pos()) } else { Task::none() };
        self.index = i.min(self.queue.len() - 1);
        self.playing = true;
        let t = self.track(&self.queue[self.real_index()]);
        if let Some(p) = &self.player {
            p.set_volume(if self.muted { 0.0 } else { self.volume });
            p.set_speed(self.cfg.settings.speed);
            p.set_crossfade(self.cfg.settings.crossfade_secs);
            p.play(t, at);
        }
        let began = self.began();
        self.position = at;
        Task::batch([done, began])
    }

    fn began(&mut self) -> Task<Message> {
        (self.position, self.scrobbled, self.loaded) = (0.0, false, true);
        self.queue_next_refresh();
        let Some(song) = self.current_song().cloned() else { return Task::none() };
        let lyr = if self.route == "lyrics" {
            self.request(&format!("lyrics:{}", song.id))
        } else {
            Task::none()
        };
        let Some(s) = self.session.clone() else { return lyr };
        Task::batch([
            lyr,
            Task::perform(async move { s.scrobble(&song.id, false).await }, |_| Message::Noop),
            self.debounce_save_queue(),
        ])
    }

    fn pos(&self) -> f32 {
        self.player.as_ref().filter(|_| self.loaded).map_or(self.position, |p| p.position())
    }

    fn scrobble_due(&mut self, pos: f32) -> Task<Message> {
        let dur = self.current_dur();
        if self.scrobbled || dur <= 0.0 || (pos < dur * 0.5 && pos < 240.0) {
            return Task::none();
        }
        self.scrobbled = true;
        match (self.session.clone(), self.current_song().map(|s| s.id.clone())) {
            (Some(s), Some(id)) => {
                Task::perform(async move { s.scrobble(&id, true).await }, |_| Message::Noop)
            }
            _ => Task::none(),
        }
    }

    fn restore(&self) -> Task<Message> {
        let Some(s) =
            self.session.clone().filter(|_| self.cfg.settings.queue_sync && self.shot.is_none())
        else {
            return Task::none();
        };
        Task::perform(async move { s.play_queue().await.ok() }, |r| {
            r.map_or(Message::Noop, |(songs, cur, pos)| Message::RestoreQueue(songs, cur, pos))
        })
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

use navify_audio::Track;

async fn decode(b: &[u8], size: u32) -> Option<(u32, u32, Color, Vec<u8>)> {
    let img = ::image::load_from_memory(b).ok()?;
    let img = if img.width() > size || img.height() > size {
        img.resize(size, size, ::image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let (w, h) = (img.width(), img.height());
    let px = img.to_rgba8().into_raw();
    let c = dominant(&img);
    Some((w, h, c, px))
}

fn dominant(img: &::image::DynamicImage) -> Color {
    let small = img.resize_exact(24, 24, ::image::imageops::FilterType::Triangle).to_rgb8();
    let mut hue = [[0f32; 4]; 12];
    let mut all = [0f32; 3];
    for p in small.pixels() {
        let [r, g, b] = p.0.map(|c| c as f32 / 255.0);
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        let d = mx - mn;
        let h = match () {
            _ if d == 0.0 => 0.0,
            _ if mx == r => ((g - b) / d).rem_euclid(6.0),
            _ if mx == g => (b - r) / d + 2.0,
            _ => (r - g) / d + 4.0,
        };
        let w = if mx > 0.0 { d / mx } else { 0.0 } * mx * (1.0 - (mx - 0.6).abs());
        let k = &mut hue[(h * 2.0) as usize % 12];
        *k = [k[0] + w, k[1] + r * w, k[2] + g * w, k[3] + b * w];
        all = [all[0] + r, all[1] + g, all[2] + b];
    }
    let [w, r, g, b] = hue.into_iter().max_by(|a, b| a[0].total_cmp(&b[0])).unwrap_or_default();
    if w < 576.0 * 0.03 {
        let l = ((all[0] * 0.3 + all[1] * 0.59 + all[2] * 0.11) / 576.0).clamp(0.15, 0.35);
        return Color::from_rgb(l, l, l);
    }
    let [r, g, b] = [r / w, g / w, b / w];
    let k = 0.5 / r.max(g).max(b).max(0.05);
    Color::from_rgb((r * k).min(1.0), (g * k).min(1.0), (b * k).min(1.0))
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
                    let (r, a, f, x) = tokio::try_join!(
                        s.albums("recent", &[], 0, 12),
                        s.albums("newest", &[], 0, 12),
                        s.albums("frequent", &[], 0, 12),
                        s.albums("random", &[], 0, 12),
                    )?;
                    Arc::new(Home { recent: r.0, added: a.0, frequent: f.0, random: x.0 })
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
    fn send<F>(&self, f: impl FnOnce(Session) -> F, fail: &'static str) -> Task<Message>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let Some(s) = self.session.clone() else { return Task::none() };
        Task::perform(f(s), move |r| match r {
            Ok(()) => Message::Noop,
            Err(e) => Message::Toast(0, format!("{fail} {e}")),
        })
    }

    fn nav(&mut self, route: String) -> Task<Message> {
        if route == self.route && !route.starts_with("search") {
            return Task::none();
        }
        self.history.truncate(self.hi + 1);
        self.history.push(route);
        self.hi = self.history.len() - 1;
        self.show()
    }

    fn show(&mut self) -> Task<Message> {
        let route = self.history[self.hi].clone();
        self.route = route.clone();
        self.menu = None;
        self.hover = None;
        self.popular_all = false;
        let key = match (route.as_str(), self.current_song()) {
            ("settings" | "login" | "search" | "genres", _) => "genres".to_string(),
            ("lyrics", Some(s)) => format!("lyrics:{}", s.id),
            ("lyrics", None) => "lib".to_string(),
            (r, _) => r.to_string(),
        };
        let mut tasks = vec![self.request(&key), self.request("lib")];
        let ids = self.visible_covers();
        tasks.push(self.want_covers(ids));
        let target = self.scrolls.get(&route).map(|(o, _)| *o).unwrap_or(0.0);
        tasks.push(iced::widget::operation::scroll_to(
            crate::view::SCROLL,
            iced::widget::operation::AbsoluteOffset::<Option<f32>> { x: None, y: Some(target) },
        ));
        Task::batch(tasks)
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        let seek = matches!(m, Message::Seek(_));
        let t = self.handle(m);
        if self.cfg.settings.discord != self.discord.is_some() {
            self.discord = self.cfg.settings.discord.then(|| {
                let (tx, rx) = std::sync::mpsc::channel::<Option<(String, String, i64)>>();
                std::thread::spawn(move || {
                    use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
                    let mut c: Option<DiscordIpcClient> = None;
                    for msg in rx {
                        if c.is_none() {
                            let mut n = DiscordIpcClient::new(DISCORD_ID);
                            c = n.connect().ok().map(|_| n);
                        }
                        let Some(cl) = &mut c else { continue };
                        let r = match msg {
                            Some((title, artist, start)) => cl.set_activity(
                                activity::Activity::new()
                                    .activity_type(activity::ActivityType::Listening)
                                    .details(title)
                                    .state(artist)
                                    .timestamps(activity::Timestamps::new().start(start)),
                            ),
                            None => cl.clear_activity(),
                        };
                        if r.is_err() {
                            c = None;
                        }
                    }
                    if let Some(mut c) = c {
                        let _ = c.close();
                    }
                });
                tx
            });
            self.os = Default::default();
        }
        let key = (self.current_song().map_or(String::new(), |s| s.id.clone()), self.playing);
        if key == self.os && !seek {
            return t;
        }
        self.os = key;
        let song = self.current_song().cloned();
        let pos = self.pos();
        if let Some(m) = &mut self.media {
            let art = song
                .as_ref()
                .zip(self.session.as_ref())
                .map(|(s, x)| x.cover_file(&s.cover, 300))
                .filter(|f| f.exists())
                .map(|f| format!("file://{}", f.display()));
            let _ = m.set_metadata(souvlaki::MediaMetadata {
                title: song.as_ref().map(|s| s.title.as_str()),
                album: song.as_ref().map(|s| s.album.as_str()),
                artist: song.as_ref().map(|s| s.artist.as_str()),
                cover_url: art.as_deref(),
                duration: song.as_ref().map(|s| Duration::from_secs_f32(s.duration.max(0.0))),
            });
            let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f32(pos.max(0.0))));
            let _ = m.set_playback(match (&song, self.playing) {
                (None, _) => souvlaki::MediaPlayback::Stopped,
                (_, true) => souvlaki::MediaPlayback::Playing { progress },
                _ => souvlaki::MediaPlayback::Paused { progress },
            });
        }
        if let Some(d) = &self.discord {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            let _ = d
                .send(song.filter(|_| self.playing).map(|s| (s.title, s.artist, now - pos as i64)));
        }
        t
    }

    fn handle(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Loaded(key, r) => {
                self.pending.retain(|p| p != &key && !p.starts_with(&format!("{key}#")));
                let v = match r {
                    Ok(v) => v,
                    Err(e) => {
                        if self.shot.is_some() {
                            eprintln!("LOAD-ERR {key} {e}");
                        }
                        if self.cache.contains_key(&key) || key != self.route {
                            return self.toast(&e);
                        }
                        self.cache.insert(key, (Instant::now(), Arc::new(e)));
                        return Task::none();
                    }
                };
                let v = self.merge(&key, v);
                self.cache.insert(key.clone(), (Instant::now(), v));
                let setup = if shot_key(self.shot.as_deref()).as_deref() == Some(key.as_str()) {
                    self.apply_shot_setup()
                } else {
                    Task::none()
                };
                let ids = self.visible_covers();
                Task::batch([self.want_covers(ids), setup])
            }
            Message::Cover(id, size, r) => {
                self.covers.want.remove(&(id.clone(), size));
                if let Some((w, h, c, px)) = r {
                    self.covers.insert((id, size), w, h, c, px);
                }
                Task::none()
            }
            Message::Nav(r) => self.nav(r),
            Message::Back if self.hi > 0 => {
                self.hi -= 1;
                self.show()
            }
            Message::Fwd if self.hi + 1 < self.history.len() => {
                self.hi += 1;
                self.show()
            }
            Message::Back | Message::Fwd => Task::none(),
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
                self.play_index(i, 0.0)
            }
            Message::PlayQueue(i) => self.play_index(i, 0.0),
            Message::Toggle => {
                if !self.loaded {
                    return self.play_index(self.index, self.position);
                }
                self.playing = !self.playing;
                if let Some(p) = &self.player {
                    p.set_paused(!self.playing);
                }
                if self.playing {
                    return Task::none();
                }
                self.position = self.pos();
                self.save_seq += 1;
                self.update(Message::SaveQ(self.save_seq))
            }
            Message::Prev => {
                if self.pos() > 3.0 {
                    return self.update(Message::Seek(0.0));
                }
                if self.index > 0 {
                    return self.play_index(self.index - 1, 0.0);
                }
                Task::none()
            }
            Message::Next(manual) => {
                if self.repeat == 2 && !manual {
                    return self.play_index(self.index, 0.0);
                }
                match self.next_index() {
                    Some(n) => self.play_index(n, 0.0),
                    None => {
                        (self.playing, self.loaded) = (false, false);
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
                self.queue_next_refresh();
                self.debounce_save_queue()
            }
            Message::Repeat => {
                self.repeat = (self.repeat + 1) % 3;
                self.queue_next_refresh();
                Task::none()
            }
            Message::Vol(v) => {
                self.volume = v;
                self.cfg.settings.volume = v;
                self.muted = false;
                if let Some(p) = &self.player {
                    p.set_volume(v);
                }
                Task::none()
            }
            Message::Mute => {
                self.muted = !self.muted;
                if let Some(p) = &self.player {
                    p.set_volume(if self.muted { 0.0 } else { self.volume });
                }
                Task::none()
            }
            Message::Star(id, on) => {
                self.starred.insert(id.clone(), on);
                self.apply_star(&id, on);
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(
                    async move {
                        let r = s.star(std::slice::from_ref(&id), on).await;
                        (id, r.map_err(|e| e.to_string()))
                    },
                    move |(id, r)| match r {
                        Ok(()) => Message::Noop,
                        Err(e) => Message::StarFailed(id, on, e),
                    },
                )
            }
            Message::StarFailed(id, on, e) => {
                self.starred.insert(id.clone(), !on);
                self.apply_star(&id, !on);
                self.toast(&format!("Could not update Your Library. {e}"))
            }
            Message::Rate(id, n) => {
                self.ratings.insert(id.clone(), n);
                self.menu = None;
                self.send(|s| async move { s.rate(&id, n).await }, "Could not set the rating.")
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
                (self.playing, self.loaded) = (false, false);
                if let Some(p) = &self.player {
                    p.stop();
                }
                self.debounce_save_queue()
            }
            Message::Menu(kind) => {
                let bits = CURSOR.load(std::sync::atomic::Ordering::Relaxed);
                let (x, y) = (f32::from_bits((bits >> 32) as u32), f32::from_bits(bits as u32));
                self.menu = Some(Menu { x, y, kind: *kind, sub: 0 });
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
                self.modal = None;
                Task::none()
            }
            Message::PlAdd(pid, ids) => {
                self.menu = None;
                let songs: Vec<Song> = ids.iter().filter_map(|id| self.find_song(id)).collect();
                let n = songs.len() as u32;
                self.edit::<(Playlist, Vec<Song>)>(&format!("playlist:{pid}"), |(_, s)| {
                    s.extend(songs)
                });
                self.edit::<Lib>("lib", |l| {
                    l.playlists.iter_mut().filter(|p| p.id == pid).for_each(|p| p.song_count += n)
                });
                let name = self
                    .data::<Lib>("lib")
                    .and_then(|l| l.playlists.iter().find(|p| p.id == pid))
                    .map_or(String::new(), |p| p.name.clone());
                Task::batch([
                    self.send(
                        |s| async move { s.playlist_update(&pid, None, None, &ids, &[]).await },
                        "Could not add the song to the playlist.",
                    ),
                    self.toast(&format!("Added to {name}.")),
                ])
            }
            Message::PlNew(name) => {
                self.menu = None;
                self.modal = None;
                let ids = std::mem::take(&mut self.new_pl_ids);
                let Some(s) = self.session.clone() else { return Task::none() };
                Task::perform(async move { s.playlist_create(&name, &ids).await }, |r| match r {
                    Ok(p) => Message::PlCreated(p),
                    Err(e) => Message::Toast(0, format!("Could not make the playlist. {e}")),
                })
            }
            Message::PlDel(id) => {
                self.menu = None;
                self.modal = None;
                let r = format!("playlist:{id}");
                self.edit::<Lib>("lib", |l| l.playlists.retain(|p| p.id != id));
                self.cache.remove(&r);
                let back = if self.route == r { self.nav("home".into()) } else { Task::none() };
                Task::batch([
                    back,
                    self.send(
                        |s| async move { s.playlist_delete(&id).await },
                        "Could not delete the playlist.",
                    ),
                ])
            }
            Message::PlRemove(idx) => {
                self.menu = None;
                let Some(pid) = self.route.strip_prefix("playlist:").map(str::to_string) else {
                    return Task::none();
                };
                let key = self.route.clone();
                self.edit::<(Playlist, Vec<Song>)>(&key, |(_, s)| {
                    if idx < s.len() {
                        s.remove(idx);
                    }
                });
                self.edit::<Lib>("lib", |l| {
                    l.playlists
                        .iter_mut()
                        .filter(|p| p.id == pid)
                        .for_each(|p| p.song_count = p.song_count.saturating_sub(1))
                });
                self.send(
                    |s| async move { s.playlist_update(&pid, None, None, &[], &[idx]).await },
                    "Could not remove the song from the playlist.",
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
                let ids = self.visible_covers();
                self.want_covers(ids)
            }
            Message::LibFilter(n) => {
                self.lib_filter = if self.lib_filter == n { 0 } else { n };
                Task::none()
            }
            Message::LibCollapse => {
                self.lib_collapsed = !self.lib_collapsed;
                let ids = self.visible_covers();
                self.want_covers(ids)
            }
            Message::NewPlaylist => {
                let n = self.data::<Lib>("lib").map_or(0, |l| l.playlists.len()) + 1;
                self.new_pl_ids.clear();
                self.update(Message::PlNew(format!("My Playlist #{n}")))
            }
            Message::LField(i, v) => {
                match i {
                    0 => self.login.url = v,
                    1 => self.login.user = v,
                    2 => self.login.pass = v,
                    3 => self.new_pl = v,
                    _ => self.new_pl_desc = v,
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
                        self.route.clear();
                        self.history.clear();
                        let t = self.persist();
                        let n = self
                            .nav(shot_key(self.shot.as_deref()).unwrap_or_else(|| "home".into()));
                        let q = self.restore();
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
                if self.loaded {
                    return Task::none();
                }
                self.position = pos as f32 / 1000.0;
                self.index = songs.iter().position(|s| s.id == cur).unwrap_or(0);
                (self.queue, self.shuffle) = (songs, None);
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
                (self.playing, self.loaded) = (false, false);
                if let Some(p) = &self.player {
                    p.stop();
                }
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
                    Set::Xfade(v) => {
                        self.cfg.settings.crossfade_secs = v;
                        if let Some(p) = &self.player {
                            p.set_crossfade(v);
                        }
                    }
                    Set::Trans(v) => {
                        self.cfg.settings.transcode_format =
                            ["original", "mp3", "opus", "aac"][v as usize].into()
                    }
                    Set::Bitrate(v) => self.cfg.settings.transcode_max_bitrate = v,
                    Set::Qsync(b) => self.cfg.settings.queue_sync = b,
                    Set::Discord(b) => self.cfg.settings.discord = b,
                    Set::Speed(v) => {
                        self.cfg.settings.speed = v;
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
            Message::Focus(true) => self.update(Message::Shown(true)),
            Message::Focus(false) => {
                Task::perform(tokio::time::sleep(Duration::from_millis(300)), |_| ())
                    .then(|_| iced::window::latest())
                    .and_then(iced::window::is_minimized)
                    .map(|m| Message::Shown(m != Some(true)))
            }
            Message::Shown(v) => {
                self.visible = v;
                Task::none()
            }
            Message::Close => {
                let c = self.cfg.clone();
                let s = self.session.clone();
                let q = self.cfg.settings.queue_sync.then(|| {
                    (
                        self.queue.iter().map(|x| x.id.clone()).collect::<Vec<_>>(),
                        self.current_song().map(|x| x.id.clone()).unwrap_or_default(),
                        (self.pos() * 1000.0) as u64,
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
            Message::SaveQ(n) => {
                if n != self.save_seq || !self.cfg.settings.queue_sync {
                    return Task::none();
                }
                let Some(s) = self.session.clone() else { return Task::none() };
                let ids: Vec<String> = self.queue.iter().map(|x| x.id.clone()).collect();
                let cur = self.current_song().map(|x| x.id.clone()).unwrap_or_default();
                let pos = (self.pos() * 1000.0) as u64;
                Task::perform(
                    async move {
                        let _ = s.save_play_queue(&ids, &cur, pos).await;
                    },
                    |_| Message::Noop,
                )
            }
            Message::PlayCtx(route, shuf) => {
                if let Some((v, src)) = self.songs_of(&route).filter(|(v, _)| !v.is_empty()) {
                    return self.update(Message::PlayCtxDone(v, src, shuf));
                }
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
                            "liked" => (
                                s.songs("starred", &[], 0, PAGE * 5).await?.0,
                                "Liked Songs".into(),
                            ),
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
                self.play_index(0, 0.0)
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
            Message::Hover(k, on) => {
                if on {
                    self.hover = Some(k);
                } else if self.hover.as_ref() == Some(&k) {
                    self.hover = None;
                }
                Task::none()
            }
            Message::PlayAt(key, n) => match self.songs_of(&key) {
                Some((list, src)) => {
                    let n = n - list[..n.min(list.len())].iter().filter(|s| s.is_dir).count();
                    let list = list.into_iter().filter(|s| !s.is_dir).collect();
                    self.update(Message::Play(list, n, src))
                }
                None => self.update(Message::PlayCtx(key, false)),
            },
            Message::Retry => {
                let key = match (self.route.as_str(), self.current_song()) {
                    ("lyrics", Some(s)) => format!("lyrics:{}", s.id),
                    (r, _) => r.to_string(),
                };
                self.cache.remove(&key);
                self.request(&key)
            }
            Message::FocusSearch => iced::widget::operation::focus("search"),
            Message::VolUp => self.update(Message::Vol((self.volume + 0.1).min(1.0))),
            Message::VolDown => self.update(Message::Vol((self.volume - 0.1).max(0.0))),
            Message::Resize(w, h) => {
                self.cfg.settings.window = (w, h);
                let ids = self.visible_covers();
                self.want_covers(ids)
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
                self.position = self.pos();
                self.scrobble_due(self.position)
            }
            Message::Modal(id, kind) => {
                self.new_pl_ids = match self.menu.take().map(|m| m.kind) {
                    Some(MenuKind::Song(s) | MenuKind::SongIn(s, _)) => vec![s.id],
                    _ => Vec::new(),
                };
                let p =
                    self.data::<Lib>("lib").and_then(|l| l.playlists.iter().find(|p| p.id == id));
                (self.new_pl, self.new_pl_desc) =
                    p.map_or_else(Default::default, |p| (p.name.clone(), p.comment.clone()));
                self.modal = Some((id, kind));
                Task::none()
            }
            Message::Edit(id, name, comment) => {
                self.modal = None;
                let set = |p: &mut Playlist| (p.name, p.comment) = (name.clone(), comment.clone());
                self.edit::<Lib>("lib", |l| {
                    l.playlists.iter_mut().filter(|p| p.id == id).for_each(set)
                });
                self.edit::<(Playlist, Vec<Song>)>(&format!("playlist:{id}"), |(p, _)| set(p));
                self.send(
                    |s| async move {
                        s.playlist_update(&id, Some(&name), Some(&comment), &[], &[]).await
                    },
                    "Could not change the playlist.",
                )
            }
            Message::MediaInit(hwnd) => {
                let tx = self.tx.clone();
                self.media = souvlaki::MediaControls::new(souvlaki::PlatformConfig {
                    display_name: "navify",
                    dbus_name: "navify",
                    hwnd: (hwnd != 0).then_some(hwnd as *mut std::ffi::c_void),
                })
                .ok()
                .and_then(|mut m| {
                    m.attach(move |e| drop(tx.send(Message::Media(e)))).ok().map(|_| m)
                });
                self.os = Default::default();
                Task::none()
            }
            Message::Media(e) => {
                use souvlaki::{MediaControlEvent as E, SeekDirection};
                let by = |d: SeekDirection, t: f32| {
                    Message::Seek(match d {
                        SeekDirection::Forward => self.pos() + t,
                        SeekDirection::Backward => (self.pos() - t).max(0.0),
                    })
                };
                let m = match e {
                    E::Toggle => Message::Toggle,
                    E::Play if !self.playing => Message::Toggle,
                    E::Pause | E::Stop if self.playing => Message::Toggle,
                    E::Next => Message::Next(true),
                    E::Previous => Message::Prev,
                    E::Seek(d) => by(d, 10.0),
                    E::SeekBy(d, t) => by(d, t.as_secs_f32()),
                    E::SetPosition(p) => Message::Seek(p.0.as_secs_f32()),
                    E::SetVolume(v) => Message::Vol(v.clamp(0.0, 1.0) as f32),
                    E::Quit => Message::Close,
                    E::Raise => {
                        return iced::window::latest().and_then(iced::window::gain_focus);
                    }
                    _ => return Task::none(),
                };
                self.update(m)
            }
            Message::PlCreated(p) => {
                let r = format!("playlist:{}", p.id);
                self.edit::<Lib>("lib", |l| l.playlists.insert(0, p));
                self.nav(r)
            }
            Message::Audio(e) => match e {
                navify_audio::Event::Started(id) => {
                    let done = self.scrobble_due(f32::MAX);
                    let n = if self.repeat == 2 { Some(self.index) } else { self.next_index() };
                    match n.filter(|&n| self.queue.get(self.real_at(n)).is_some_and(|s| s.id == id))
                    {
                        Some(n) => {
                            self.index = n;
                            Task::batch([done, self.began()])
                        }
                        None => done,
                    }
                }
                navify_audio::Event::Ended => self.update(Message::Ended),
                navify_audio::Event::Error(e) => {
                    (self.playing, self.loaded) = (false, false);
                    self.toast(&format!("Playback failed. {e}"))
                }
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
        if let Some(p) = &self.player {
            let n = if self.repeat == 2 { Some(self.index) } else { self.next_index() };
            p.queue_next(n.map(|n| self.track(&self.queue[self.real_at(n)])));
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

    fn visible_covers<'a>(&'a self) -> Vec<(String, u32)> {
        let (off, view) = self.scrolls.get(&self.route).copied().unwrap_or((0.0, 800.0));
        let (cols, cw) = self.grid();
        let span = |n: usize, h: f32, per: usize| {
            let lo = (off / h).max(0.0) as usize;
            (lo.saturating_sub(1) * per).min(n)..((lo + (view / h) as usize + 3) * per).min(n)
        };
        let mut out: Vec<(&str, u32)> = Vec::new();
        fn rows(v: &[Song], r: std::ops::Range<usize>) -> impl Iterator<Item = &str> {
            v[r].iter().map(|s| s.cover.as_str())
        }
        let rows = |v: &'a [Song]| rows(v, span(v.len(), crate::pages::ROW, 1));
        let key = self.route.as_str();
        if let Some((a, _)) = self.data::<(Album, Vec<Song>)>(key) {
            out.push((&a.cover, 300));
            out.push((a.artists.first().map_or(&a.artist_id, |x| &x.id), 128));
        } else if let Some((p, s)) = self.data::<(Playlist, Vec<Song>)>(key) {
            out.push((&p.cover, 300));
            out.extend(rows(s).map(|c| (c, 128)));
        } else if let Some(p) = self.data::<Pager<Song>>(key) {
            out.extend(rows(&p.items).map(|c| (c, 128)));
        } else if let Some(s) = self.data::<Vec<Song>>(key) {
            out.extend(rows(s).map(|c| (c, 128)));
        } else if let Some((a, albums, top, info)) =
            self.data::<(Artist, Vec<Album>, Vec<Song>, Info)>(key)
        {
            let mut disc: Vec<&Album> = albums.iter().collect();
            disc.sort_by_key(|x| std::cmp::Reverse(x.year));
            out.push((&a.cover, 640));
            out.extend(top.iter().take(10).map(|s| (s.cover.as_str(), 128)));
            out.extend(disc.iter().take(cols).map(|x| (x.cover.as_str(), 300)));
            out.extend(info.similar.iter().take(cols).map(|x| (x.cover.as_str(), 300)));
        } else if let Some(p) = self.data::<Pager<Album>>(key) {
            out.extend(
                p.items[span(p.items.len(), cw + 52.0, cols)]
                    .iter()
                    .map(|a| (a.cover.as_str(), 300)),
            );
        } else if let Some(p) = self.data::<Pager<Artist>>(key) {
            out.extend(
                p.items[span(p.items.len(), cw + 52.0, cols)]
                    .iter()
                    .map(|a| (a.cover.as_str(), 300)),
            );
        } else if let Some(r) = self.data::<SearchRes>(key) {
            out.extend(r.artists.iter().take(cols).map(|a| (a.cover.as_str(), 300)));
            out.extend(r.albums.iter().take(cols).map(|a| (a.cover.as_str(), 300)));
            out.extend(r.songs.iter().take(4).map(|s| (s.cover.as_str(), 128)));
        } else if let Some(h) = self.data::<Home>(key) {
            let pls = self.data::<Lib>("lib").map_or(&[][..], |l| &l.playlists[..]);
            out.extend(pls.iter().take(3).map(|p| (p.cover.as_str(), 128)));
            out.extend(h.recent.iter().take(7).map(|a| (a.cover.as_str(), 128)));
            for l in [&h.recent, &h.added, &h.frequent, &h.random] {
                out.extend(l.iter().take(cols).map(|a| (a.cover.as_str(), 300)));
            }
            out.extend(pls.iter().take(cols).map(|p| (p.cover.as_str(), 300)));
        }
        if let Some(s) = self.current_song() {
            out.extend([(s.cover.as_str(), 128), (s.cover.as_str(), 300)]);
        }
        let ahead = if self.panel == 2 { 100 } else { self.panel as usize };
        out.extend(
            (self.index + 1..self.queue.len())
                .take(ahead)
                .filter_map(|n| self.queue.get(self.real_at(n)))
                .map(|s| (s.cover.as_str(), 128)),
        );
        if let Some(l) = self.data::<Lib>("lib") {
            out.extend(l.playlists.iter().take(40).map(|p| (p.cover.as_str(), 128)));
            out.extend(l.albums.iter().take(40).map(|a| (a.cover.as_str(), 128)));
            out.extend(l.artists.iter().take(40).map(|a| (a.cover.as_str(), 128)));
        }
        out.into_iter().map(|(c, s)| (c.to_string(), s)).collect()
    }
}

fn song_list(v: &AnyData) -> Option<(&[Song], &str)> {
    v.downcast_ref::<(Album, Vec<Song>)>()
        .map(|(a, s)| (&s[..], a.name.as_str()))
        .or_else(|| {
            v.downcast_ref::<(Playlist, Vec<Song>)>().map(|(p, s)| (&s[..], p.name.as_str()))
        })
        .or_else(|| v.downcast_ref::<Pager<Song>>().map(|p| (&p.items[..], "Songs")))
        .or_else(|| {
            v.downcast_ref::<(Artist, Vec<Album>, Vec<Song>, Info)>()
                .map(|(a, _, t, _)| (&t[..], a.name.as_str()))
        })
        .or_else(|| v.downcast_ref::<SearchRes>().map(|r| (&r.songs[..], "Search")))
        .or_else(|| v.downcast_ref::<Vec<Song>>().map(|s| (&s[..], "Folder")))
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

static CURSOR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

static RX: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Message>>> =
    std::sync::Mutex::new(None);

const DISCORD_ID: &str = "0";

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
                let bits =
                    (u64::from(position.x.to_bits()) << 32) | u64::from(position.y.to_bits());
                CURSOR.store(bits, std::sync::atomic::Ordering::Relaxed);
                None
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
        {
            fn audio_stream() -> impl iced::futures::Stream<Item = Message> {
                iced::stream::channel(
                    8,
                    |mut tx: iced::futures::channel::mpsc::Sender<_>| async move {
                        let Some(mut rx) = RX.lock().ok().and_then(|mut r| r.take()) else {
                            return;
                        };
                        while let Some(m) = rx.recv().await {
                            let _ = iced::futures::SinkExt::send(&mut tx, m).await;
                        }
                    },
                )
            }
            subs.push(iced::Subscription::run(audio_stream));
        }
        iced::Subscription::batch(subs)
    }
}
