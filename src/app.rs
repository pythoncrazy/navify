use std::{
    any::Any,
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use iced::widget::operation::{self as op, AbsoluteOffset};
use iced::{Color, Task, widget::image};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::api::{Album, Artist, Info, Playlist, Session, Song};
use crate::config::{self, Config};

pub const PAGE: usize = 100;
const STALE: Duration = Duration::from_secs(300);
const COVER_CAP: usize = 64 * 1024 * 1024;
pub const ICON: &[u8] = include_bytes!("../assets/icons/hicolor/256x256/apps/navify.png");

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
    Song(Vec<Song>),
    SongIn(Vec<Song>, Vec<usize>),
    Album(Box<Album>),
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

#[derive(Clone)]
pub struct Act(Arc<dyn Fn(&mut App) + Send + Sync>);

impl std::fmt::Debug for Act {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("Act")
    }
}

pub fn act(f: impl Fn(&mut App) + Send + Sync + 'static) -> Message {
    Message::Do(Act(Arc::new(f)))
}

pub fn set<T: Clone + Send + Sync + 'static>(f: fn(&mut config::Settings, T), v: T) -> Message {
    Message::Set(Act(Arc::new(move |a| f(&mut a.cfg.settings, v.clone()))))
}

#[derive(Clone, Debug)]
pub enum Message {
    Loaded(String, Result<AnyData, String>),
    Cover(String, u32, Result<(u32, u32, Color, Vec<u8>), String>),
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
    Seek(f32),
    Shuffle,
    Repeat,
    Vol(f32),
    Mute,
    Star(Vec<String>, bool),
    Rate(String, u8),
    Queue(Vec<Song>, bool),
    QDel(usize),
    QClear,
    Menu(Box<MenuKind>),
    MenuClose,
    PlAdd(String, Vec<String>),
    PlNew(String, Vec<String>),
    PlDel(String),
    PlRemove(Vec<usize>),
    Radio(String),
    Toast(u64, String),
    Panel(u8),
    LibCollapse,
    Drag(Option<u8>),
    DragTo(f32),
    NewPlaylist,
    LField(u8, String),
    LSubmit,
    LoggedIn(Result<Session, String>),
    LogOut,
    UseServer(String),
    DropServer(String),
    Set(Act),
    Do(Act),
    ClearCache,
    Focus(bool),
    Shown(bool),
    Close,
    Noop,
    Quit,
    SaveQ(u64),
    Tick,
    Heard(u64),
    Edit(String, String, String),
    Modal(String, u8),
    StarFailed(Vec<String>, bool, String),
    PlCreated(Playlist),
    RestoreQueue(Vec<Song>, String, u64),
    NativeFailed,
    PlayCtx(String, u8),
    PlayCtxDone(Vec<Song>, String, u8),
    RowMenu(String, usize),
    SelectAll,
    DeleteSel,
    PlayAt(String, usize),
    Retry,
    DiscChip(u8),
    DiscOrder,
    FocusSearch,
    FocusMove(bool),
    Arrow(bool),
    Enter,
    VolUp,
    Faster(bool),
    Like,
    VolDown,
    Resize(f32, f32),
    SeekRelease,
    Audio(navify_audio::Event),
    Media(souvlaki::MediaControlEvent),
    MediaInit(isize),
    Window(bool),
    CloseWindow,
    Win(iced::window::Id),
}

const SIZES: [u32; 3] = [128, 300, 640];

type Slot = Option<(u64, image::Handle, usize)>;

#[derive(Debug, Default)]
pub struct Covers {
    pub map: HashMap<String, [Slot; 3]>,
    order: VecDeque<(String, usize, u64)>,
    pub bytes: usize,
    tick: u64,
    pub color: HashMap<String, Color>,
    pub want: HashSet<(String, u32)>,
}

impl Covers {
    fn slot(size: u32) -> usize {
        SIZES
            .iter()
            .position(|s| *s == size)
            .unwrap_or_else(|| panic!("cover size {size} is not a bucket"))
    }

    pub fn get(&self, id: &str, size: u32) -> Option<image::Handle> {
        let e = self.map.get(id)?;
        let mut near = [0, 1, 2];
        near.sort_unstable_by_key(|&j| SIZES[j].abs_diff(size));
        near.iter()
            .find_map(|&j| e[j].as_ref())
            .map(|(_, h, _)| h.clone())
    }

    pub fn has(&self, id: &str, size: u32) -> bool {
        self.map
            .get(id)
            .is_some_and(|e| e[Self::slot(size)].is_some())
    }

    pub fn insert(&mut self, (id, size): (String, u32), w: u32, h: u32, c: Color, px: Vec<u8>) {
        self.tick += 1;
        let (i, n) = (Self::slot(size), (w * h * 4) as usize);
        self.color.insert(id.clone(), c);
        let old = self.map.entry(id.clone()).or_default()[i].replace((
            self.tick,
            image::Handle::from_rgba(w, h, px),
            n,
        ));
        self.bytes = self.bytes + n - old.map_or(0, |o| o.2);
        self.order.push_back((id, i, self.tick));
        while self.bytes > COVER_CAP {
            let Some((id, j, t)) = self.order.pop_front() else {
                break;
            };
            let Some(e) = self.map.get_mut(&id) else {
                continue;
            };
            if e[j].as_ref().is_some_and(|x| x.0 == t) {
                self.bytes -= e[j].take().expect("the slot is full").2;
                if e.iter().all(Option::is_none) {
                    self.map.remove(&id);
                }
            }
        }
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

#[derive(Default)]
pub struct App {
    pub session: Option<Session>,
    pub cfg: Config,
    pub route: String,
    pub history: Vec<String>,
    pub hi: usize,
    pub cache: HashMap<String, (Instant, AnyData)>,
    seeds: HashMap<String, AnyData>,
    pub pending: HashSet<String>,
    pub scrolls: HashMap<String, (f32, f32)>,
    pub covers: Covers,
    pub player: Option<navify_audio::Player>,
    media: Option<souvlaki::MediaControls>,
    pub tray: Option<Box<dyn Any>>,
    win: Option<iced::window::Id>,
    os: (String, bool),
    pub queue: Vec<Song>,
    pub index: usize,
    pub shuffle: Option<Vec<usize>>,
    pub repeat: u8,
    pub source: String,
    pub ctx: String,
    pub playing: bool,
    pub position: f32,
    scrobbled: bool,
    heard: f32,
    since: Option<Instant>,
    listen_seq: u64,
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
    pub drag: Option<u8>,
    pub volume: f32,
    pub muted: bool,
    pub login: Login,
    pub hidden: bool,
    pub modal: Option<(String, u8)>,
    pub new_pl_desc: String,
    pub shot: Option<String>,
    pub new_pl: String,
    pub sel: Option<(String, usize)>,
    pub picked: BTreeSet<usize>,
    anchor: usize,
    pub hover: Option<String>,
    pub popular_all: bool,
    pub disc_chip: u8,
    pub disc_old: bool,
    pub seek_preview: Option<f32>,
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
        let prune = app.disk(Some(300 << 20));
        let open = if app.shot.is_some() {
            Task::none()
        } else {
            iced::window::open(app.window()).1.map(Message::Win)
        };
        let t = Task::batch([
            prune,
            open,
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
                app.show()
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
        let setups = self
            .shot
            .take()
            .and_then(|s| s.split_once('|').map(|(_, x)| x.to_string()));
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
                        self.menu = Some(Menu {
                            x: 480.0,
                            y: 300.0,
                            kind: MenuKind::Song(vec![sg]),
                            sub: 0,
                        });
                    }
                }
                "collapsed" => self.cfg.settings.lib_collapsed = true,
                "toast" => {
                    self.toast = Some((u64::MAX, "Added to queue".into()));
                }
                "modal0" | "modal1" | "modal2" => {
                    let k = s.as_bytes()[5] - b'0';
                    let id = self
                        .data::<Lib>("lib")
                        .and_then(|l| l.playlists.first())
                        .map_or(String::new(), |p| p.id.clone());
                    task = self.update(Message::Modal(if k == 0 { String::new() } else { id }, k));
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
        self.cache
            .get(key)
            .map(|(_, v)| v)
            .or_else(|| self.seeds.get(key))
            .and_then(|v| v.downcast_ref())
    }

    pub fn is_starred(&self, id: &str, server: bool) -> bool {
        self.starred.get(id).copied().unwrap_or(server)
    }

    pub fn scroll(&self) -> (f32, f32) {
        self.scrolls
            .get(&self.route)
            .copied()
            .unwrap_or((0.0, self.view_size().1 - 136.0))
    }

    pub fn scale(&self) -> f32 {
        self.cfg.settings.zoom
    }

    pub fn window(&self) -> iced::window::Settings {
        let icon = ::image::load_from_memory(ICON)
            .expect("the bundled icon is a PNG")
            .into_rgba8();
        let ((w, h), s) = (self.cfg.settings.window, self.scale());
        iced::window::Settings {
            size: iced::Size::new(w.max(800.0), h.max(600.0)) * (1.0 / s),
            min_size: Some(iced::Size::new(800.0, 600.0)),
            exit_on_close_request: false,
            icon: Some(
                iced::window::icon::from_rgba(icon.to_vec(), icon.width(), icon.height())
                    .expect("the bundled icon has a valid size"),
            ),
            #[cfg(target_os = "linux")]
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "navify".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn view_size(&self) -> (f32, f32) {
        let (s, (w, h)) = (self.scale(), self.cfg.settings.window);
        (w / s, h / s)
    }

    pub(crate) fn pick(&mut self, key: String, n: usize) {
        let m = iced::keyboard::Modifiers::from_bits_truncate(
            MODS.load(std::sync::atomic::Ordering::Relaxed),
        );
        let same = self.sel.as_ref().is_some_and(|s| s.0 == key);
        if same && m.shift() {
            self.picked = (self.anchor.min(n)..=self.anchor.max(n)).collect();
        } else if same && m.command() {
            if !self.picked.remove(&n) {
                self.picked.insert(n);
            }
            self.anchor = n;
        } else {
            (self.picked, self.anchor) = ([n].into(), n);
        }
        self.sel = Some((key, n));
    }

    pub fn lib_w(&self) -> f32 {
        if self.cfg.settings.lib_collapsed {
            72.0
        } else {
            self.cfg.settings.panels.0
        }
    }

    pub fn right_w(&self) -> f32 {
        self.cfg.settings.panels.1
    }

    pub fn main_w(&self) -> f32 {
        self.view_size().0
            - 24.0
            - self.lib_w()
            - if self.panel > 0 {
                self.right_w() + 8.0
            } else {
                0.0
            }
    }

    pub fn grid(&self) -> (usize, f32) {
        let w = self.main_w() - 24.0;
        let n = ((w / 180.0) as usize).max(2);
        (n, w / n as f32)
    }

    fn songs_of(&self, key: &str) -> Option<(Vec<Song>, String)> {
        if let Some(id) = key.strip_prefix("artist:") {
            let top = self.data::<Vec<Song>>(&format!("top:{id}"))?;
            return Some((top.clone(), self.find_artist(id)?.name));
        }
        let (s, name) = song_list(&self.cache.get(key)?.1)?;
        Some((
            s.to_vec(),
            if key == "liked" { "Liked Songs" } else { name }.to_string(),
        ))
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
                v.downcast_ref::<(Artist, Vec<Album>)>().map(|x| &x.1[..]),
                v.downcast_ref::<Vec<Album>>().map(|x| &x[..]),
                v.downcast_ref::<Lib>().map(|l| &l.albums[..]),
                v.downcast_ref::<(Album, Vec<Song>)>()
                    .map(|x| std::slice::from_ref(&x.0)),
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
            [
                v.downcast_ref::<Pager<Artist>>().map(|p| &p.items[..]),
                v.downcast_ref::<SearchRes>().map(|r| &r.artists[..]),
                v.downcast_ref::<(Artist, Vec<Album>)>()
                    .map(|x| std::slice::from_ref(&x.0)),
                v.downcast_ref::<Info>().map(|x| &x.similar[..]),
                v.downcast_ref::<Lib>().map(|l| &l.artists[..]),
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
        let (session, route) = cfg
            .active
            .as_ref()
            .and_then(|id| cfg.servers.iter().find(|s| &s.id == id))
            .map(|s| (Some(Session::new(s.clone())), "home".to_string()))
            .unwrap_or((None, "login".to_string()));
        let route = shot_key(shot.as_deref()).unwrap_or(route);
        Self {
            session,
            volume: cfg.settings.volume,
            history: vec![if route == "login" { "login" } else { "home" }.into()],
            route,
            player: tokio::runtime::Handle::try_current().ok().map(|h| {
                let (p, mut rx) = navify_audio::Player::new(h.clone(), Session::client());
                p.set_device(&cfg.settings.device);
                h.spawn(async move {
                    while let Some(e) = rx.recv().await {
                        let _ = CH.0.send(Message::Audio(e));
                    }
                });
                p
            }),
            shot,
            cfg,
            ..Default::default()
        }
    }

    pub fn current_song(&self) -> Option<&Song> {
        (self.index < self.queue.len()).then(|| &self.queue[self.real_index()])
    }

    pub fn real_index(&self) -> usize {
        self.real_at(self.index)
    }

    fn seed(&mut self, key: &str) {
        let Some((kind, id)) = key
            .split_once(':')
            .filter(|_| !self.cache.contains_key(key))
        else {
            return;
        };
        let v: Option<AnyData> = match kind {
            "album" => self
                .find_album(id)
                .map(|a| Arc::new((a, Vec::<Song>::new())) as AnyData),
            "artist" => self
                .find_artist(id)
                .map(|a| Arc::new((a, Vec::<Album>::new())) as AnyData),
            "playlist" => self
                .data::<Lib>("lib")
                .and_then(|l| l.playlists.iter().find(|p| p.id == id))
                .map(|p| Arc::new((p.clone(), Vec::<Song>::new())) as AnyData),
            _ => None,
        };
        if let Some(v) = v {
            self.seeds.insert(key.to_string(), v);
        }
    }

    fn request(&mut self, key: &str, offset: usize) -> Task<Message> {
        if offset == 0
            && self
                .cache
                .get(key)
                .is_some_and(|(t, v)| t.elapsed() < STALE && !v.is::<String>())
        {
            return Task::none();
        }
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
        if !self.pending.insert(format!("{key}#{offset}")) {
            return Task::none();
        }
        fetch(s, key.to_string(), offset)
    }

    fn toast(&mut self, text: &str) -> Task<Message> {
        self.toast_seq += 1;
        let n = self.toast_seq;
        self.toast = Some((n, text.to_string()));
        Task::perform(tokio::time::sleep(Duration::from_secs(3)), move |_| {
            Message::Toast(n, String::new())
        })
    }

    fn debounce_save_queue(&mut self) -> Task<Message> {
        if !self.cfg.settings.queue_sync {
            return Task::none();
        }
        self.save_seq += 1;
        let n = self.save_seq;
        Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
            Message::SaveQ(n)
        })
    }

    fn persist(&self) -> Task<Message> {
        let mut c = self.cfg.clone();
        if let Some(s) = &self.session
            && let Some(v) = c.servers.iter_mut().find(|v| v.id == s.server.id)
        {
            v.jwt = s.jwt();
        }
        Task::perform(async move { c.save() }, |r| match r {
            Ok(()) => Message::Noop,
            Err(e) => Message::Toast(0, format!("Could not save the settings. {e}")),
        })
    }

    pub fn gain(&self, song: &Song) -> f32 {
        let st = &self.cfg.settings;
        let db = match st.replaygain.as_str() {
            "track" => song.rg.track_gain.or(song.rg.album_gain),
            "album" => song.rg.album_gain.or(song.rg.track_gain),
            _ => return 1.0,
        }
        .unwrap_or(st.fallback_db);
        let peak = song
            .rg
            .track_peak
            .or(song.rg.album_peak)
            .unwrap_or(1.0)
            .max(1e-3);
        10f32.powf((db + st.preamp_db) / 20.0).min(1.0 / peak)
    }

    fn track(&self, song: &Song) -> Track {
        let st = &self.cfg.settings;
        let (url, transcoded) = self
            .session
            .as_ref()
            .expect("playback needs a session")
            .stream_url(
                song,
                Some(st.transcode_format.as_str()).filter(|f| !f.is_empty()),
                st.transcode_max_bitrate,
            );
        Track {
            id: song.id.clone(),
            url,
            gain: self.gain(song),
            transcoded,
            duration: song.duration,
        }
    }

    fn disk(&self, covers: Option<u64>) -> Task<Message> {
        let dir = crate::api::cache_dir("");
        Task::perform(
            tokio::task::spawn_blocking(move || {
                covers.inspect(|&c| crate::api::prune(&dir.join("covers"), c));
                crate::api::cache_files(&dir)
                    .iter()
                    .map(|f| f.2)
                    .sum::<u64>()
            }),
            |n| {
                Message::Loaded(
                    "disk".into(),
                    Ok(Arc::new(n.expect("cache scan")) as AnyData),
                )
            },
        )
    }

    fn next_index(&self) -> Option<usize> {
        let len = self.queue.len();
        (self.index + 1 < len)
            .then_some(self.index + 1)
            .or((self.repeat == 1 && len > 0).then_some(0))
    }

    fn start(&mut self, i: usize, shuffled: bool) -> Task<Message> {
        self.shuffle = shuffled.then(|| shuffle(0..self.queue.len(), i));
        self.play_index(if shuffled { 0 } else { i }, 0.0)
    }

    fn play_index(&mut self, i: usize, at: f32) -> Task<Message> {
        if self.queue.is_empty() {
            return Task::none();
        }
        let done = if self.loaded {
            self.listen(false)
        } else {
            Task::none()
        };
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
        Task::batch([done, began, self.listen(true)])
    }

    fn began(&mut self) -> Task<Message> {
        (self.position, self.scrobbled, self.heard, self.loaded) = (0.0, false, 0.0, true);
        self.queue_next_refresh();
        let Some(song) = self.current_song().cloned() else {
            return Task::none();
        };
        let lyr = self.request(&format!("lyrics:{}", song.id), 0);
        let now = if self.cfg.settings.scrobble {
            self.send(
                move |s| async move { s.scrobble(&song.id, false).await },
                "Could not send the playing track to the server.",
            )
        } else {
            Task::none()
        };
        Task::batch([lyr, now, self.debounce_save_queue()])
    }

    fn pos(&self) -> f32 {
        self.player
            .as_ref()
            .filter(|_| self.loaded)
            .map_or(self.position, |p| p.position())
    }

    fn listen(&mut self, on: bool) -> Task<Message> {
        let (speed, enabled) = (self.cfg.settings.speed, self.cfg.settings.scrobble);
        self.heard += self
            .since
            .take()
            .map_or(0.0, |t| t.elapsed().as_secs_f32() * speed);
        self.since = on.then(Instant::now);
        self.listen_seq += 1;
        let Some(song) = self
            .current_song()
            .filter(|s| enabled && !self.scrobbled && s.duration > 0.0)
        else {
            return Task::none();
        };
        let (id, left) = (
            song.id.clone(),
            (song.duration * 0.5).min(240.0) - self.heard,
        );
        if left <= 0.0 {
            self.scrobbled = true;
            self.send(
                move |s| async move { s.scrobble(&id, true).await },
                "Could not send the played track to the server.",
            )
        } else if on {
            let n = self.listen_seq;
            Task::perform(
                tokio::time::sleep(Duration::from_secs_f32(left / speed)),
                move |_| Message::Heard(n),
            )
        } else {
            Task::none()
        }
    }

    fn restore(&self) -> Task<Message> {
        let Some(s) = self
            .session
            .clone()
            .filter(|_| self.cfg.settings.queue_sync && self.shot.is_none())
        else {
            return Task::none();
        };
        Task::perform(async move { s.play_queue().await }, |r| match r {
            Ok((songs, cur, pos)) => Message::RestoreQueue(songs, cur, pos),
            Err(e) => Message::Toast(0, format!("Could not restore the play queue. {e}")),
        })
    }

    pub(crate) fn real_at(&self, i: usize) -> usize {
        self.shuffle.as_ref().map_or(i, |o| {
            assert_eq!(
                o.len(),
                self.queue.len(),
                "the shuffle order covers the queue"
            );
            o[i]
        })
    }

    fn covers_now(&mut self) -> Task<Message> {
        let ids = self.visible_covers();
        self.want_covers(ids)
    }

    fn want_covers(&mut self, ids: impl IntoIterator<Item = (String, u32)>) -> Task<Message> {
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
        let tasks: Vec<Task<Message>> = ids
            .into_iter()
            .filter(|(id, size)| {
                !id.is_empty()
                    && !self.covers.has(id, *size)
                    && self.covers.want.insert((id.clone(), *size))
            })
            .map(|(id, size)| {
                let (s, k) = (s.clone(), id.clone());
                Task::perform(
                    async move {
                        let b = s.cover(&k, size).await.map_err(|e| e.to_string())?;
                        tokio::task::spawn_blocking(move || decode(&b, size))
                            .await
                            .expect("decode task")
                    },
                    move |r| Message::Cover(id, size, r),
                )
            })
            .collect();
        Task::batch(tasks)
    }
}

use navify_audio::Track;

fn decode(b: &[u8], size: u32) -> Result<(u32, u32, Color, Vec<u8>), String> {
    let img = ::image::load_from_memory(b).map_err(|e| e.to_string())?;
    let img = if img.width() > size || img.height() > size {
        img.resize(size, size, ::image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    Ok((
        img.width(),
        img.height(),
        dominant(&img),
        img.to_rgba8().into_raw(),
    ))
}

fn dominant(img: &::image::DynamicImage) -> Color {
    let small = img
        .resize_exact(24, 24, ::image::imageops::FilterType::Triangle)
        .to_rgb8();
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
    let [w, r, g, b] = hue
        .into_iter()
        .max_by(|a, b| a[0].total_cmp(&b[0]))
        .expect("12 hue bins");
    if w < 576.0 * 0.03 {
        let l = ((all[0] * 0.3 + all[1] * 0.59 + all[2] * 0.11) / 576.0).clamp(0.15, 0.35);
        return Color::from_rgb(l, l, l);
    }
    let [r, g, b] = [r / w, g / w, b / w];
    let k = 0.5 / r.max(g).max(b).max(0.05);
    Color::from_rgb((r * k).min(1.0), (g * k).min(1.0), (b * k).min(1.0))
}

macro_rules! pager {
    ($v:expr, $p:ident => $e:expr) => {
        ($v.downcast_ref::<Pager<Album>>().map(|$p| $e))
            .or_else(|| $v.downcast_ref::<Pager<Artist>>().map(|$p| $e))
            .or_else(|| $v.downcast_ref::<Pager<Song>>().map(|$p| $e))
    };
}

fn pager<T: Send + Sync + 'static>((items, total): (Vec<T>, usize), offset: usize) -> AnyData {
    Arc::new(Pager {
        items,
        total,
        offset,
    })
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
                    Arc::new(Home {
                        recent: r.0,
                        added: a.0,
                        frequent: f.0,
                        random: x.0,
                    })
                }
                "lib" => {
                    let (p, st) = tokio::try_join!(s.playlists(), s.starred())?;
                    Arc::new(Lib {
                        playlists: p,
                        artists: st.0,
                        albums: st.1,
                    })
                }
                "album" => Arc::new(s.album(arg).await?),
                "artist" => Arc::new(s.artist(arg).await?),
                "top" => Arc::new(s.top_songs(arg).await?),
                "info" => Arc::new(s.artist_info(arg).await?),
                "appears" => Arc::new(s.appears(arg).await?),
                "playlist" => Arc::new(s.playlist(arg).await?),
                "liked" => pager(s.songs("starred", offset, PAGE).await?, offset),
                "albums" => {
                    let sort = if arg.is_empty() { "newest" } else { arg };
                    pager(s.albums(sort, &[], offset, PAGE).await?, offset)
                }
                "artists" => pager(s.artists(offset, PAGE).await?, offset),
                "songs" => pager(
                    s.songs(if arg.is_empty() { "name" } else { arg }, offset, PAGE)
                        .await?,
                    offset,
                ),
                "genres" => Arc::new(s.genres().await?),
                "genre" => pager(
                    s.albums("newest", &[("genre", arg)], offset, PAGE).await?,
                    offset,
                ),
                "folders" => Arc::new(s.indexes().await?),
                "folder" => Arc::new(s.directory(arg).await?),
                "lyrics" => Arc::new(s.lyrics(arg).await?),
                "search" => Arc::new({
                    let (a, b, c) = s.search(arg, 20).await?;
                    SearchRes {
                        artists: a,
                        albums: b,
                        songs: c,
                    }
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
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
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

    pub fn key_of(&self, route: &str) -> String {
        match (route.split_once(':'), self.current_song()) {
            (None, _) if matches!(route, "settings" | "login" | "search" | "genres") => {
                "genres".into()
            }
            (None, Some(s)) if route == "lyrics" => format!("lyrics:{}", s.id),
            (None, None) if route == "lyrics" => "lib".into(),
            (Some(("discography", a)), _) => format!("artist:{}", a.split(':').next().unwrap_or(a)),
            _ => route.into(),
        }
    }

    fn disc_load(&mut self) -> Task<Message> {
        let keys: Vec<String> = crate::pages::disc_view(self)
            .iter()
            .filter(|_| !self.cfg.settings.disc_grid)
            .map(|a| format!("album:{}", a.id))
            .collect();
        let covers = self.covers_now();
        Task::batch(
            keys.iter()
                .map(|k| self.request(k, 0))
                .chain([covers])
                .collect::<Vec<_>>(),
        )
    }

    fn show(&mut self) -> Task<Message> {
        let route = self.history[self.hi].clone();
        self.route = route.clone();
        (self.menu, self.hover, self.sel, self.picked) = (None, None, None, BTreeSet::new());
        (self.popular_all, self.disc_chip) = (false, 0);
        let key = self.key_of(&route);
        let parts = route.strip_prefix("artist:").map(|id| {
            [
                format!("top:{id}"),
                format!("info:{id}"),
                format!("appears:{id}"),
            ]
        });
        let parts = parts.as_ref().map_or(&[][..], |p| &p[..]);
        self.cache.retain(|k, (t, _)| {
            t.elapsed() < STALE || *k == key || k == "lib" || parts.contains(k)
        });
        self.seed(&key);
        let mut tasks = vec![self.request(&key, 0), self.request("lib", 0)];
        tasks.extend(parts.iter().map(|k| self.request(k, 0)));
        tasks.push(self.covers_now());
        tasks.push(self.disc_load());
        let target = self.scrolls.get(&route).map(|(o, _)| *o).unwrap_or(0.0);
        tasks.push(op::scroll_to(
            crate::view::SCROLL,
            AbsoluteOffset { x: 0.0, y: target },
        ));
        if route == "login" {
            let n = [&self.login.url, &self.login.user]
                .iter()
                .position(|v| v.is_empty());
            tasks.push(op::focus(format!("login{}", n.unwrap_or(2))));
        }
        if route == "settings" {
            tasks.push(Task::perform(
                tokio::task::spawn_blocking(navify_audio::devices),
                |d| {
                    Message::Loaded(
                        "devices".into(),
                        d.expect("device scan").map(|v| Arc::new(v) as AnyData),
                    )
                },
            ));
            tasks.push(self.disk(None));
        }
        Task::batch(tasks)
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        let seek = matches!(m, Message::Seek(_));
        let t = self.handle(m);
        let id = self.current_song().map_or("", |s| s.id.as_str());
        if (id, self.playing) == (self.os.0.as_str(), self.os.1) && !seek {
            return t;
        }
        self.os = (id.to_string(), self.playing);
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
                duration: song
                    .as_ref()
                    .map(|s| Duration::from_secs_f32(s.duration.max(0.0))),
            });
            let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f32(
                pos.max(0.0),
            )));
            let _ = m.set_playback(match (&song, self.playing) {
                (None, _) => souvlaki::MediaPlayback::Stopped,
                (_, true) => souvlaki::MediaPlayback::Playing { progress },
                _ => souvlaki::MediaPlayback::Paused { progress },
            });
        }
        t
    }

    fn handle(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Loaded(key, r) => {
                self.pending.retain(|p| !p.starts_with(&format!("{key}#")));
                let v = match r {
                    Ok(v) => v,
                    Err(e) => {
                        if self.shot.is_some() {
                            eprintln!("LOAD-ERR {key} {e}");
                        }
                        if self.cache.contains_key(&key)
                            || self.seeds.contains_key(&key)
                            || key != self.key_of(&self.route)
                        {
                            return self.toast(&e);
                        }
                        self.cache.insert(key, (Instant::now(), Arc::new(e)));
                        return Task::none();
                    }
                };
                self.seeds.remove(&key);
                let v = self.merge(&key, v);
                self.cache.insert(key.clone(), (Instant::now(), v));
                let setup = if shot_key(self.shot.as_deref()).as_deref() == Some(key.as_str()) {
                    self.apply_shot_setup()
                } else {
                    Task::none()
                };
                Task::batch([self.disc_load(), setup])
            }
            Message::Cover(id, size, r) => {
                self.covers.want.remove(&(id.clone(), size));
                match r {
                    Ok((w, h, c, px)) => self.covers.insert((id, size), w, h, c, px),
                    Err(e) => return self.toast(&format!("Could not load a cover. {e}")),
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
                        t = self.request(&key, len);
                    }
                }
                Task::batch([t, self.disc_load()])
            }
            Message::Search(q) => {
                self.search_q = q;
                self.search_seq += 1;
                let n = self.search_seq;
                Task::perform(tokio::time::sleep(Duration::from_millis(250)), move |_| {
                    Message::SearchGo(n)
                })
            }
            Message::SearchGo(n) => {
                if n != self.search_seq {
                    return Task::none();
                }
                let q = self.search_q.trim().to_string();
                let r = if q.is_empty() {
                    "search".into()
                } else {
                    format!("search:{q}")
                };
                if r != self.route {
                    self.nav(r)
                } else {
                    self.request(&r, 0)
                }
            }
            Message::Play(songs, i, source) => {
                self.queue = songs;
                self.source = source;
                self.ctx.clear();
                self.start(i, self.shuffle.is_some())
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
                let l = self.listen(self.playing);
                if self.playing {
                    return l;
                }
                self.position = self.pos();
                self.save_seq += 1;
                Task::batch([l, self.update(Message::SaveQ(self.save_seq))])
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
                    None => self.halt(),
                }
            }
            Message::Seek(pos) => {
                let again = pos < 5.0 && self.pos() > 10.0;
                self.position = pos;
                if let Some(p) = &self.player {
                    p.seek(pos);
                }
                if !again {
                    return Task::none();
                }
                let done = self.listen(false);
                (self.scrobbled, self.heard) = (false, 0.0);
                Task::batch([done, self.listen(self.playing)])
            }
            Message::Shuffle => {
                let cur = if self.index < self.queue.len() {
                    self.real_index()
                } else {
                    0
                };
                if self.shuffle.take().is_some() {
                    self.index = cur;
                } else {
                    self.shuffle = Some(shuffle(0..self.queue.len(), cur));
                    self.index = 0;
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
            Message::Star(ids, on) => {
                (self.menu, self.sel, self.picked) = (None, None, BTreeSet::new());
                ids.iter().for_each(|id| {
                    self.starred.insert(id.clone(), on);
                    self.apply_star(id, on);
                });
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        let r = s.star(&ids, on).await;
                        (ids, r.map_err(|e| e.to_string()))
                    },
                    move |(ids, r)| match r {
                        Ok(()) => Message::Noop,
                        Err(e) => Message::StarFailed(ids, on, e),
                    },
                )
            }
            Message::StarFailed(ids, on, e) => {
                ids.iter().for_each(|id| {
                    self.starred.insert(id.clone(), !on);
                    self.apply_star(id, !on);
                });
                self.toast(&format!("Could not update Your Library. {e}"))
            }
            Message::Rate(id, n) => {
                self.ratings.insert(id.clone(), n);
                self.menu = None;
                self.send(
                    |s| async move { s.rate(&id, n).await },
                    "Could not set the rating.",
                )
            }
            Message::Queue(songs, next) => {
                let (n, at) = (songs.len(), if next { self.index + 1 } else { usize::MAX });
                if let Some(o) = &mut self.shuffle {
                    let base = self.queue.len();
                    self.queue.extend(songs);
                    let at = at.min(o.len());
                    o.splice(at..at, shuffle(base..base + n, usize::MAX));
                } else {
                    let at = at.min(self.queue.len());
                    if at <= self.index && !self.queue.is_empty() {
                        self.index += n;
                    }
                    self.queue.splice(at..at, songs);
                }
                self.queue_next_refresh();
                Task::batch([self.debounce_save_queue(), self.toast("Added to queue")])
            }
            Message::QDel(i) => {
                let ri = self.real_at(i);
                if ri < self.queue.len() && i != self.index {
                    self.queue.remove(ri);
                    if let Some(o) = &mut self.shuffle {
                        o.retain(|&x| x != ri);
                        o.iter_mut().filter(|x| **x > ri).for_each(|x| *x -= 1);
                    }
                    if i < self.index {
                        self.index -= 1;
                    }
                }
                self.queue_next_refresh();
                self.debounce_save_queue()
            }
            Message::QClear => {
                let done = self.halt();
                self.queue.clear();
                self.index = 0;
                self.shuffle = self.shuffle.as_ref().map(|_| Vec::new());
                Task::batch([done, self.debounce_save_queue()])
            }
            Message::Menu(kind) => {
                let bits = CURSOR.load(std::sync::atomic::Ordering::Relaxed);
                let (x, y) = (
                    f32::from_bits((bits >> 32) as u32),
                    f32::from_bits(bits as u32),
                );
                let speed = matches!(*kind, MenuKind::Speed);
                self.menu = Some(Menu {
                    x,
                    y,
                    kind: *kind,
                    sub: 0,
                });
                if !speed {
                    return Task::none();
                }
                let row = (self.cfg.settings.speed * 10.0).round() - 5.0;
                let y = (row * 40.0 - (crate::pages::SPEED_H - 88.0) / 2.0).max(0.0);
                op::scroll_to("speed", AbsoluteOffset { x: 0.0, y })
            }
            Message::MenuClose => {
                if self.menu.is_none() && self.modal.is_none() {
                    (self.sel, self.picked) = (None, BTreeSet::new());
                }
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
                    l.playlists
                        .iter_mut()
                        .filter(|p| p.id == pid)
                        .for_each(|p| p.song_count += n)
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
            Message::PlNew(name, ids) => {
                self.menu = None;
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
                Task::perform(
                    async move { s.playlist_create(&name, &ids).await },
                    |r| match r {
                        Ok(p) => Message::PlCreated(p),
                        Err(e) => Message::Toast(0, format!("Could not make the playlist. {e}")),
                    },
                )
            }
            Message::PlDel(id) => {
                self.menu = None;
                self.modal = None;
                let r = format!("playlist:{id}");
                self.edit::<Lib>("lib", |l| l.playlists.retain(|p| p.id != id));
                self.cache.remove(&r);
                let back = if self.route == r {
                    self.nav("home".into())
                } else {
                    Task::none()
                };
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
                (self.sel, self.picked) = (None, BTreeSet::new());
                self.edit::<(Playlist, Vec<Song>)>(&key, |(_, s)| {
                    let mut n = 0;
                    s.retain(|_| (!idx.contains(&n), n += 1).0);
                });
                let k = idx.len() as u32;
                self.edit::<Lib>("lib", |l| {
                    l.playlists
                        .iter_mut()
                        .filter(|p| p.id == pid)
                        .for_each(|p| p.song_count = p.song_count.saturating_sub(k))
                });
                self.send(
                    |s| async move { s.playlist_update(&pid, None, None, &[], &idx).await },
                    "Could not remove the songs from the playlist.",
                )
            }
            Message::Radio(id) => {
                self.menu = None;
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
                Task::perform(
                    async move { s.similar_songs(&id, 30).await.map_err(|e| e.to_string()) },
                    |r| match r {
                        Ok(v) if !v.is_empty() => Message::Play(v, 0, "Radio".into()),
                        Ok(_) => Message::Toast(0, "No similar songs were found.".into()),
                        Err(e) => Message::Toast(0, format!("Could not start the radio. {e}")),
                    },
                )
            }
            Message::Toast(n, text) if text.is_empty() => {
                if self
                    .toast
                    .as_ref()
                    .is_some_and(|(tn, _)| *tn == n || n == 0)
                {
                    self.toast = None;
                }
                Task::none()
            }
            Message::Toast(_, text) => self.toast(&text),
            Message::Panel(n) => {
                self.panel = if self.panel == n { 0 } else { n };
                self.covers_now()
            }
            Message::Drag(d) => {
                self.drag = d;
                if d.is_some() {
                    return Task::none();
                }
                Task::batch([self.persist(), self.covers_now()])
            }
            Message::DragTo(x) => {
                let vw = self.view_size().0;
                let p = &mut self.cfg.settings.panels;
                match self.drag {
                    Some(0) if x < 208.0 => self.cfg.settings.lib_collapsed = true,
                    Some(0) => {
                        p.0 = (x - 8.0).clamp(280.0, 480.0);
                        self.cfg.settings.lib_collapsed = false;
                    }
                    _ => p.1 = (vw - 8.0 - x).clamp(280.0, 420.0),
                }
                Task::none()
            }
            Message::LibCollapse => {
                self.cfg.settings.lib_collapsed = !self.cfg.settings.lib_collapsed;
                self.covers_now()
            }
            Message::NewPlaylist => {
                let n = self.data::<Lib>("lib").map_or(0, |l| l.playlists.len()) + 1;
                self.update(Message::PlNew(format!("My Playlist #{n}"), Vec::new()))
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
            Message::LSubmit => {
                if self.login.busy {
                    return Task::none();
                }
                if let Some(n) = [&self.login.url, &self.login.user]
                    .iter()
                    .position(|v| v.trim().is_empty())
                {
                    self.login.error =
                        ["Enter the server address.", "Enter the username."][n].into();
                    return op::focus(format!("login{n}"));
                }
                self.login.busy = true;
                self.login.error.clear();
                let (u, n, p) = (
                    self.login.url.clone(),
                    self.login.user.clone(),
                    self.login.pass.clone(),
                );
                Task::perform(
                    async move { Session::login(&u, &n, &p).await.map_err(|e| e.to_string()) },
                    Message::LoggedIn,
                )
            }
            Message::LoggedIn(r) => {
                self.login.busy = false;
                match r {
                    Ok(s) => {
                        let kept = match self.login.remember {
                            true => config::remember(&s.server.id, &self.login.pass),
                            false => Ok(()),
                        };
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
                        let k = kept.map_or_else(|e| self.toast(&e.to_string()), |()| Task::none());
                        Task::batch([t, n, q, k])
                    }
                    Err(e) => {
                        self.login.error = e;
                        Task::none()
                    }
                }
            }
            Message::RestoreQueue(songs, cur, pos) => {
                if songs.is_empty() || self.loaded {
                    return Task::none();
                }
                self.position = pos as f32 / 1000.0;
                self.index = songs.iter().position(|s| s.id == cur).unwrap_or(0);
                (self.queue, self.shuffle) = (songs, None);
                Task::none()
            }
            Message::LogOut => {
                let id = self.cfg.active.take();
                let gone = id.map_or(Ok(()), |id| {
                    self.cfg.servers.retain(|s| s.id != id);
                    config::forget(&id)
                });
                let h = self.halt();
                self.session = None;
                self.queue.clear();
                self.cache.clear();
                let t = self.persist();
                let n = self.nav("login".into());
                let g = gone.map_or_else(|e| self.toast(&e.to_string()), |()| Task::none());
                Task::batch([h, t, n, g])
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
                    |ok| {
                        if ok {
                            Message::Noop
                        } else {
                            Message::NativeFailed
                        }
                    },
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
                let gone = config::forget(&id)
                    .map_or_else(|e| self.toast(&e.to_string()), |()| Task::none());
                Task::batch([self.persist(), gone])
            }
            Message::Do(Act(f)) => {
                f(self);
                Task::none()
            }
            Message::Set(Act(f)) => {
                self.menu = None;
                let o = self.cfg.settings.clone();
                f(self);
                let n = &self.cfg.settings;
                if let Some(p) = &self.player {
                    (o.crossfade_secs != n.crossfade_secs)
                        .then(|| p.set_crossfade(n.crossfade_secs));
                    (o.speed != n.speed).then(|| p.set_speed(n.speed));
                    (o.device != n.device).then(|| p.set_device(&n.device));
                }
                let gain = |s: &config::Settings| {
                    (
                        s.replaygain.clone(),
                        s.preamp_db,
                        s.fallback_db,
                        s.transcode_format.clone(),
                        s.transcode_max_bitrate,
                    )
                };
                let audio = gain(&o) != gain(n);
                let heard = self.loaded && (o.speed, o.scrobble) != (n.speed, n.scrobble);
                let (grid, zoom) = (o.disc_grid != n.disc_grid, o.zoom != n.zoom);
                if audio && let (Some(p), Some(s)) = (&self.player, self.current_song()) {
                    p.set_gain(self.gain(s));
                    self.queue_next_refresh();
                }
                let (w, h) = self.view_size();
                Task::batch([
                    if grid { self.disc_load() } else { Task::none() },
                    if zoom {
                        self.update(Message::Resize(w, h))
                    } else {
                        Task::none()
                    },
                    if heard {
                        self.listen(self.playing)
                    } else {
                        Task::none()
                    },
                    self.persist(),
                ])
            }
            Message::ClearCache => {
                self.covers = Covers {
                    want: std::mem::take(&mut self.covers.want),
                    ..Default::default()
                };
                Task::perform(
                    tokio::fs::remove_dir_all(crate::api::cache_dir("")),
                    |r| match r {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                            Message::Toast(0, format!("Could not clear the cache. {e}"))
                        }
                        _ => Message::Noop,
                    },
                )
                .chain(self.disk(None))
            }
            Message::Focus(true) => self.update(Message::Shown(true)),
            Message::Focus(false) => {
                Task::perform(tokio::time::sleep(Duration::from_millis(300)), |_| ())
                    .then(|_| iced::window::latest())
                    .and_then(iced::window::is_minimized)
                    .map(|m| Message::Shown(m != Some(true)))
            }
            Message::Shown(v) => {
                self.hidden = !v;
                Task::none()
            }
            Message::Close => {
                Task::batch([self.persist(), self.update(Message::SaveQ(self.save_seq))])
                    .collect()
                    .map(|_| Message::Quit)
            }
            Message::CloseWindow => self.update(if self.tray.is_some() && self.cfg.settings.tray {
                Message::Window(false)
            } else {
                Message::Close
            }),
            Message::Window(false) if self.tray.is_none() => self
                .win
                .map_or(Task::none(), |id| iced::window::minimize(id, true)),
            Message::Window(show) => {
                let Some(id) = self.win else {
                    return iced::window::open(self.window()).1.map(Message::Win);
                };
                if show {
                    return iced::window::set_mode(id, iced::window::Mode::Windowed)
                        .chain(iced::window::gain_focus(id));
                }
                self.hidden = true;
                Task::batch([
                    self.persist(),
                    self.update(Message::SaveQ(self.save_seq)),
                    if cfg!(windows) {
                        iced::window::set_mode(id, iced::window::Mode::Hidden)
                    } else {
                        self.win = None;
                        iced::window::close(id)
                    },
                ])
            }
            Message::Win(id) => {
                self.win = Some(id);
                if self.tray.is_none() {
                    self.tray = crate::tray::spawn(CH.0.clone())
                        .inspect_err(|e| eprintln!("The tray icon is not available. {e}"))
                        .ok();
                }
                let y = self.scrolls.get(&self.route).map_or(0.0, |(o, _)| *o);
                let scroll = op::scroll_to(crate::view::SCROLL, AbsoluteOffset { x: 0.0, y });
                if self.media.is_some() {
                    return scroll;
                }
                scroll.chain(
                    iced::window::run(id, |w| {
                        use iced::window::raw_window_handle::RawWindowHandle;
                        match w.window_handle().map(|h| h.as_raw()) {
                            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
                            _ => 0,
                        }
                    })
                    .map(Message::MediaInit),
                )
            }
            Message::Quit => iced::exit(),
            Message::Noop => Task::none(),
            Message::SaveQ(n) => {
                if n != self.save_seq || !self.cfg.settings.queue_sync {
                    return Task::none();
                }
                let ids: Vec<String> = self.queue.iter().map(|x| x.id.clone()).collect();
                let cur = self
                    .current_song()
                    .map(|x| x.id.clone())
                    .unwrap_or_default();
                let pos = (self.pos() * 1000.0) as u64;
                self.send(
                    |s| async move { s.save_play_queue(&ids, &cur, pos).await },
                    "Could not save the play queue.",
                )
            }
            Message::PlayCtx(route, mode) => {
                self.menu = None;
                if mode < 2 {
                    self.ctx = route.clone();
                }
                if let Some((v, src)) = self.songs_of(&route).filter(|(v, _)| !v.is_empty()) {
                    return self.update(Message::PlayCtxDone(v, src, mode));
                }
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
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
                                let ((a, _), t) =
                                    tokio::try_join!(s.artist(&id), s.top_songs(&id))?;
                                (t, a.name)
                            }
                            "liked" => (
                                s.songs("starred", 0, PAGE * 5).await?.0,
                                "Liked Songs".into(),
                            ),
                            "songs" => (
                                s.songs(if id.is_empty() { "name" } else { &id }, 0, PAGE * 5)
                                    .await?
                                    .0,
                                "Songs".into(),
                            ),
                            _ => (Vec::new(), String::new()),
                        };
                        Ok::<_, anyhow::Error>((songs, src))
                    },
                    move |r: Result<(Vec<Song>, String), anyhow::Error>| match r {
                        Ok((v, src)) if !v.is_empty() => Message::PlayCtxDone(v, src, mode),
                        Ok(_) => Message::Toast(0, "Nothing to play.".into()),
                        Err(e) => Message::Toast(0, format!("Could not play. {e}")),
                    },
                )
            }
            Message::PlayCtxDone(songs, _, 2) => self.update(Message::Queue(songs, false)),
            Message::PlayCtxDone(songs, src, mode) => {
                self.queue = songs;
                self.source = src;
                self.start(0, mode == 1)
            }
            Message::RowMenu(key, n) => {
                if !self.sel.as_ref().is_some_and(|s| s.0 == key) || !self.picked.contains(&n) {
                    (self.sel, self.picked, self.anchor) = (Some((key.clone(), n)), [n].into(), n);
                }
                let list = self.songs_of(&key).expect("a rendered row has its list").0;
                let idx: Vec<usize> = self
                    .picked
                    .iter()
                    .copied()
                    .filter(|&i| !list[i].is_dir)
                    .collect();
                let songs = idx.iter().map(|&i| list[i].clone()).collect();
                self.update(Message::Menu(Box::new(if key.starts_with("playlist:") {
                    MenuKind::SongIn(songs, idx)
                } else {
                    MenuKind::Song(songs)
                })))
            }
            Message::SelectAll => {
                let Some((list, _)) = self.songs_of(&self.route) else {
                    return Task::none();
                };
                self.sel = Some((self.route.clone(), 0));
                (self.picked, self.anchor) = ((0..list.len()).collect(), 0);
                Task::none()
            }
            Message::DeleteSel => match &self.sel {
                Some(s) if s.0 == self.route && self.route.starts_with("playlist:") => {
                    self.update(Message::PlRemove(self.picked.iter().copied().collect()))
                }
                _ => Task::none(),
            },
            Message::PlayAt(key, n) => match self.songs_of(&key) {
                Some((list, src)) => {
                    let n = n - list[..n.min(list.len())]
                        .iter()
                        .filter(|s| s.is_dir)
                        .count();
                    let list = list.into_iter().filter(|s| !s.is_dir).collect();
                    let t = self.update(Message::Play(list, n, src));
                    self.ctx = key;
                    t
                }
                None => self.update(Message::PlayCtx(key, 0)),
            },
            Message::Retry => {
                let key = self.key_of(&self.route);
                self.cache.remove(&key);
                self.request(&key, 0)
            }
            Message::FocusSearch => op::focus("search"),
            Message::FocusMove(back) => layer_focus(
                self.modal.is_some() || self.menu.is_some(),
                move |cur, n| match cur {
                    Some(i) => (i + if back { n - 1 } else { 1 }) % n,
                    None if back => n - 1,
                    None => 0,
                },
            ),
            Message::Arrow(up) if self.menu.is_some() => self.update(Message::FocusMove(up)),
            Message::Arrow(up) => {
                let Some((key, n)) = self.sel.clone() else {
                    return Task::none();
                };
                let list = self.songs_of(&key).expect("a selected row has its list").0;
                let n = if up {
                    n.saturating_sub(1)
                } else {
                    (n + 1).min(list.len().saturating_sub(1))
                };
                self.pick(key.clone(), n);
                let (kind, _) = key.split_once(':').unwrap_or((&key, ""));
                if key != self.route || !matches!(kind, "album" | "playlist" | "liked" | "songs") {
                    return Task::none();
                }
                let discs = if kind == "album" && list.iter().any(|s| s.disc != list[0].disc) {
                    1 + list[..=n]
                        .windows(2)
                        .filter(|w| w[0].disc != w[1].disc)
                        .count()
                } else {
                    0
                };
                let top = crate::pages::art_px(self)
                    + 2.0 * crate::pages::PAD
                    + crate::pages::BAR
                    + crate::pages::HEAD
                    + (n + discs) as f32 * crate::pages::ROW;
                let (off, view) = self.scroll();
                let to = if top < off + 100.0 {
                    top - 100.0
                } else if top + crate::pages::ROW > off + view {
                    top + crate::pages::ROW - view
                } else {
                    return Task::none();
                };
                self.scrolls.insert(key, (to.max(0.0), view));
                op::scroll_to(
                    crate::view::SCROLL,
                    AbsoluteOffset {
                        x: 0.0,
                        y: to.max(0.0),
                    },
                )
            }
            Message::Enter => match self.sel.clone() {
                Some((k, n)) if self.menu.is_none() && self.modal.is_none() => {
                    self.update(Message::PlayAt(k, n))
                }
                _ => Task::none(),
            },
            Message::VolUp => self.update(Message::Vol((self.volume + 0.1).min(1.0))),
            Message::Faster(up) => {
                let n = (self.cfg.settings.speed * 10.0).round() + if up { 1.0 } else { -1.0 };
                self.update(set(|s, v| s.speed = v, n.clamp(5.0, 35.0) / 10.0))
            }
            Message::Like => match self.current_song() {
                Some(s) => {
                    let (id, on) = (s.id.clone(), !self.is_starred(&s.id, s.starred));
                    self.update(Message::Star(vec![id], on))
                }
                None => Task::none(),
            },
            Message::VolDown => self.update(Message::Vol((self.volume - 0.1).max(0.0))),
            Message::Resize(w, h) => {
                let s = self.scale();
                self.cfg.settings.window = (w * s, h * s);
                let h = self.view_size().1;
                self.scrolls.values_mut().for_each(|s| s.1 = h - 136.0);
                self.covers_now()
            }
            Message::DiscChip(n) => {
                self.disc_chip = n;
                self.covers_now()
            }
            Message::DiscOrder => {
                self.disc_old = !self.disc_old;
                self.disc_load()
            }
            Message::SeekRelease => self
                .seek_preview
                .take()
                .map_or(Task::none(), |v| self.update(Message::Seek(v))),
            Message::Tick => {
                self.position = self.pos();
                Task::none()
            }
            Message::Heard(n) if n == self.listen_seq => self.listen(self.since.is_some()),
            Message::Heard(_) => Task::none(),
            Message::Modal(id, kind) => {
                self.menu = None;
                let p = self
                    .data::<Lib>("lib")
                    .and_then(|l| l.playlists.iter().find(|p| p.id == id))
                    .expect("dialogs open only for playlists in the library");
                let cover = p.cover.clone();
                (self.new_pl, self.new_pl_desc) = (p.name.clone(), p.comment.clone());
                let art = self.want_covers([(cover, 300)]);
                self.modal = Some((id, kind));
                Task::batch([
                    art,
                    layer_focus(true, |_, n| n.min(2) - 1)
                        .chain(op::select_all(iced::widget::Id::new("layer"))),
                ])
            }
            Message::Edit(id, name, comment) => {
                self.modal = None;
                let (name, comment) = (name.trim().to_string(), comment.trim().to_string());
                assert!(!name.is_empty(), "the dialog blocks an empty name");
                let set = |p: &mut Playlist| (p.name, p.comment) = (name.clone(), comment.clone());
                self.edit::<Lib>("lib", |l| {
                    l.playlists.iter_mut().filter(|p| p.id == id).for_each(set)
                });
                self.edit::<(Playlist, Vec<Song>)>(&format!("playlist:{id}"), |(p, _)| set(p));
                self.send(
                    |s| async move {
                        s.playlist_update(&id, Some(&name), Some(&comment), &[], &[])
                            .await
                    },
                    "Could not change the playlist.",
                )
            }
            Message::MediaInit(hwnd) => {
                let tx = CH.0.clone();
                let m = souvlaki::MediaControls::new(souvlaki::PlatformConfig {
                    display_name: "navify",
                    dbus_name: "navify",
                    hwnd: (hwnd != 0).then_some(hwnd as *mut std::ffi::c_void),
                })
                .and_then(|mut m| {
                    m.attach(move |e| drop(tx.send(Message::Media(e))))
                        .map(|()| m)
                });
                match m {
                    Ok(m) => self.media = Some(m),
                    Err(e) => {
                        return self.toast(&format!("Media controls are not available. {e:?}"));
                    }
                }
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
                    E::Raise => Message::Window(true),
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
                    let done = self.listen(false);
                    let n = if self.repeat == 2 {
                        Some(self.index)
                    } else {
                        self.next_index()
                    };
                    match n.filter(|&n| self.queue.get(self.real_at(n)).is_some_and(|s| s.id == id))
                    {
                        Some(n) => {
                            self.index = n;
                            Task::batch([done, self.began(), self.listen(true)])
                        }
                        None => Task::batch([done, self.listen(true)]),
                    }
                }
                navify_audio::Event::Ended => self.update(Message::Next(false)),
                navify_audio::Event::Error(e) => {
                    let done = self.listen(false);
                    (self.playing, self.loaded) = (false, false);
                    Task::batch([done, self.toast(&format!("Playback failed. {e}"))])
                }
            },
        }
    }
    fn halt(&mut self) -> Task<Message> {
        let l = self.listen(false);
        (self.playing, self.loaded) = (false, false);
        if let Some(p) = &self.player {
            p.stop();
        }
        l
    }

    fn queue_next_refresh(&self) {
        if let Some(p) = &self.player {
            let n = if self.repeat == 2 {
                Some(self.index)
            } else {
                self.next_index()
            };
            p.queue_next(n.map(|n| self.track(&self.queue[self.real_at(n)])));
        }
    }

    fn merge(&self, key: &str, v: AnyData) -> AnyData {
        let Some((_, old)) = self.cache.get(key) else {
            return v;
        };
        pager!(v, b => old.downcast_ref::<Pager<_>>().filter(|_| b.offset > 0).map(|a| {
            Arc::new(Pager { items: [&a.items[..], &b.items[..]].concat(), total: b.total, offset: 0 }) as AnyData
        }))
        .flatten()
        .unwrap_or(v)
    }

    fn page_len(&self, key: &str) -> Option<usize> {
        let v = &self.cache.get(key)?.1;
        pager!(v, p => (p.items.len() < p.total).then_some(p.items.len())).flatten()
    }

    fn visible_covers<'a>(&'a self) -> Vec<(String, u32)> {
        let (off, view) = self.scroll();
        let (cols, cw) = self.grid();
        let span = |n: usize, h: f32, per: usize| {
            let lo = (off / h).max(0.0) as usize;
            (lo.saturating_sub(1) * per).min(n)..((lo + (view / h) as usize + 3) * per).min(n)
        };
        let grid = |n| span(n, cw + 52.0, cols);
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
        } else if key.starts_with("discography:") {
            out.extend(
                crate::pages::disc_view(self)
                    .iter()
                    .map(|a| (a.cover.as_str(), 300)),
            );
        } else if let Some((a, albums)) = self.data::<(Artist, Vec<Album>)>(key) {
            let id = &key["artist:".len()..];
            let top = self
                .data::<Vec<Song>>(&format!("top:{id}"))
                .map_or(&[][..], |v| &v[..]);
            let similar = self
                .data::<Info>(&format!("info:{id}"))
                .map_or(&[][..], |i| &i.similar[..]);
            let appears = self
                .data::<Vec<Album>>(&format!("appears:{id}"))
                .map_or(&[][..], |v| &v[..]);
            out.push((&a.cover, 300));
            out.extend(top.iter().take(10).map(|s| (s.cover.as_str(), 128)));
            out.extend(
                crate::pages::disc_shelf(self, albums)
                    .iter()
                    .map(|x| (x.cover.as_str(), 300)),
            );
            out.extend(similar.iter().take(cols).map(|x| (x.cover.as_str(), 300)));
            out.extend(appears.iter().take(cols).map(|x| (x.cover.as_str(), 300)));
        } else if let Some(p) = self.data::<Pager<Album>>(key) {
            out.extend(
                p.items[grid(p.items.len())]
                    .iter()
                    .map(|a| (a.cover.as_str(), 300)),
            );
        } else if let Some(p) = self.data::<Pager<Artist>>(key) {
            out.extend(
                p.items[grid(p.items.len())]
                    .iter()
                    .map(|a| (a.cover.as_str(), 300)),
            );
        } else if let Some(r) = self.data::<SearchRes>(key) {
            out.extend(r.artists.iter().take(cols).map(|a| (a.cover.as_str(), 300)));
            out.extend(r.albums.iter().take(cols).map(|a| (a.cover.as_str(), 300)));
            out.extend(r.songs.iter().take(4).map(|s| (s.cover.as_str(), 128)));
        } else if let Some(h) = self.data::<Home>(key) {
            let pls = self
                .data::<Lib>("lib")
                .map_or(&[][..], |l| &l.playlists[..]);
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
        let ahead = if self.panel == 2 {
            100
        } else {
            self.panel as usize
        };
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

fn layer_focus(
    layer: bool,
    pick: impl Fn(Option<usize>, usize) -> usize + Send + 'static,
) -> Task<Message> {
    use iced::advanced::widget::operation::{Focusable, Outcome};
    use iced::advanced::widget::{Id, Operation};
    struct Scan<F>(bool, F, usize, Option<usize>);
    struct Nth(bool, usize, usize);
    impl<F: Fn(Option<usize>, usize) -> usize + Send> Operation<Message> for Scan<F> {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation<Message>)) {
            f(self);
        }
        fn focusable(&mut self, id: Option<&Id>, _: iced::Rectangle, s: &mut dyn Focusable) {
            if !self.0 || id == Some(&Id::from("layer")) {
                self.3 = if s.is_focused() { Some(self.2) } else { self.3 };
                self.2 += 1;
            }
        }
        fn finish(&self) -> Outcome<Message> {
            match self.2 {
                0 => Outcome::None,
                n => Outcome::Chain(Box::new(Nth(self.0, (self.1)(self.3, n), 0))),
            }
        }
    }
    impl Operation<Message> for Nth {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation<Message>)) {
            f(self);
        }
        fn focusable(&mut self, id: Option<&Id>, _: iced::Rectangle, s: &mut dyn Focusable) {
            let hit = (!self.0 || id == Some(&Id::from("layer"))) && {
                self.2 += 1;
                self.2 - 1 == self.1
            };
            if hit { s.focus() } else { s.unfocus() }
        }
    }
    iced::advanced::widget::operate(Scan(layer, pick, 0, None))
}

fn song_list(v: &AnyData) -> Option<(&[Song], &str)> {
    v.downcast_ref::<(Album, Vec<Song>)>()
        .map(|(a, s)| (&s[..], a.name.as_str()))
        .or_else(|| {
            v.downcast_ref::<(Playlist, Vec<Song>)>()
                .map(|(p, s)| (&s[..], p.name.as_str()))
        })
        .or_else(|| {
            v.downcast_ref::<Pager<Song>>()
                .map(|p| (&p.items[..], "Songs"))
        })
        .or_else(|| {
            v.downcast_ref::<SearchRes>()
                .map(|r| (&r.songs[..], "Search"))
        })
        .or_else(|| v.downcast_ref::<Vec<Song>>().map(|s| (&s[..], "Folder")))
}

fn shuffle(r: std::ops::Range<usize>, first: usize) -> Vec<usize> {
    use std::hash::BuildHasher;
    let h = std::collections::hash_map::RandomState::new();
    let mut o: Vec<usize> = r.collect();
    o.sort_by_cached_key(|&x| h.hash_one(x));
    if let Some(p) = o.iter().position(|&x| x == first) {
        o.swap(0, p);
    }
    o
}

pub static CURSOR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static MODS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

type Chan = (
    UnboundedSender<Message>,
    std::sync::Mutex<Option<UnboundedReceiver<Message>>>,
);
static CH: std::sync::LazyLock<Chan> = std::sync::LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, std::sync::Mutex::new(Some(rx)))
});

impl App {
    pub fn view(&self) -> iced::Element<'_, Message> {
        crate::view::view(self)
    }

    pub fn title(&self) -> String {
        self.current_song().map_or("Navify".into(), |s| {
            format!("{} \u{2022} {}", s.title, s.artist)
        })
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
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) => {
                MODS.store(m.bits(), std::sync::atomic::Ordering::Relaxed);
                None
            }
            iced::Event::Window(iced::window::Event::Focused) => Some(Message::Focus(true)),
            iced::Event::Window(iced::window::Event::Unfocused) => Some(Message::Focus(false)),
            iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::CloseWindow),
            iced::Event::Window(iced::window::Event::Resized(s)) => {
                Some(Message::Resize(s.width, s.height))
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) => Some(Message::MenuClose),
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
                            "a" => Some(Message::SelectAll),
                            "," => Some(Message::Nav("settings".into())),
                            "q" => Some(Message::Close),
                            "w" => Some(Message::Window(false)),
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
                        Key::Named(Named::Tab) => Some(Message::FocusMove(modifiers.shift())),
                        Key::Named(Named::Space) => Some(Message::Toggle),
                        Key::Named(Named::ArrowLeft) if modifiers.alt() => Some(Message::Back),
                        Key::Named(Named::ArrowRight) if modifiers.alt() => Some(Message::Fwd),
                        Key::Named(d @ (Named::ArrowLeft | Named::ArrowRight))
                            if modifiers.shift() =>
                        {
                            use souvlaki::{MediaControlEvent::SeekBy, SeekDirection as S};
                            let dir = if d == Named::ArrowLeft {
                                S::Backward
                            } else {
                                S::Forward
                            };
                            Some(Message::Media(SeekBy(
                                dir,
                                std::time::Duration::from_secs(5),
                            )))
                        }
                        Key::Character(c) if modifiers.alt() && modifiers.shift() => {
                            match c.to_lowercase().as_str() {
                                "h" => Some(Message::Nav("home".into())),
                                "s" => Some(Message::Nav("liked".into())),
                                "j" => Some(Message::Panel(1)),
                                "q" => Some(Message::Panel(2)),
                                "l" => Some(Message::LibCollapse),
                                "b" => Some(Message::Like),
                                _ => None,
                            }
                        }
                        Key::Character(c) if modifiers.shift() => match c.as_str() {
                            "<" | "," => Some(Message::Faster(false)),
                            ">" | "." => Some(Message::Faster(true)),
                            _ => None,
                        },
                        Key::Named(Named::ArrowUp) => Some(Message::Arrow(true)),
                        Key::Named(Named::ArrowDown) => Some(Message::Arrow(false)),
                        Key::Named(Named::Enter) => Some(Message::Enter),
                        Key::Named(Named::Delete | Named::Backspace) => Some(Message::DeleteSel),
                        _ => None,
                    }
                }
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(b))
                if matches!(b, iced::mouse::Button::Back | iced::mouse::Button::Forward) =>
            {
                Some(if b == iced::mouse::Button::Back {
                    Message::Back
                } else {
                    Message::Fwd
                })
            }
            _ => None,
        })];
        if self.drag.is_some() {
            subs.push(event::listen_with(|e, _, _| match e {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::DragTo(position.x))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => {
                    Some(Message::Drag(None))
                }
                _ => None,
            }));
        }
        if self.playing && !self.hidden {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::Tick),
            );
        }
        {
            fn audio_stream() -> impl iced::futures::Stream<Item = Message> {
                iced::stream::channel(
                    8,
                    |mut tx: iced::futures::channel::mpsc::Sender<_>| async move {
                        let mut rx =
                            CH.1.lock()
                                .expect("event channel")
                                .take()
                                .expect("one listener");
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
