use std::{
    any::Any,
    collections::{BTreeSet, HashMap, HashSet},
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use iced::keyboard::{Key, Modifiers};
use iced::widget::operation::{self as op, AbsoluteOffset};
use iced::{Color, Task, widget::image, widget::text_editor};
use navify_audio::Track;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::api::{Album, Artist, Info, Playlist, Session, Song, Trio};
use crate::config::{self, Config};

pub const PAGE: usize = 100;
const STALE: Duration = Duration::from_secs(300);
const KEEP: usize = 64;
const COVER_CAP: usize = 8 * 1024 * 1024;
const SIZES: [u32; 4] = [128, 192, 300, 640];
pub const ICON: &[u8] = include_bytes!("../assets/icons/hicolor/256x256/apps/navify.png");

pub type Home = [Vec<Album>; 4];

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Lib {
    pub playlists: Vec<Playlist>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub liked: usize,
}

type Page<T> = (Vec<T>, usize);
pub type AnyData = Arc<dyn Any + Send + Sync>;
type Decoded = (Color, image::Handle, usize);
type Slot = Option<(u64, image::Handle, usize)>;
type Rows = (Vec<Song>, Vec<usize>);
type Stamp = (Option<String>, u8, usize);
type Run = dyn Fn(&mut App) -> Task<Message> + Send + Sync;

macro_rules! pager {
    ($v:expr, $p:ident => $e:expr) => {
        ($v.downcast_ref::<Page<Album>>().and_then(|$p| $e))
            .or_else(|| $v.downcast_ref::<Page<Artist>>().and_then(|$p| $e))
            .or_else(|| $v.downcast_ref::<Page<Song>>().and_then(|$p| $e))
    };
}

macro_rules! stored {
    ($key:expr, $m:ident) => {
        match $key.split_once(':').map_or($key.as_str(), |p| p.0) {
            "home" => $m!(Home),
            "lib" => $m!(Lib),
            "album" => $m!((Album, Vec<Song>)),
            "playlist" => $m!((Playlist, Vec<Song>)),
            "artist" => $m!((Artist, Vec<Album>)),
            "top" => $m!(Vec<Song>),
            "info" => $m!(Info),
            _ => None,
        }
    };
}

fn data_file() -> std::path::PathBuf {
    crate::api::cache_dir("").join("data")
}

fn prune(v: &mut serde_json::Value) {
    use serde_json::Value::{Array, Bool, Null, Number, Object, String};
    match v {
        Object(m) => m.retain(|k, x| {
            prune(x);
            match x {
                Null | Bool(false) => false,
                Number(n) => n.as_f64() != Some(0.0) || k.ends_with("Gain") || k.ends_with("Peak"),
                String(s) => !s.is_empty(),
                Array(a) => !a.is_empty(),
                Object(o) => !o.is_empty(),
                _ => true,
            }
        }),
        Array(a) => a.iter_mut().for_each(prune),
        _ => {}
    }
}

fn load_data(server: &str) -> HashMap<String, (Instant, AnyData)> {
    let file = std::fs::File::open(data_file()).ok();
    let mut v: serde_json::Value = file
        .and_then(|f| serde_json::from_reader(flate2::read::DeflateDecoder::new(f)).ok())
        .unwrap_or_default();
    let (now, ours) = (Instant::now(), v["server"] == server);
    let Some(map) = v["data"].as_object_mut().filter(|_| ours) else {
        return HashMap::new();
    };
    let all = std::mem::take(map).into_iter().filter_map(|(k, v)| {
        macro_rules! de {
            ($t:ty) => {
                serde_json::from_value::<$t>(v)
                    .ok()
                    .map(|x| Arc::new(x) as AnyData)
            };
        }
        let d = stored!(k, de)?;
        Some((k, (now, d)))
    });
    all.collect()
}

#[derive(Clone, Debug)]
pub enum MenuKind {
    Song(Vec<Song>),
    SongIn(Vec<Song>, &'static str, Message),
    Album(Box<Album>),
    Artist(Artist),
    Playlist(Playlist),
    User,
    Create,
    View,
    Columns,
    Disc,
    LibSort,
}

#[derive(Clone, Debug)]
pub struct Menu {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
    pub kind: MenuKind,
    pub sub: u8,
    pub sub_y: f32,
    pub find: String,
}

#[derive(Clone)]
pub struct Act(Arc<Run>);

impl std::fmt::Debug for Act {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("Act")
    }
}

pub fn task(f: impl Fn(&mut App) -> Task<Message> + Send + Sync + 'static) -> Message {
    Message::Do(Act(Arc::new(f)))
}

pub fn act(f: impl Fn(&mut App) + Send + Sync + 'static) -> Message {
    task(move |a| {
        f(a);
        Task::none()
    })
}

pub fn set<T: Clone + Send + Sync + 'static>(f: fn(&mut config::Settings, T), v: T) -> Message {
    task(move |a| {
        let o = a.cfg.settings.clone();
        f(&mut a.cfg.settings, v.clone());
        a.settings(o)
    })
}

fn scan_devices() -> Task<Message> {
    let scan = tokio::task::spawn_blocking(navify_audio::devices);
    Task::perform(scan, |d| {
        let d = d.expect("device scan").map(|v| Arc::new(v) as AnyData);
        Message::Loaded("devices".into(), 0, d)
    })
}

fn ours(s: &Session, m: Message) -> Message {
    let s = s.clone();
    task(
        move |a: &mut App| match a.session.as_ref().is_some_and(|x| x.same(&s)) {
            true => a.update(m.clone()),
            false => Task::none(),
        },
    )
}

fn say(text: String) -> Message {
    task(move |a| a.toast(&text))
}

fn blocking<E: std::fmt::Display + Send + 'static>(
    fail: &'static str,
    f: impl FnOnce() -> Result<(), E> + Send + 'static,
) -> Task<Message> {
    Task::perform(tokio::task::spawn_blocking(f), move |r| {
        match r.expect("the blocking task ends") {
            Ok(()) => Message::Noop,
            Err(e) => say(format!("{fail}{e:#}")),
        }
    })
}

fn reload(pid: &str) -> impl Fn(&mut App) -> Task<Message> + Clone + Send + Sync + 'static {
    let key = format!("playlist:{pid}");
    move |a| {
        let shown = a.cache.remove(&key).is_some() && a.route == key;
        a.cache.remove("lib");
        let page = Task::batch(shown.then(|| a.request(&key, 0)));
        Task::batch([a.request("lib", 0), page])
    }
}

fn bare(id: &str) -> &str {
    id.split_once('#').map_or(id, |x| x.0)
}

static SHOWN: std::sync::LazyLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);

pub(crate) fn later(secs: f32, m: Message) -> Task<Message> {
    Task::future(tokio::time::sleep(Duration::from_secs_f32(secs))).map(move |()| m.clone())
}

#[derive(Clone, Debug)]
pub enum Message {
    Loaded(String, usize, Result<AnyData, String>),
    Nav(String),
    Back,
    Fwd,
    Scrolled(f32, f32, f32),
    LibScrolled(f32, f32),
    Search(String),
    SearchGo(u64),
    Play(Vec<Song>, usize, String),
    PlayQueue(usize),
    Toggle,
    Prev,
    Next(bool),
    Seek(f32),
    Shuffle,
    Vol(f32),
    Step(bool, f32),
    Star(Vec<String>, bool),
    Rate(String, u8),
    Queue(Vec<Song>, bool),
    QDel(usize),
    Menu(Box<MenuKind>),
    MenuBelow(Box<Message>, f32, f32),
    MenuClose,
    PlAdd(String, Vec<String>),
    AddCtx(String, String),
    LibMenu(String),
    PlNew(String, Vec<String>),
    PlDel(String),
    PlRemove(Vec<usize>),
    Radio(String),
    Panel(u8),
    Drag(Option<u8>),
    LField(u8, String),
    Desc(text_editor::Action),
    LSubmit,
    LogOut,
    UseServer(String),
    DropServer(String),
    Do(Act),
    ClearCache,
    Close,
    Noop,
    Edit(String, String, String, bool),
    Modal(String, u8),
    PlayCtx(String, u8),
    Warm(String),
    RowMenu(String, usize),
    SelectAll,
    DeleteSel,
    PlayAt(String, usize),
    Arrow(bool),
    Resize(f32, f32),
    Window(bool),
    Key(String),
    Confirm,
}

#[derive(Debug, Default)]
pub struct Covers {
    map: HashMap<String, (String, [Slot; 4])>,
    pub bytes: usize,
    tick: u64,
    color: HashMap<String, Color>,
    want: HashSet<(String, u32)>,
    failed: HashSet<String>,
    pub wait: bool,
    disk: usize,
}

fn slot(size: u32) -> usize {
    let i = SIZES.iter().position(|s| *s == size);
    i.unwrap_or_else(|| panic!("cover size {size} is not a bucket"))
}

impl Covers {
    pub fn empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, id: &str, size: u32) -> Option<image::Handle> {
        let ((_, e), mut near) = (self.map.get(bare(id))?, [0, 1, 2, 3]);
        near.sort_unstable_by_key(|&j| SIZES[j].abs_diff(size));
        near.iter().find_map(|&j| Some(e[j].as_ref()?.1.clone()))
    }

    pub fn color(&self, id: &str) -> Option<Color> {
        self.color.get(bare(id)).copied()
    }
    pub fn insert(&mut self, id: String, size: u32, (c, h, n): Decoded) {
        let (id, hash) = id.split_once('#').unwrap_or((&id, ""));
        self.tick += 1;
        self.color.insert(id.to_string(), c);
        let (old_hash, e) = self.map.entry(id.to_string()).or_default();
        if !hash.is_empty() && old_hash != hash {
            let freed = e.iter_mut().filter_map(Option::take);
            self.bytes -= freed.map(|s| s.2).sum::<usize>();
            *old_hash = hash.to_string();
        }
        let old = e[slot(size)].replace((self.tick, h, n));
        self.bytes = self.bytes + n - old.map_or(0, |o| o.2);
        while self.bytes > COVER_CAP {
            let all = self
                .map
                .iter()
                .flat_map(|(id, (_, e))| e.iter().enumerate().map(move |(j, s)| (s, id, j)));
            let all = all.filter_map(|(s, id, j)| Some((s.as_ref()?.0, id, j)));
            let (_, id, j) = all.min().expect("the counted bytes belong to covers");
            let id = id.clone();
            let (_, e) = self.map.get_mut(&id).expect("the cover is cached");
            self.bytes -= e[j].take().expect("the slot is full").2;
            if e.iter().all(Option::is_none) {
                self.map.remove(&id);
                self.color.remove(&id);
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
    pub seeds: HashMap<String, AnyData>,
    stale: HashSet<String>,
    pub pending: HashSet<String>,
    pub scrolls: HashMap<String, (f32, f32)>,
    pub lyric: bool,
    pub covers: Covers,
    pub player: Option<navify_audio::Player>,
    media: Option<souvlaki::MediaControls>,
    pub tray: Option<Box<dyn Any>>,
    win: Option<iced::window::Id>,
    os: (String, bool, bool),
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
    pub plays: HashMap<String, u32>,
    pub search_q: String,
    pub search_seq: u64,
    pub menu: Option<Menu>,
    pub toast: Option<(u64, String)>,
    pub toast_seq: u64,
    pub save_seq: u64,
    play_seq: u64,
    pub panel: u8,
    pub lib_filter: u8,
    pub lib_find: Option<String>,
    pub lib_scroll: (f32, f32),
    pub drag: Option<u8>,
    pub volume: f32,
    pub muted: bool,
    pub login: Login,
    pub hidden: bool,
    pub modal: Option<(String, u8)>,
    pub held: Option<Message>,
    expired: (Vec<String>, Option<Instant>),
    pub recording: Option<&'static str>,
    awake: (bool, Option<std::sync::mpsc::Sender<bool>>),
    pub new_pl_desc: Option<text_editor::Content>,
    pub shot: Option<String>,
    pub new_pl: String,
    pub new_pl_public: bool,
    pub sel: Option<(String, usize)>,
    pub picked: BTreeSet<usize>,
    anchor: usize,
    pub hover: Option<String>,
    pub popular_all: bool,
    pub search_tab: usize,
    pub disc_chip: u8,
    pub disc_old: bool,
    pub disc_name: bool,
    pub seek_preview: Option<f32>,
    pub recent: Vec<Song>,
    pub recent_tab: bool,
    pub qmove: Option<(usize, usize)>,
    pub full: bool,
    pub pl_find: Option<String>,
    pub pl_more: Option<String>,
    pub pl_sort: u8,
    pl_view: (String, Stamp, Option<Rows>),
}

fn shot_key(shot: Option<&str>) -> Option<String> {
    let k = shot?.split('|').next()?;
    (!k.is_empty() && k != "login" && k != "*").then(|| k.to_string())
}

fn login(u: String, n: String, p: String) -> Task<Message> {
    let run = async move { Session::login(&u, &n, &p).await };
    Task::perform(run, |r| {
        let r = r.map_err(|e| e.to_string());
        task(move |a| a.logged_in(r.clone()))
    })
}

impl App {
    pub fn new() -> Self {
        let cfg = Config::load();
        let shot = std::env::var("NAVIFY_SHOT").ok().filter(|s| !s.is_empty());
        let want = cfg.active.as_ref();
        let active = cfg.servers.iter().find(|s| want == Some(&s.id));
        let session = active.map(|s| Session::new(s.clone()));
        let route = if session.is_some() { "home" } else { "login" };
        let route = shot_key(shot.as_deref()).unwrap_or(route.into());
        let player = tokio::runtime::Handle::try_current().ok().map(|h| {
            let (p, mut rx) = navify_audio::Player::new(h.clone(), Session::client());
            p.set_device(&cfg.settings.device);
            h.spawn(async move {
                while let Some(e) = rx.recv().await {
                    let _ = CH.0.send(task(move |a| a.audio(e.clone())));
                }
            });
            p
        });
        let history = vec![if route == "login" { "login" } else { "home" }.into()];
        let volume = cfg.settings.volume;
        let cache = session
            .as_ref()
            .filter(|_| shot.is_none())
            .map_or_else(HashMap::new, |s| load_data(&s.server.id));
        Self {
            stale: cache.keys().cloned().collect(),
            cache,
            session,
            cfg,
            route,
            history,
            player,
            shot,
            volume,
            ..Self::default()
        }
    }

    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self::new();
        app.hidden = app.cfg.settings.start_min;
        let env = |k| std::env::var(k).ok();
        let creds = env("NAVIFY_URL").zip(env("NAVIFY_USER"));
        let creds = creds.zip(env("NAVIFY_PASS"));
        let open = Task::batch(app.shot.is_none().then(|| app.open()));
        let start = if app.session.is_none()
            && !app.shot.as_deref().is_some_and(|s| s.starts_with("login"))
            && let Some(((u, n), p)) = creds
        {
            app.login.busy = true;
            login(u, n, p)
        } else if app.session.is_some() || app.route != "login" {
            let r = std::mem::take(&mut app.route);
            Task::batch([app.nav(r), app.restore()])
        } else {
            app.show()
        };
        app.covers.wait = app.covers.disk > 0 && !app.stale.is_empty();
        let prune = app.disk(Some(300 << 20));
        (app, Task::batch([prune, open, start]))
    }

    fn apply_shot_setup(&mut self) -> Task<Message> {
        let lists = self.cache.values().filter_map(|(_, v)| song_list(v));
        let songs = lists.map(|l| l.0).find(|s| !s.is_empty());
        let songs = songs.unwrap_or(&[]).to_vec();
        let setups = self.shot.take().unwrap_or_default();
        let mut task = Task::none();
        for s in setups.split_once('|').map_or("", |x| x.1).split(',') {
            match s {
                "queue" | "panel1" => {
                    (self.queue, self.index) = (songs.iter().take(20).cloned().collect(), 0);
                    let album = self
                        .route
                        .starts_with("album:")
                        .then(|| songs.first())
                        .flatten();
                    self.source = album.map_or_else(|| "Liked Songs".into(), |s| s.album.clone());
                    self.panel = if s == "queue" { 2 } else { 1 };
                    let first = songs.first().map(|s| format!("lyrics:{}", s.id));
                    task = Task::batch(first.map(|k| self.request(&k, 0)));
                    let recent = self.data::<Home>("home").and_then(|h| h[0].first());
                    if let Some(a) = recent.filter(|_| songs.is_empty()) {
                        task = self.update(Message::PlayCtx(format!("album:{}", a.id), 0));
                    }
                }
                "menu" if !songs.is_empty() => {
                    let at = (480f32.to_bits() as u64) << 32 | 300f32.to_bits() as u64;
                    CURSOR.store(at, Relaxed);
                    let kind = Box::new(MenuKind::Song(songs[..1].to_vec()));
                    task = self.update(Message::Menu(kind));
                }
                "collapsed" => self.cfg.settings.lib_collapsed = true,
                "toast" => self.toast = Some((u64::MAX, "Added to queue".into())),
                "modal1" | "modal2" => {
                    let lib = self.data::<Lib>("lib").expect("the library is loaded");
                    let id = lib.playlists[0].id.clone();
                    task = self.update(Message::Modal(id, s.as_bytes()[5] - b'0'));
                }
                "lyrics" if !songs.is_empty() => {
                    (self.queue, self.index) = (songs[..1].to_vec(), 0);
                    task = self.nav("lyrics".into());
                }
                _ => {}
            }
        }
        task
    }

    pub fn data<T: 'static>(&self, key: &str) -> Option<&T> {
        let v = self.cache.get(key).map(|(_, v)| v);
        v.or_else(|| self.seeds.get(key))
            .and_then(|v| v.downcast_ref())
    }

    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        let lib = self.data::<Lib>("lib");
        let p = lib.and_then(|l| l.playlists.iter().find(|p| p.id == id));
        let key = format!("playlist:{id}");
        p.or_else(|| Some(&self.data::<(Playlist, Vec<Song>)>(&key)?.0))
    }

    pub fn owns(&self, p: &Playlist) -> bool {
        p.owner.is_empty()
            || self
                .session
                .as_ref()
                .is_some_and(|s| s.server.user == p.owner)
    }

    fn mine_pl(&self, key: &str) -> bool {
        key.strip_prefix("playlist:")
            .and_then(|id| self.playlist(id))
            .is_some_and(|p| self.owns(p))
    }

    pub fn is_starred(&self, id: &str, server: bool) -> bool {
        self.starred.get(id).copied().unwrap_or(server)
    }

    pub fn scroll(&self) -> (f32, f32) {
        let h = self.view_size().1 - 152.0;
        self.scrolls.get(&self.route).copied().unwrap_or((0.0, h))
    }

    pub fn scale(&self) -> f32 {
        self.cfg.settings.zoom
    }

    pub fn bucket(&self, px: f32) -> u32 {
        let need = px * self.scale();
        SIZES.into_iter().find(|&s| s as f32 >= need).unwrap_or(640)
    }

    fn open(&self) -> Task<Message> {
        use iced::{Size, window};
        let icon = ::image::load_from_memory(ICON).expect("the bundled icon is a PNG");
        let (icon, (w, h)) = (icon.into_rgba8(), self.cfg.settings.window);
        let icon = window::icon::from_rgba(icon.to_vec(), icon.width(), icon.height());
        let settings = window::Settings {
            size: Size::new(w.max(800.0), h.max(600.0)) * (1.0 / self.scale()),
            min_size: Some(Size::new(800.0, 600.0)),
            exit_on_close_request: false,
            visible: !(self.hidden && self.cfg.settings.tray),
            icon: Some(icon.expect("the bundled icon has a valid size")),
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: "navify".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        window::open(settings).1.map(|id| task(move |a| a.win(id)))
    }

    pub fn view_size(&self) -> (f32, f32) {
        let (s, (w, h)) = (self.scale(), self.cfg.settings.window);
        (w / s, h / s)
    }

    pub(crate) fn pick(&mut self, key: String, n: usize) {
        let m = Modifiers::from_bits_truncate(MODS.load(Relaxed));
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

    pub fn row_h(&self) -> f32 {
        let k = self.route.split(':').next().unwrap_or("");
        let c = self.cfg.settings.compact && matches!(k, "album" | "playlist" | "liked" | "songs");
        [56.0, 32.0][usize::from(c)]
    }

    pub fn lib_layout(&self) -> (u8, usize, f32, f32) {
        let view = [self.cfg.settings.lib_view, 1][usize::from(self.lib_mini())];
        let inner = self.lib_w() - 16.0;
        let cols = |min: f32| ((inner / min) as usize).max(2);
        match view {
            0 => (0, 1, inner, 32.0),
            2 => (
                2,
                cols(90.0),
                inner / cols(90.0) as f32,
                inner / cols(90.0) as f32,
            ),
            3 => (
                3,
                cols(128.0),
                inner / cols(128.0) as f32,
                inner / cols(128.0) as f32 + 50.0,
            ),
            _ => (1, 1, inner, 64.0),
        }
    }

    pub fn lib_mini(&self) -> bool {
        let s = &self.cfg.settings;
        let right = [48.0, s.panels.1 + 8.0][usize::from(self.panel > 0)];
        s.lib_collapsed || self.view_size().0 - 24.0 - s.panels.0 - right < 400.0
    }

    pub fn lib_w(&self) -> f32 {
        [self.cfg.settings.panels.0, 72.0][usize::from(self.lib_mini())]
    }

    pub fn menu_on(&self, route: &str) -> bool {
        self.menu.as_ref().is_some_and(|m| match &m.kind {
            MenuKind::Album(a) => route == format!("album:{}", a.id),
            MenuKind::Artist(a) => route == format!("artist:{}", a.id),
            MenuKind::Playlist(p) => route == format!("playlist:{}", p.id),
            _ => false,
        })
    }

    pub fn right_w(&self) -> f32 {
        self.cfg.settings.panels.1
    }

    pub fn main_w(&self) -> f32 {
        let right = [48.0, self.right_w() + 8.0][usize::from(self.panel > 0)];
        self.view_size().0 - 24.0 - self.lib_w() - right
    }

    pub fn grid(&self) -> (usize, f32) {
        let w = self.main_w() - [24.0, 48.0][usize::from(self.route.starts_with("search:"))];
        let n = ((w / 180.0) as usize).max(2);
        (n, w / n as f32)
    }

    pub(crate) fn songs_of(&self, key: &str) -> Option<(&[Song], String)> {
        if let Some(id) = key.strip_prefix("artist:") {
            let top = self.data::<Vec<Song>>(&format!("top:{id}"))?;
            return Some((top, self.find_artist(id)?.name.clone()));
        }
        let (s, name) = song_list(&self.cache.get(key)?.1)?;
        let s = self.pl_rows(key).map_or(s, |v| &v.0[..]);
        let name = if key == "liked" { "Liked Songs" } else { name };
        Some((s, name.to_string()))
    }

    fn pl_rows(&self, key: &str) -> Option<&Rows> {
        self.pl_view.2.as_ref().filter(|_| self.pl_view.0 == key)
    }

    fn orig(&self, key: &str, i: usize) -> usize {
        self.pl_rows(key).map_or(i, |v| v.1[i])
    }

    fn pl_view_of(&self) -> Option<Rows> {
        let q = self.pl_find.as_deref().unwrap_or("").to_lowercase();
        if q.is_empty() && self.pl_sort == 0 {
            return None;
        }
        let songs = &self.data::<(Playlist, Vec<Song>)>(&self.route)?.1;
        let hit = |s: &Song| {
            [&s.title, &s.artist, &s.album]
                .iter()
                .any(|f| f.to_lowercase().contains(&q))
        };
        let mut idx: Vec<usize> = (0..songs.len()).filter(|&n| hit(&songs[n])).collect();
        let low = |s: &String| s.to_lowercase();
        match self.pl_sort & 15 {
            1 => idx.sort_by_cached_key(|&n| low(&songs[n].title)),
            2 => idx.sort_by_cached_key(|&n| low(&songs[n].artist)),
            3 => idx.sort_by_cached_key(|&n| low(&songs[n].album)),
            4 => idx.sort_by_key(|&n| &songs[n].created),
            5 => idx.sort_by_key(|&n| songs[n].year),
            6 => idx.sort_by_key(|&n| (songs[n].duration * 1000.0) as u32),
            _ => {}
        }
        if self.pl_sort & 16 != 0 {
            idx.reverse();
        }
        Some((idx.iter().map(|&n| songs[n].clone()).collect(), idx))
    }

    fn edit<T: Clone + Send + Sync + 'static>(&mut self, key: &str, f: impl FnOnce(&mut T)) {
        if let Some((_, v)) = self.cache.get_mut(key)
            && let Some(x) = v.downcast_ref::<T>()
        {
            let mut x = x.clone();
            f(&mut x);
            *v = Arc::new(x);
        }
    }

    fn edit_pl(&mut self, id: &str, f: impl Fn(&mut Playlist)) {
        let mine = |p: &&mut Playlist| p.id == id;
        self.edit::<Lib>("lib", |l| l.playlists.iter_mut().filter(mine).for_each(f));
    }

    fn recount(&mut self, pid: &str, more: (u32, f32), f: impl FnOnce(&mut Vec<Song>)) {
        let key = format!("playlist:{pid}");
        self.edit::<(Playlist, Vec<Song>)>(&key, |(p, s)| {
            f(s);
            (p.song_count, p.duration) = (s.len() as u32, s.iter().map(|x| x.duration).sum());
        });
        let now = self.data::<(Playlist, Vec<Song>)>(&key);
        let now = now.map(|x| (x.0.song_count, x.0.duration));
        self.edit_pl(pid, |p| {
            let add = (p.song_count + more.0, p.duration + more.1);
            (p.song_count, p.duration) = now.unwrap_or(add);
        });
    }

    fn pl_send(
        &mut self,
        pid: String,
        verb: &str,
        add: Vec<String>,
        rm: Vec<usize>,
    ) -> Task<Message> {
        self.menu = None;
        let done = format!("{verb} {}.", self.playlist(&pid).map_or("", |p| &p.name));
        let old = self.playlist(&pid);
        let old = old.map_or(String::new(), |p| p.cover.clone());
        let fresh = self.pl_fresh(pid.clone(), old, 3);
        let undo = reload(&pid);
        let f =
            |s: Session| async move { s.playlist_update(&pid, None, None, None, &add, &rm).await };
        Task::batch([
            self.send(f, "Could not change the playlist.", undo)
                .chain(fresh),
            self.toast(&done),
        ])
    }

    fn pl_fresh(&self, id: String, old: String, n: u8) -> Task<Message> {
        let ok = move |p: Vec<Playlist>| {
            let (id, old) = (id.clone(), old.clone());
            task(move |a| {
                let new = p.iter().find(|x| x.id == id).map(|x| x.cover.clone());
                if let Some(c) = &new {
                    let key = format!("playlist:{id}");
                    a.edit::<(Playlist, Vec<Song>)>(&key, |(x, _)| x.cover.clone_from(c));
                }
                a.edit::<Lib>("lib", |l| l.playlists = p.clone());
                let hash = |c: &str| c.split_once('#').map_or("", |x| x.1).to_string();
                let same = n > 0 && new.as_deref().map(hash) == Some(hash(&old));
                let (id, old) = (id.clone(), old.clone());
                let again = move |a: &mut App| a.pl_fresh(id.clone(), old.clone(), n - 1);
                let again = same.then(|| later(2.0, task(again)));
                Task::batch([a.covers_now()].into_iter().chain(again))
            })
        };
        let load = |s: Session| async move { s.playlists().await };
        self.call(load, "Could not load the playlists.", ok)
    }

    fn find_song(&self, id: &str) -> Option<Song> {
        let mut lists = self.cache.values().filter_map(|(_, v)| song_list(v));
        let s = lists.find_map(|(l, _)| l.iter().find(|s| s.id == id));
        s.or_else(|| self.queue.iter().chain(&self.recent).find(|s| s.id == id))
            .cloned()
    }

    fn find_album(&self, id: &str) -> Option<Album> {
        self.cache.values().find_map(|(_, v)| {
            let home = v.downcast_ref::<Home>().into_iter().flatten();
            let lists = [
                v.downcast_ref::<Page<Album>>().map(|p| &p.0[..]),
                v.downcast_ref::<Trio>().map(|r| &r.1[..]),
                v.downcast_ref::<(Artist, Vec<Album>)>().map(|x| &x.1[..]),
                v.downcast_ref::<Vec<Album>>().map(|x| &x[..]),
                v.downcast_ref::<Lib>().map(|l| &l.albums[..]),
                v.downcast_ref::<(Album, Vec<Song>)>()
                    .map(|x| std::slice::from_ref(&x.0)),
            ];
            let all = lists.into_iter().flatten();
            let mut all = all.chain(home.map(|v| &v[..])).flatten();
            all.find(|a| a.id == id).cloned()
        })
    }

    pub fn find_artist(&self, id: &str) -> Option<&Artist> {
        let mut all = self.cache.values().flat_map(|(_, v)| {
            [
                v.downcast_ref::<Lib>().map(|l| &l.artists[..]),
                v.downcast_ref::<Page<Artist>>().map(|p| &p.0[..]),
                v.downcast_ref::<Trio>().map(|r| &r.0[..]),
                v.downcast_ref::<(Artist, Vec<Album>)>()
                    .map(|x| std::slice::from_ref(&x.0)),
                v.downcast_ref::<(Vec<Artist>, Vec<Song>)>()
                    .map(|x| &x.0[..]),
                v.downcast_ref::<Vec<Artist>>().map(|x| &x[..]),
                v.downcast_ref::<Info>().map(|x| &x.similar[..]),
            ]
            .into_iter()
            .flatten()
            .flatten()
        });
        all.clone()
            .find(|a| a.id == id && !a.music_brainz_id.is_empty())
            .or_else(|| all.find(|a| a.id == id))
    }

    fn star(&mut self, ids: &[String], on: bool) {
        for id in ids {
            let was = self.starred.insert(id.clone(), on);
            if let Some(mut s) = self.find_song(id) {
                let had = usize::from(was.unwrap_or(s.starred));
                self.edit::<Lib>("lib", |l| {
                    l.liked = (l.liked + on as usize).saturating_sub(had)
                });
                s.starred = on;
                self.edit::<Page<Song>>("liked", |(items, total)| {
                    let had = items.iter().any(|x| x.id == s.id);
                    items.retain(|x| x.id != s.id);
                    *total = (*total + on as usize).saturating_sub(had as usize);
                    items.splice(0..0, on.then_some(s));
                });
            } else if let Some(mut a) = self.find_album(id) {
                a.starred = on;
                self.edit::<Lib>("lib", |l| {
                    l.albums.retain(|x| x.id != a.id);
                    l.albums.extend(on.then_some(a));
                });
            } else if let Some(mut a) = self.find_artist(id).cloned() {
                a.starred = on;
                self.edit::<Lib>("lib", |l| {
                    l.artists.retain(|x| x.id != a.id);
                    l.artists.extend(on.then_some(a));
                });
            }
        }
    }

    pub fn current_song(&self) -> Option<&Song> {
        (self.index < self.queue.len()).then(|| &self.queue[self.real_at(self.index)])
    }

    pub fn real_at(&self, i: usize) -> usize {
        self.shuffle.as_ref().map_or(i, |o| {
            assert_eq!(o.len(), self.queue.len(), "shuffle covers the queue");
            o.get(i).copied().unwrap_or(i)
        })
    }

    pub(crate) fn history(&mut self) -> Task<Message> {
        let native = self.session.as_ref().is_some_and(|s| s.server.native);
        match native && self.panel == 2 && self.recent_tab {
            true => self.request("songs:recent", 0),
            false => Task::none(),
        }
    }

    pub(crate) fn request(&mut self, key: &str, offset: usize) -> Task<Message> {
        let fresh = |(t, v): &(Instant, AnyData)| {
            t.elapsed() < STALE && (key.starts_with("banner:") || !v.is::<String>())
        };
        if offset == 0 && !self.stale.contains(key) && self.cache.get(key).is_some_and(fresh) {
            return Task::none();
        }
        match self.session.clone() {
            Some(s) if self.pending.insert(format!("{key}#{offset}")) => {
                fetch(s, key.to_string(), offset)
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn toast(&mut self, text: &str) -> Task<Message> {
        self.toast_seq += 1;
        let n = self.toast_seq;
        self.toast = Some((n, text.to_string()));
        let clear = act(move |a| a.toast = a.toast.take().filter(|t| t.0 != n));
        later(3.0, clear)
    }

    fn save_later(&mut self) -> Task<Message> {
        let covers = self.covers_now();
        if !self.cfg.settings.queue_sync {
            return covers;
        }
        self.save_seq += 1;
        let n = self.save_seq;
        let save = task(move |a| Task::batch((n == a.save_seq).then(|| a.save_queue())));
        Task::batch([covers, later(5.0, save)])
    }

    fn save_queue(&self) -> Task<Message> {
        if !self.cfg.settings.queue_sync || self.queue.iter().any(|s| !s.url.is_empty()) {
            return Task::none();
        }
        let ids: Vec<String> = self.queue.iter().map(|x| x.id.clone()).collect();
        let cur = self.current_song().map_or(String::new(), |x| x.id.clone());
        let pos = (self.pos() * 1000.0) as u64;
        let save = |s: Session| async move { s.save_play_queue(&ids, &cur, pos).await };
        self.send(save, "Could not save the play queue.", |_| Task::none())
    }

    fn persist(&self) -> Task<Message> {
        let mut c = self.cfg.clone();
        if let Some(s) = &self.session
            && let Some(v) = c.servers.iter_mut().find(|v| v.id == s.server.id)
        {
            v.jwt = s.jwt();
        }
        static LAST: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Relaxed);
        let save = move || {
            let mut last = LAST.lock().expect("save lock");
            if n < *last {
                return Ok(());
            }
            *last = n;
            c.save()
        };
        blocking("Could not save the settings. ", save)
    }

    fn save_data(&self) -> Task<Message> {
        let Some(s) = &self.session else {
            return Task::none();
        };
        let mut all: Vec<_> = (self.cache.iter())
            .filter_map(|(k, (t, d))| {
                macro_rules! ser {
                    ($t:ty) => {
                        d.downcast_ref::<$t>()
                            .and_then(|x| serde_json::to_value(x).ok())
                    };
                }
                let top = k != "home" && k != "lib";
                Some(((top, std::cmp::Reverse(*t)), k.clone(), stored!(k, ser)?))
            })
            .collect();
        all.sort_by_key(|a| a.0);
        let data: serde_json::Map<_, _> = all.into_iter().take(40).map(|e| (e.1, e.2)).collect();
        let mut body = serde_json::json!({"server": s.server.id, "data": data});
        blocking("Could not save the page cache. ", move || {
            use flate2::{Compression, write::DeflateEncoder};
            prune(&mut body);
            let (path, tmp) = (data_file(), data_file().with_extension("tmp"));
            std::fs::create_dir_all(path.parent().expect("the cache has a directory"))?;
            let mut z = DeflateEncoder::new(Vec::new(), Compression::best());
            serde_json::to_writer(&mut z, &body)?;
            std::fs::write(&tmp, z.finish()?)?;
            Ok::<_, anyhow::Error>(std::fs::rename(tmp, path)?)
        })
    }

    pub fn gain(&self, song: &Song) -> f32 {
        let (st, rg) = (&self.cfg.settings, &song.rg);
        let (db, peak) = match st.replaygain.as_str() {
            "track" => (
                rg.track_gain.or(rg.album_gain),
                rg.track_peak.or(rg.album_peak),
            ),
            "album" => (
                rg.album_gain.or(rg.track_gain),
                rg.album_peak.or(rg.track_peak),
            ),
            _ => return 1.0,
        };
        let peak = peak.unwrap_or(1.0).max(1e-3);
        let db = db.unwrap_or(st.fallback_db) + st.preamp_db;
        let max = [f32::MAX, 1.0 / peak][usize::from(st.rg_clip)];
        10f32.powf(db / 20.0).min(max)
    }

    fn track(&self, song: &Song) -> Track {
        let st = &self.cfg.settings;
        let s = self.session.as_ref().expect("playback needs a session");
        let format = Some(st.transcode_format.as_str()).filter(|f| !f.is_empty());
        let (url, transcoded) = s.stream_url(song, format, st.transcode_max_bitrate);
        let (id, gain, duration) = (song.id.clone(), self.gain(song), song.duration);
        let rate = [song.sampling_rate, 48_000][usize::from(format == Some("opus"))];
        Track {
            id,
            url,
            gain,
            transcoded,
            duration,
            rate,
        }
    }

    fn disk(&self, covers: Option<u64>) -> Task<Message> {
        let dir = crate::api::cache_dir("");
        let scan = move || {
            covers.inspect(|&c| crate::api::prune(&dir.join("covers"), c));
            let files = crate::api::cache_files(&dir);
            files.iter().map(|f| f.2).sum::<u64>()
        };
        Task::perform(tokio::task::spawn_blocking(scan), |n| {
            let n: AnyData = Arc::new(n.expect("cache scan"));
            Message::Loaded("disk".into(), 0, Ok(n))
        })
    }

    fn next_index(&self, manual: bool) -> Option<usize> {
        let (i, len) = (self.index, self.queue.len());
        if self.repeat == 2 && !manual {
            return (i < len).then_some(i);
        }
        let wrap = (self.repeat >= 1 && len > 0).then_some(0);
        (i + 1 < len).then_some(i + 1).or(wrap)
    }

    pub fn binding(&self, id: &str) -> &str {
        let default = KEYS.iter().find(|k| k.0 == id).expect("a known action").2;
        let keys = &self.cfg.settings.keys;
        keys.get(id).map_or(default, |s| s.as_str())
    }

    fn kept(&self) -> impl Fn(&Song) -> bool + use<> {
        let words = self.cfg.settings.queue_skip.to_lowercase();
        let words = words.split(',').map(str::trim).filter(|w| !w.is_empty());
        let words: Vec<String> = words.map(String::from).collect();
        move |s| {
            words.is_empty() || {
                let hay = [&s.title, &s.artist, &s.album, &s.genre].map(|f| f.to_lowercase());
                !words.iter().any(|w| hay.iter().any(|h| h.contains(w)))
            }
        }
    }

    fn start(&mut self, songs: Vec<Song>, src: String, i: usize, shuffled: bool) -> Task<Message> {
        let done = Task::batch(self.loaded.then(|| self.listen(false)));
        (self.queue, self.source, self.scrobbled) = (songs, src, true);
        self.play_seq += 1;
        let (keep, had) = (self.kept(), !self.queue.is_empty());
        let i = self.queue.iter().take(i).filter(|s| keep(s)).count();
        self.queue.retain(|s| keep(s));
        self.shuffle = shuffled.then(|| shuffle(0..self.queue.len(), i));
        if self.queue.is_empty() {
            (self.index, self.ctx, self.source) = Default::default();
            let t = had.then(|| self.toast("The queue filter removed all the songs."));
            return Task::batch([done, self.halt()].into_iter().chain(t));
        }
        Task::batch([done, self.play_index(if shuffled { 0 } else { i }, 0.0)])
    }

    fn play_index(&mut self, i: usize, at: f32) -> Task<Message> {
        if self.queue.is_empty() {
            return Task::none();
        }
        self.qmove = None;
        let done = Task::batch(self.loaded.then(|| self.listen(false)));
        (self.index, self.playing) = (i.min(self.queue.len() - 1), true);
        let t = self.track(&self.queue[self.real_at(self.index)]);
        self.player.iter().for_each(|p| p.play(t.clone(), at));
        let began = self.began();
        self.position = at;
        Task::batch([done, began, self.listen(true)])
    }

    pub fn lead_artist(&self) -> Option<&str> {
        let s = self.current_song().filter(|s| s.url.is_empty())?;
        let id = s.artists.first().map_or(&s.artist_id, |a| &a.id);
        (!id.is_empty()).then_some(id)
    }

    pub fn artist_cover<'a>(&'a self, id: &'a str) -> &'a str {
        let cover = self.find_artist(id).map(|a| a.cover.as_str());
        cover.filter(|c| !c.is_empty()).unwrap_or(id)
    }

    fn about(&mut self) -> Task<Message> {
        let go = self.panel == 1 && !self.full;
        let id = self.lead_artist().filter(|_| go);
        let id = id.map(|id| format!("info:{id}"));
        let lyr = go || self.route == "lyrics";
        let lyr = self.current_song().filter(|s| lyr && s.url.is_empty());
        let lyr = lyr.map(|s| format!("lyrics:{}", s.id));
        Task::batch([id, lyr].into_iter().flatten().map(|k| self.request(&k, 0)))
    }

    fn began(&mut self) -> Task<Message> {
        self.seek_preview = None;
        (self.position, self.scrobbled, self.heard, self.loaded) = (0.0, false, 0.0, true);
        self.lyric = true;
        self.queue_next_refresh();
        let Some(song) = self.current_song().cloned() else {
            return Task::none();
        };
        self.recent.retain(|s| s.id != song.id);
        self.recent.insert(0, song.clone());
        self.recent.truncate(50);
        let song_id = song.id.clone();
        let now = |s: Session| async move { s.scrobble(&song_id, false).await };
        let msg = "Could not send the playing track to the server.";
        let now = self.send(now, msg, |_| Task::none());
        let radio = !song.url.is_empty();
        let now = Task::batch((self.cfg.settings.scrobble && !radio).then_some(now));
        let lyr = self.about();
        #[cfg(all(unix, not(target_os = "macos")))]
        let files = self
            .session
            .as_ref()
            .map(|s| [128, 192, 300, 640].map(|z| s.cover_file(&song.cover, z)));
        let show = move || {
            let mut n = notify_rust::Notification::new();
            let body = format!("{}{}{}", song.artist, crate::pages::DOT, song.album);
            n.appname("Navify")
                .summary(&song.title)
                .body(&body)
                .icon("navify");
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                static ID: AtomicU32 = AtomicU32::new(0);
                if let Some(p) = files.iter().flatten().find(|p| p.exists()) {
                    n.image_path(&p.to_string_lossy());
                }
                let h = n.id(ID.load(Relaxed)).show().map_err(|e| e.to_string())?;
                ID.store(h.id(), Relaxed);
                Ok::<_, String>(())
            }
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            n.show().map(drop).map_err(|e| e.to_string())
        };
        let note =
            (self.cfg.settings.notify).then(|| blocking("Could not show a notification. ", show));
        Task::batch([lyr, now, Task::batch(note), self.save_later()])
    }

    pub fn pos(&self) -> f32 {
        let p = self.player.as_ref().filter(|_| self.loaded);
        p.map_or(self.position, |p| p.position())
    }

    pub fn clock(&self) -> Box<dyn Fn() -> f32> {
        let fixed = self.seek_preview.unwrap_or(self.position);
        match self
            .player
            .as_ref()
            .filter(|_| self.loaded && self.seek_preview.is_none())
        {
            Some(p) => Box::new(p.clock()),
            None => Box::new(move || fixed),
        }
    }

    fn listen(&mut self, on: bool) -> Task<Message> {
        let enabled = self.cfg.settings.scrobble;
        let since = self.since.take();
        self.heard += since.map_or(0.0, |t| t.elapsed().as_secs_f32());
        self.since = on.then(Instant::now);
        self.listen_seq += 1;
        let ok = |s: &&Song| enabled && !self.scrobbled && s.duration > 0.0;
        let Some(song) = self.current_song().filter(ok) else {
            return Task::none();
        };
        let st = &self.cfg.settings;
        let pct = (song.duration * st.scrobble_pct / 100.0).min(240.0);
        let at = [pct, st.scrobble_secs][usize::from(st.scrobble_by_time)];
        let at = at.min(song.duration - st.crossfade_secs - 1.0).max(0.0);
        let left = at - self.heard;
        let (id, base) = (song.id.clone(), song.play_count);
        if left <= 0.0 {
            self.scrobbled = true;
            let n = self.plays.get(&id).map_or(base, |&n| n.max(base));
            self.plays.insert(id.clone(), n + 1);
            let done = |s: Session| async move { s.scrobble(&id, true).await };
            return self.send(
                done,
                "Could not send the played track to the server.",
                |_| Task::none(),
            );
        }
        let n = self.listen_seq;
        let heard =
            task(move |a| Task::batch((n == a.listen_seq).then(|| a.listen(a.since.is_some()))));
        Task::batch(on.then(|| later(left, heard)))
    }

    fn restore(&self) -> Task<Message> {
        if !self.cfg.settings.queue_sync || self.shot.is_some() {
            return Task::none();
        }
        let ok = |(songs, cur, pos): (Vec<Song>, String, u64)| {
            task(move |a| {
                if !songs.is_empty() && !a.loaded && a.queue.is_empty() {
                    a.position = pos as f32 / 1000.0;
                    a.index = songs.iter().position(|s| s.id == cur).unwrap_or(0);
                    (a.queue, a.shuffle) = (songs.clone(), None);
                }
                a.covers_now()
            })
        };
        let fail = "Could not restore the play queue.";
        self.call(|s| async move { s.play_queue().await }, fail, ok)
    }

    pub fn covers_now(&mut self) -> Task<Message> {
        let ids = self.visible_covers();
        *SHOWN.lock().expect("cover set") = ids.iter().map(|(k, _)| bare(k).into()).collect();
        self.want_covers(ids)
    }

    pub fn disc_load(&mut self) -> Task<Message> {
        let albums = crate::pages::disc_view(self);
        let keys = albums.iter().filter(|_| !self.cfg.settings.disc_grid);
        let keys: Vec<String> = keys.map(|a| format!("album:{}", a.id)).collect();
        let mut tasks: Vec<_> = keys.iter().map(|k| self.request(k, 0)).collect();
        tasks.push(self.covers_now());
        Task::batch(tasks)
    }

    pub fn repeat(&mut self) {
        self.repeat = (self.repeat + 1) % 3;
        self.queue_next_refresh();
    }

    pub fn lib_collapse(&mut self) -> Task<Message> {
        self.cfg.settings.lib_collapsed = !self.cfg.settings.lib_collapsed;
        self.covers_now()
    }

    pub fn set_full(&mut self, on: bool) -> Task<Message> {
        use iced::window::{Mode, set_mode};
        self.full = on;
        let mode = [Mode::Windowed, Mode::Fullscreen][usize::from(on)];
        let mode = self.win.map(|id| set_mode(id, mode));
        Task::batch([Task::batch(mode), self.covers_now()])
    }

    fn want_covers(&mut self, ids: impl IntoIterator<Item = (String, u32)>) -> Task<Message> {
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
        let mut need = Vec::new();
        for (id, size) in ids {
            self.covers.tick += 1;
            let (t, e) = (self.covers.tick, self.covers.map.get_mut(bare(&id)));
            let hash = id.split_once('#').map_or("", |x| x.1);
            let e = e.filter(|(h, _)| hash.is_empty() || h == hash);
            let have = e
                .and_then(|(_, e)| e[slot(size)].as_mut())
                .map(|s| s.0 = t)
                .is_some();
            let bad = self.covers.failed.contains(bare(&id));
            let raw = bare(&id).rsplit(':').next().unwrap_or_default();
            if raw.is_empty() || have || bad || !self.covers.want.insert((bare(&id).into(), size)) {
                continue;
            }
            let seen = SHOWN.lock().expect("cover set").contains(bare(&id));
            need.push((id, size, seen));
        }
        if need.is_empty() {
            return Task::none();
        }
        type Batch = Vec<(String, u32, Result<Decoded, String>)>;
        static ONE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        static NET: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
        type Known = Option<(std::path::PathBuf, HashMap<String, Color>)>;
        static KNOWN: std::sync::Mutex<Known> = std::sync::Mutex::new(None);
        let dec = |items: Vec<(String, u32, Result<bytes::Bytes, String>)>,
                   mut colors: HashMap<String, Color>,
                   s: &Session| {
            let (file, s) = (s.cover_file("", 0).with_file_name("colors"), s.clone());
            async move {
                if items.is_empty() {
                    return Vec::new();
                }
                let _one = ONE.acquire().await.expect("the decode semaphore is open");
                tokio::task::spawn_blocking(move || {
                    let mut known = KNOWN.lock().expect("color lock");
                    if known.as_ref().is_none_or(|k| k.0 != file) {
                        let text = std::fs::read_to_string(&file).unwrap_or_default();
                        if text.len() > 1 << 20 {
                            let _ = std::fs::remove_file(&file);
                        }
                        let rgb = |l: &str| {
                            let (k, c) = l.rsplit_once(' ')?;
                            let [_, r, g, b] = u32::from_str_radix(c, 16).ok()?.to_be_bytes();
                            Some((k.to_string(), Color::from_rgb8(r, g, b)))
                        };
                        *known = Some((file.clone(), text.lines().filter_map(rgb).collect()));
                    }
                    let known = &mut known.as_mut().expect("loaded colors").1;
                    let mut add = String::new();
                    let out = items.into_iter().map(|(k, sz, b)| {
                        let c = colors.get(bare(&k)).or(known.get(&k)).copied();
                        let r = b.and_then(|b| decode(b, &k, sz, c, &s.cover_file(&k, sz)));
                        if let Ok((c, _, _)) = &r {
                            colors.insert(bare(&k).into(), *c);
                            if known.insert(k.clone(), *c).is_none() {
                                let [r, g, b, _] = c.into_rgba8();
                                add += &format!("{k} {r:02x}{g:02x}{b:02x}\n");
                            }
                        }
                        (k, sz, r)
                    });
                    let out = out.collect::<Batch>();
                    if !add.is_empty() {
                        use std::io::Write;
                        let f = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&file);
                        let _ = f.and_then(|mut f| f.write_all(add.as_bytes()));
                    }
                    out
                })
                .await
                .expect("decode task")
            }
        };
        let apply = |a: &mut App, out: Batch| {
            let mut err = None;
            for (id, size, r) in out {
                a.covers.want.remove(&(bare(&id).into(), size));
                match r {
                    Ok(d) => a.covers.insert(id, size, d),
                    Err(e) if e.is_empty() => {}
                    Err(e) => {
                        a.covers.failed.insert(bare(&id).into());
                        err = Some(e);
                    }
                }
            }
            err.map(|e| a.toast(&format!("Could not load a cover. {e}")))
        };
        self.covers.disk += 1;
        let (colors, own, s2) = (self.covers.color.clone(), s.clone(), s.clone());
        let load = async move {
            let (mut hit, mut miss) = (Vec::new(), Vec::new());
            for (k, sz, seen) in need {
                match tokio::fs::read(s.cover_file(&k, sz)).await {
                    Ok(b) if !b.is_empty() => hit.push((k, sz, Ok(b.into()))),
                    _ => miss.push((k, sz, seen)),
                }
            }
            (dec(hit, colors, &s).await, miss)
        };
        Task::perform(load, move |(out, miss)| {
            let s = s2.clone();
            let t = task(move |a| {
                a.covers.disk = a.covers.disk.saturating_sub(1);
                if a.covers.disk == 0 {
                    a.covers.wait = false;
                }
                let mut tasks: Vec<_> = apply(a, out.clone()).into_iter().collect();
                for chunk in miss.chunks(4) {
                    let (chunk, s, own, colors) =
                        (chunk.to_vec(), s.clone(), s.clone(), a.covers.color.clone());
                    let net = async move {
                        let mut dl = Vec::new();
                        for (k, sz, seen) in chunk {
                            let s = s.clone();
                            dl.push(tokio::spawn(async move {
                                let _net =
                                    NET.acquire().await.expect("the cover semaphore is open");
                                let gone =
                                    seen && !SHOWN.lock().expect("cover set").contains(bare(&k));
                                let r = match gone {
                                    true => Err(String::new()),
                                    false => s.cover(&k, sz).await.map_err(|e| e.to_string()),
                                };
                                (k, sz, r)
                            }));
                        }
                        let mut got = Vec::new();
                        for h in dl {
                            got.push(h.await.expect("cover task"));
                        }
                        dec(got, colors, &s).await
                    };
                    tasks.push(Task::perform(net, move |res| {
                        let res = res.clone();
                        ours(&own, task(move |a| Task::batch(apply(a, res.clone()))))
                    }));
                }
                Task::batch(tasks)
            });
            ours(&own, t)
        })
    }

    fn call<T, F>(
        &self,
        f: impl FnOnce(Session) -> F,
        fail: &'static str,
        ok: impl Fn(T) -> Message + Send + 'static,
    ) -> Task<Message>
    where
        F: Future<Output = anyhow::Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
        Task::perform(f(s.clone()), move |r| {
            ours(&s, r.map_or_else(|e| say(format!("{fail} {e}")), &ok))
        })
    }

    fn send<F, U>(&self, f: impl FnOnce(Session) -> F, fail: &str, undo: U) -> Task<Message>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
        U: Fn(&mut App) -> Task<Message> + Clone + Send + Sync + 'static,
    {
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
        let fail = fail.to_string();
        Task::perform(f(s.clone()), move |r| match r {
            Ok(()) => Message::Noop,
            Err(e) => {
                let (undo, text) = (undo.clone(), format!("{fail} {e}"));
                ours(&s, task(move |a| Task::batch([undo(a), a.toast(&text)])))
            }
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

    pub(crate) fn scroll_to(&self, y: f32) -> Task<Message> {
        op::scroll_to(crate::view::SCROLL, AbsoluteOffset { x: 0.0, y })
    }

    fn show(&mut self) -> Task<Message> {
        let route = self.history[self.hi].clone();
        (self.route, self.pl_find, self.pl_more) = (route.clone(), None, None);
        self.covers.wait = false;
        self.panel = [self.panel, 0][usize::from(self.panel == 4)];
        (self.menu, self.sel, self.picked) = (None, None, BTreeSet::new());
        let keep = |h: &String| h.starts_with("lib") || h == "queue" || h == "search";
        self.hover = self.hover.take().filter(keep);
        (self.popular_all, self.disc_chip) = (false, 0);
        if !route.starts_with("search") {
            self.search_tab = 0;
        }
        let q = route.strip_prefix("search:").unwrap_or("");
        if self.search_q.trim() != q {
            (self.search_q, self.search_seq) = (q.into(), self.search_seq + 1);
        }
        let key = self.key_of(&route);
        self.seeds.extend(self.seed(&key).map(|v| (key.clone(), v)));
        let id = route.strip_prefix("artist:");
        let b = id
            .filter(|_| self.cfg.settings.banners)
            .and_then(|_| self.data::<(Artist, Vec<Album>)>(&key))
            .map(|(a, _)| a.banner_key());
        let parts = id.map(|id| ["top", "info", "appears"].map(|k| format!("{k}:{id}")));
        let parts = parts.as_ref().map_or(&[][..], |p| &p[..]);
        let about = self.lead_artist().map(|id| format!("info:{id}"));
        let keep = |k: &String| {
            *k == key
                || k == "lib"
                || parts.contains(k)
                || about.as_ref() == Some(k)
                || b.as_ref() == Some(k)
        };
        let old = self
            .cache
            .iter()
            .filter(|(k, (t, _))| t.elapsed() >= STALE && !keep(k));
        let mut old: Vec<(Instant, String)> = old.map(|(k, (t, _))| (*t, k.clone())).collect();
        old.sort_unstable();
        let n = self.cache.len().saturating_sub(KEEP).min(old.len());
        old[..n]
            .iter()
            .for_each(|(_, k)| drop(self.cache.remove(k)));
        let keys = parts.iter().chain(&b).map(String::as_str);
        let keys = [key.as_str(), "lib"].into_iter().chain(keys);
        let mut tasks: Vec<_> = keys.map(|k| self.request(k, 0)).collect();
        if self.full {
            tasks.push(self.set_full(false));
        }
        tasks.push(self.disc_load());
        match route == "lyrics" {
            true => self.lyric = true,
            false => tasks.push(self.scroll_to(self.scrolls.get(&route).map_or(0.0, |s| s.0))),
        }
        if route == "login" {
            let l = &self.login;
            let n = [&l.url, &l.user].iter().position(|v| v.is_empty());
            tasks.push(op::focus(format!("login{}", n.unwrap_or(2))));
        }
        if route == "settings" {
            tasks.extend([scan_devices(), self.disk(None)]);
        }
        Task::batch(tasks)
    }

    fn seed(&self, key: &str) -> Option<AnyData> {
        let (kind, id) = key.split_once(':')?;
        if self.cache.get(key).is_some_and(|(_, v)| !v.is::<String>()) {
            return None;
        }
        Some(match kind {
            "album" => Arc::new((self.find_album(id)?, Vec::<Song>::new())),
            "artist" => Arc::new((self.find_artist(id)?.clone(), Vec::<Album>::new())),
            "playlist" => Arc::new((self.playlist(id)?.clone(), Vec::<Song>::new())),
            _ => return None,
        })
    }

    pub(crate) fn settings(&mut self, o: config::Settings) -> Task<Message> {
        self.menu = None;
        let n = &self.cfg.settings;
        if let Some(p) = &self.player
            && o.device != n.device
        {
            p.set_device(&n.device);
        }
        let gain = |s: &config::Settings| {
            let t = (s.transcode_format.clone(), s.transcode_max_bitrate);
            (
                s.replaygain.clone(),
                s.preamp_db,
                s.fallback_db,
                s.rg_clip,
                t,
            )
        };
        let point = |s: &config::Settings| {
            (
                s.scrobble,
                s.scrobble_by_time,
                s.scrobble_pct,
                s.scrobble_secs,
            )
        };
        let heard = self.loaded && point(&o) != point(n);
        let (grid, zoom) = (o.disc_grid != n.disc_grid, o.zoom != n.zoom);
        if gain(&o) != gain(n)
            && let (Some(p), Some(s)) = (&self.player, self.current_song())
        {
            p.set_gain(self.gain(s));
            self.queue_next_refresh();
        }
        let (w, h) = self.view_size();
        let grid = Task::batch(grid.then(|| self.disc_load()));
        let zoom = Task::batch(zoom.then(|| self.update(Message::Resize(w, h))));
        let heard = Task::batch(heard.then(|| self.listen(self.playing)));
        Task::batch([grid, zoom, heard, self.covers_now(), self.persist()])
    }

    fn reset(&mut self) -> Task<Message> {
        let h = self.halt();
        (self.queue, self.shuffle, self.index, self.position) = (Vec::new(), None, 0, 0.0);
        (self.ctx, self.source, self.qmove) = Default::default();
        (self.starred, self.ratings, self.plays, self.recent) = Default::default();
        (self.cache, self.seeds, self.pending, self.scrolls) = Default::default();
        self.stale.clear();
        (self.covers, self.history, self.hi, self.panel) = Default::default();
        (self.menu, self.modal, self.recording, self.hover) = Default::default();
        (self.sel, self.picked, self.pl_find, self.pl_more) = Default::default();
        (self.lib_find, self.full) = Default::default();
        (self.route, self.expired, self.save_seq) =
            (String::new(), Default::default(), self.save_seq + 1);
        h
    }

    fn halt(&mut self) -> Task<Message> {
        let l = self.listen(false);
        self.seek_preview = None;
        (self.playing, self.loaded) = (false, false);
        self.player.iter().for_each(|p| p.stop());
        l
    }

    fn queue_next_refresh(&self) {
        if let Some(p) = &self.player {
            let n = self.next_index(false);
            p.queue_next(n.map(|n| self.track(&self.queue[self.real_at(n)])));
        }
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        use souvlaki::MediaPlayback as P;
        let seek = matches!(m, Message::Seek(_));
        let mut t = self.handle(m);
        let data = self.cache.get(&self.route);
        let data = data.map_or(0, |c| Arc::as_ptr(&c.1).cast::<()>() as usize);
        let (r, (f, s, d), _) = &self.pl_view;
        if (r, f, *s, *d) != (&self.route, &self.pl_find, self.pl_sort, data) {
            let diff = (f, *s) != (&self.pl_find, self.pl_sort);
            let moved = diff || r.starts_with("playlist:");
            let stamp = (self.pl_find.clone(), self.pl_sort, data);
            if moved && self.sel.as_ref().is_some_and(|s| s.0 == self.route) && *r == self.route {
                (self.sel, self.picked) = (None, BTreeSet::new());
            }
            let rows = self.pl_view_of();
            let rows = rows.filter(|_| self.route.starts_with("playlist:"));
            let cov = (diff || *r == self.route) && self.route.starts_with("playlist:");
            self.pl_view = (self.route.clone(), stamp, rows);
            if cov {
                t = Task::batch([t, self.covers_now()]);
            }
        }
        let awake = self.playing && self.cfg.settings.awake;
        if awake != self.awake.0 {
            let tx = self
                .awake
                .1
                .get_or_insert_with(|| crate::power::spawn(CH.0.clone()));
            tx.send(awake).expect("the power thread runs");
            self.awake.0 = awake;
        }
        if let Some(p) = &self.player {
            p.set_volume(if self.muted { 0.0 } else { self.volume });
            p.set_crossfade(self.cfg.settings.crossfade_secs);
        }
        let (song, s) = (self.current_song(), self.session.as_ref());
        let id = song.map_or("", |s| s.id.as_str());
        let same = (id, self.playing) == (self.os.0.as_str(), self.os.1) && !seek;
        if self.media.is_none() || same && self.os.2 {
            return t;
        }
        let file = song.zip(s);
        let file = file.map(|(s, x)| x.cover_file(&s.cover, self.bucket(56.0)));
        let art = file.filter(|f| f.exists());
        if same && art.is_none() {
            return t;
        }
        self.os = (id.to_string(), self.playing, art.is_some());
        let (song, pos) = (self.current_song().cloned(), self.pos());
        let song = song.as_ref();
        let art = art.map(|f| format!("file://{}", f.display()));
        if let Some(m) = &mut self.media {
            let _ = m.set_metadata(souvlaki::MediaMetadata {
                title: song.map(|s| s.title.as_str()),
                album: song.map(|s| s.album.as_str()),
                artist: song.map(|s| s.artist.as_str()),
                cover_url: art.as_deref(),
                duration: song.map(|s| Duration::from_secs_f32(s.duration.max(0.0))),
            });
            let progress = Duration::from_secs_f32(pos.max(0.0));
            let progress = Some(souvlaki::MediaPosition(progress));
            let _ = m.set_playback(match (song, self.playing) {
                (None, _) => P::Stopped,
                (_, true) => P::Playing { progress },
                _ => P::Paused { progress },
            });
        }
        t
    }

    fn handle(&mut self, m: Message) -> Task<Message> {
        let st = &self.cfg.settings;
        let replaces = match &m {
            Message::Play(..) => true,
            Message::PlayAt(k, n) => {
                let ok = self.songs_of(k);
                let ok = ok.is_none_or(|(l, _)| l.get(*n).is_some_and(|s| !s.is_dir));
                ok && st.play_mode < 2 && *k != self.ctx
            }
            Message::PlayCtx(k, 0) => st.play_mode < 2 && *k != self.ctx,
            Message::PlayCtx(k, 1) => *k != self.ctx,
            _ => false,
        };
        if replaces && st.confirm_queue && self.modal.is_none() && !self.queue.is_empty() {
            (self.menu, self.modal, self.held) = (None, Some((String::new(), 4)), Some(m));
            return layer_focus(true, |_, n| n.min(2) - 1);
        }
        match m {
            Message::Confirm => {
                let t = self.held.take().map_or_else(Task::none, |m| self.update(m));
                self.modal = None;
                return t;
            }
            Message::Loaded(key, off, Err(e)) => {
                self.pending.remove(&format!("{key}#{off}"));
                if self.shot.is_some() {
                    eprintln!("LOAD-ERR {key} {e}");
                }
                if self.cache.contains_key(&key) || key != self.key_of(&self.route) {
                    let load = (key == "lib" && self.route == "home").then(|| self.disc_load());
                    return Task::batch([self.toast(&e)].into_iter().chain(load));
                }
                self.seeds.remove(&key);
                self.cache.insert(key, (Instant::now(), Arc::new(e)));
            }
            Message::Loaded(key, off, Ok(v)) => {
                self.pending.remove(&format!("{key}#{off}"));
                self.stale.remove(&key);
                let mb = key
                    .strip_prefix("artist:")
                    .and_then(|id| self.find_artist(id))
                    .map(|a| a.music_brainz_id.clone())
                    .filter(|m| !m.is_empty());
                let seeded = self.seeds.remove(&key).is_some();
                let old = self.cache.get(&key).map(|c| &c.1);
                let join = |old: &AnyData| {
                    pager!(v, b => old.downcast_ref::<Page<_>>().and_then(|a| match off {
                        0 => {
                            let r = [&b.0[..], &a.0[b.0.len().min(a.0.len())..]].concat();
                            let same = r.iter().zip(&a.0).all(|(x, y)| x.id == y.id);
                            (same && a.1 == b.1 && a.0.len() > b.0.len()).then_some(r)
                        }
                        _ => (a.0.len() == off).then(|| [&a.0[..], &b.0[..]].concat()),
                    }).map(|r| Arc::new((r, b.1)) as AnyData))
                };
                let mut v = match old.and_then(join) {
                    None if off == 0 => v,
                    Some(v) => v,
                    _ => return Task::none(),
                };
                if let Some((m, (a, _))) = mb.zip(
                    Arc::get_mut(&mut v).and_then(|x| x.downcast_mut::<(Artist, Vec<Album>)>()),
                ) && a.music_brainz_id.is_empty()
                {
                    a.music_brainz_id = m;
                }
                let banner = key.starts_with("artist:") && self.cfg.settings.banners;
                let banner = banner.then(|| v.downcast_ref::<(Artist, Vec<Album>)>());
                let banner = banner.flatten();
                let banner = banner.map(|(a, _)| a.banner_key());
                self.cache.insert(key.clone(), (Instant::now(), v));
                let banner = Task::batch(banner.map(|k| self.request(&k, 0)));
                let setup = shot_key(self.shot.as_deref()).is_some_and(|k| k == key);
                let setup = Task::batch(setup.then(|| self.apply_shot_setup()));
                let wait = self.route == "home"
                    && matches!(key.as_str(), "home" | "lib")
                    && (self.pending.contains("home#0") || self.pending.contains("lib#0"));
                if wait {
                    return Task::batch([setup, banner]);
                }
                let d0 = self.covers.disk;
                let load = self.disc_load();
                let active =
                    key == self.key_of(&self.route) || (self.route == "home" && key == "lib");
                if off == 0 && !seeded && self.covers.disk > d0 && active {
                    self.covers.wait = true;
                }
                return Task::batch([load, setup, banner]);
            }
            Message::Nav(r) => return self.nav(r),
            Message::Back if self.hi > 0 && self.session.is_some() && self.modal.is_none() => {
                self.hi -= 1;
                return self.show();
            }
            Message::Fwd
                if self.hi + 1 < self.history.len()
                    && self.session.is_some()
                    && self.modal.is_none() =>
            {
                self.hi += 1;
                return self.show();
            }
            Message::Back | Message::Fwd => {}
            Message::Scrolled(off, view, content) => {
                let (key, step) = (self.route.clone(), self.row_h() as usize);
                let old = self.scrolls.insert(key.clone(), (off, view));
                let old = old.unwrap_or_default();
                let v = self.cache.get(&key).map(|c| &c.1);
                let more = v.and_then(|v| pager!(v, p => (p.0.len() < p.1).then_some(p.0.len())));
                let more = more.filter(|_| content > 0.0 && off + view * 3.0 >= content);
                let more = Task::batch(more.map(|n| self.request(&key, n)));
                let moved = old.0 as usize / step != off as usize / step || old.1 != view;
                return Task::batch([more, Task::batch(moved.then(|| self.disc_load()))]);
            }
            Message::Search(q) => {
                (self.search_q, self.search_seq) = (q, self.search_seq + 1);
                let (t, n) = (self.search_q.trim(), self.search_seq);
                if t.is_empty() || self.cache.contains_key(&format!("search:{t}")) {
                    return self.update(Message::SearchGo(n));
                }
                return later(0.25, Message::SearchGo(n));
            }
            Message::SearchGo(n) if n == self.search_seq => {
                let q = self.search_q.trim();
                let r = format!("search{}{q}", if q.is_empty() { "" } else { ":" });
                return match (r == self.route, self.route.starts_with("search")) {
                    (true, _) => self.request(&r, 0),
                    (false, true) => {
                        self.history.truncate(self.hi + 1);
                        self.history[self.hi] = r;
                        self.show()
                    }
                    (false, false) => self.nav(r),
                };
            }
            Message::SearchGo(_) | Message::Noop => {}
            Message::Play(songs, i, source) => {
                self.ctx.clear();
                return self.start(songs, source, i, self.shuffle.is_some());
            }
            Message::PlayQueue(i) => return self.play_index(i, 0.0),
            Message::Toggle if !self.loaded => return self.play_index(self.index, self.position),
            Message::Toggle => {
                self.playing = !self.playing;
                self.player.iter().for_each(|p| p.set_paused(!self.playing));
                let l = self.listen(self.playing);
                if self.playing {
                    return l;
                }
                (self.position, self.save_seq) = (self.pos(), self.save_seq + 1);
                return Task::batch([l, self.save_queue()]);
            }
            Message::Prev if self.pos() > 3.0 || self.index == 0 => {
                return self.update(Message::Seek(0.0));
            }
            Message::Prev => return self.play_index(self.index - 1, 0.0),
            Message::Next(manual) => {
                return match self.next_index(manual) {
                    Some(n) => self.play_index(n, 0.0),
                    None => {
                        let done = self.halt();
                        (self.index, self.position) = (0, 0.0);
                        Task::batch([done, self.covers_now()])
                    }
                };
            }
            Message::Seek(pos) => {
                let Some(dur) = self.current_song().map(|s| s.duration) else {
                    return Task::none();
                };
                let pos = pos.clamp(0.0, dur.max(0.0));
                let again = pos < 5.0 && self.pos() > 10.0;
                (self.position, self.lyric) = (pos, true);
                self.player.iter().for_each(|p| p.seek(pos));
                if again {
                    let done = self.listen(false);
                    (self.scrobbled, self.heard) = (false, 0.0);
                    return Task::batch([done, self.listen(self.playing)]);
                }
            }
            Message::Shuffle => {
                let cur = self.current_song().map_or(0, |_| self.real_at(self.index));
                (self.shuffle, self.index) = match self.shuffle {
                    Some(_) => (None, cur),
                    None => (Some(shuffle(0..self.queue.len(), cur)), 0),
                };
                self.queue_next_refresh();
                return self.save_later();
            }
            Message::Vol(v) => (self.volume, self.cfg.settings.volume, self.muted) = (v, v, false),
            Message::Step(true, d) => {
                let v = self.volume + d * self.cfg.settings.vol_step / 100.0;
                return self.update(Message::Vol(v.clamp(0.0, 1.0)));
            }
            Message::Step(false, d) => {
                let to = self.pos() + d * self.cfg.settings.seek_secs;
                return self.update(Message::Seek(to.max(0.0)));
            }
            Message::Star(ids, on) => {
                self.menu = None;
                if self.route == "liked" && !on {
                    (self.sel, self.picked) = (None, BTreeSet::new());
                }
                let was = |id: &String| {
                    let s = self.starred.get(id).copied();
                    s.or_else(|| self.find_song(id).map(|s| s.starred))
                        .or_else(|| self.find_album(id).map(|a| a.starred))
                        .or_else(|| self.find_artist(id).map(|a| a.starred))
                };
                let back = ids.iter().filter(|id| was(id) != Some(on)).cloned();
                let back: Vec<String> = back.collect();
                self.star(&ids, on);
                let star = |s: Session| async move { s.star(&ids, on).await };
                let star = self.send(star, "Could not update Your Library.", move |a| {
                    a.star(&back, !on);
                    Task::none()
                });
                return Task::batch([self.covers_now(), star]);
            }
            Message::Rate(id, n) => {
                let old = self.ratings.insert(id.clone(), n);
                self.menu = None;
                let back = id.clone();
                let rate = |s: Session| async move { s.rate(&id, n).await };
                return self.send(rate, "Could not set the rating.", move |a| {
                    match old {
                        Some(v) => a.ratings.insert(back.clone(), v),
                        None => a.ratings.remove(&back),
                    };
                    Task::none()
                });
            }
            Message::Queue(songs, next) => {
                self.menu = None;
                if songs.is_empty() {
                    return Task::none();
                }
                let songs: Vec<Song> = songs.into_iter().filter(self.kept()).collect();
                if songs.is_empty() {
                    return self.toast("The queue filter removed all the songs.");
                }
                let n = songs.len();
                let cur = self.current_song().map(|_| self.real_at(self.index));
                let q = cur.filter(|_| next).map_or(self.queue.len(), |r| r + 1);
                self.queue.splice(q..q, songs);
                if let Some(o) = &mut self.shuffle {
                    o.iter_mut().filter(|x| **x >= q).for_each(|x| *x += n);
                    let (at, new) = match next {
                        true => ((self.index + 1).min(o.len()), (q..q + n).collect()),
                        false => (o.len(), shuffle(q..q + n, usize::MAX)),
                    };
                    o.splice(at..at, new);
                }
                self.queue_next_refresh();
                return Task::batch([self.save_later(), self.toast("Added to queue")]);
            }
            Message::QDel(i) => {
                (self.qmove, self.menu) = (None, None);
                let ri = self.real_at(i);
                if ri < self.queue.len() && i != self.index {
                    self.queue.remove(ri);
                    if let Some(o) = &mut self.shuffle {
                        o.retain(|&x| x != ri);
                        o.iter_mut().filter(|x| **x > ri).for_each(|x| *x -= 1);
                    }
                    self.index -= (i < self.index) as usize;
                }
                self.queue_next_refresh();
                return self.save_later();
            }
            Message::Menu(kind) => {
                let (x, y) = match *kind {
                    MenuKind::Create => (self.lib_w() - 108.0, 123.0),
                    MenuKind::User => (self.view_size().0 - 338.0, 65.0),
                    MenuKind::View => (self.lib_w() + self.main_w() - 148.0, cursor().1 + 24.0),
                    MenuKind::Disc => (crate::pages::disc_x(self), cursor().1 + 24.0),
                    MenuKind::LibSort => (self.lib_w() - 220.0, cursor().1 + 24.0),
                    _ => (cursor().0, cursor().1 + 8.0),
                };
                self.menu = Some(Menu {
                    x,
                    y,
                    dx: 0.0,
                    dy: y - cursor().1,
                    kind: *kind,
                    sub: 0,
                    sub_y: 0.0,
                    find: String::new(),
                });
                return Task::none();
            }
            Message::MenuBelow(m, dx, dy) => {
                let t = self.update(*m);
                let a = *ANCHOR.lock().expect("the anchor lock");
                if let Some(m) = &mut self.menu {
                    (m.x, m.y) = (a.x + dx, a.y + a.height + dy);
                    (m.dx, m.dy) = (a.width - 2.0 * dx, a.height / 2.0 + dy);
                }
                return t;
            }
            Message::MenuClose
                if self.full
                    && self.menu.is_none()
                    && self.modal.is_none()
                    && self.recording.is_none() =>
            {
                return self.set_full(false);
            }
            Message::MenuClose => {
                if self.menu.is_none() && self.modal.is_none() && self.recording.is_none() {
                    (self.sel, self.picked) = (None, BTreeSet::new());
                    if self.pl_find.take().is_some()
                        | self.lib_find.take().is_some()
                        | self.pl_more.take().is_some()
                    {
                        return self.covers_now();
                    }
                }
                (self.menu, self.modal, self.recording, self.held) = (None, None, None, None);
            }
            Message::Key(c) if self.recording.is_some() => {
                let id = self.recording.take().expect("a recording");
                let named = ["Tab", "Shift+Tab", "Enter", "Delete", "Backspace"];
                if c.starts_with("Arrow") || named.contains(&c.as_str()) {
                    return Task::none();
                }
                let used = KEYS.iter().map(|k| k.0).filter(|k| self.binding(k) == c);
                let used: Vec<&str> = used.collect();
                let keys = &mut self.cfg.settings.keys;
                keys.extend(used.into_iter().map(|k| (k.to_string(), String::new())));
                keys.insert(id.to_string(), c);
                return self.persist();
            }
            Message::Key(c) => {
                let hit = KEYS.iter().find(|k| {
                    self.binding(k.0) == c
                        && (matches!(k.0, "close" | "quit")
                            || self.session.is_some()
                                && (self.modal.is_none()
                                    || k.0 == "shortcuts"
                                        && self.modal.as_ref().is_some_and(|m| m.1 == 3)))
                });
                let base = c.rsplit('+').next().filter(|_| !c.starts_with("C+"));
                let open = self.session.is_some() && self.modal.is_none();
                let m = match (hit, base.unwrap_or_default()) {
                    (Some(k), _) => (k.3)(),
                    (_, "Tab") => return self.focus_move(c.contains("Shift+")),
                    (_, "ArrowUp") if open => Message::Arrow(true),
                    (_, "ArrowDown") if open => Message::Arrow(false),
                    (_, "Enter") if open && self.menu.is_none() => {
                        return Task::batch(
                            self.sel
                                .clone()
                                .map(|(k, n)| self.update(Message::PlayAt(k, n))),
                        );
                    }
                    (_, "Delete" | "Backspace") if open && self.menu.is_none() => {
                        Message::DeleteSel
                    }
                    _ => return Task::none(),
                };
                return self.update(m);
            }
            Message::PlAdd(pid, ids) => {
                let have = self.data::<(Playlist, Vec<Song>)>(&format!("playlist:{pid}"));
                let dup = have.is_some_and(|p| p.1.iter().any(|s| ids.contains(&s.id)));
                if dup && self.modal.is_none() {
                    let add = Some(Message::PlAdd(pid.clone(), ids));
                    (self.menu, self.modal, self.held) = (None, Some((pid, 7)), add);
                    return layer_focus(true, |_, n| n.min(2) - 1);
                }
                if self.modal.as_ref().is_some_and(|m| m.1 == 7) {
                    (self.modal, self.held) = (None, None);
                }
                let songs: Vec<Song> = ids.iter().filter_map(|id| self.find_song(id)).collect();
                let more = (songs.len() as u32, songs.iter().map(|s| s.duration).sum());
                let fix = (songs.len() < ids.len()).then(|| Task::done(task(reload(&pid))));
                self.recount(&pid, more, |s| s.extend(songs));
                let t = self.pl_send(pid, "Added to", ids, Vec::new());
                return fix.into_iter().fold(t, Task::chain);
            }
            Message::PlNew(name, ids) => {
                self.menu = None;
                let n = self.data::<Lib>("lib").map_or(0, |l| l.playlists.len()) + 1;
                let name = match name.is_empty() {
                    true => format!("My Playlist #{n}"),
                    false => name,
                };
                let make = |s: Session| async move { s.playlist_create(&name, &ids).await };
                return self.call(make, "Could not make the playlist.", |p: Playlist| {
                    task(move |a| {
                        let name = p.name.to_lowercase();
                        a.edit::<Lib>("lib", |l| {
                            let at = l
                                .playlists
                                .partition_point(|x| x.name.to_lowercase() < name);
                            l.playlists.insert(at, p.clone())
                        });
                        a.nav(format!("playlist:{}", p.id))
                    })
                });
            }
            Message::PlDel(id) => {
                (self.menu, self.modal) = (None, None);
                let r = format!("playlist:{id}");
                self.edit::<Lib>("lib", |l| l.playlists.retain(|p| p.id != id));
                self.cache.remove(&r);
                let back = Task::batch((self.route == r).then(|| self.nav("home".into())));
                let (mut h, mut hi) = (Vec::new(), 0);
                for (i, k) in std::mem::take(&mut self.history).into_iter().enumerate() {
                    if k != r && h.last() != Some(&k) {
                        h.push(k);
                    }
                    if i == self.hi {
                        hi = h.len() - 1;
                    }
                }
                (self.history, self.hi) = (h, hi);
                let undo = reload(&id);
                let del = |s: Session| async move { s.playlist_delete(&id).await };
                return Task::batch([back, self.send(del, "Could not delete the playlist.", undo)]);
            }
            Message::PlRemove(idx) => {
                let Some(pid) = self.route.strip_prefix("playlist:").map(str::to_string) else {
                    return Task::none();
                };
                (self.sel, self.picked) = (None, BTreeSet::new());
                self.recount(&pid, (0, 0.0), |s| {
                    let mut n = 0;
                    s.retain(|_| (!idx.contains(&n), n += 1).0);
                });
                return self.pl_send(pid, "Removed from", Vec::new(), idx);
            }
            Message::Radio(id) => {
                self.menu = None;
                let similar = |s: Session| async move { s.similar_songs(&id, 30).await };
                self.play_seq += 1;
                let seq = self.play_seq;
                let ok = move |v: Vec<Song>| match v.is_empty() {
                    true => say("No similar songs were found.".into()),
                    false => task(move |a| match a.play_seq == seq {
                        true => a.update(Message::Play(v.clone(), 0, "Radio".into())),
                        false => Task::none(),
                    }),
                };
                return self.call(similar, "Could not start the radio.", ok);
            }
            Message::Panel(n) => {
                self.panel = if self.panel == n { 0 } else { n };
                if self.panel != 4 {
                    self.pl_more = None;
                }
                let scan = (self.panel == 3).then(scan_devices);
                let find = (self.panel == 4).then(|| op::focus("pladd"));
                let t = [self.covers_now(), self.about(), self.history()];
                return Task::batch(t.into_iter().chain(scan).chain(find));
            }
            Message::Drag(d) => {
                self.drag = d;
                if d.is_none() {
                    return Task::batch([self.persist(), self.covers_now()]);
                }
            }
            Message::LField(i, v) => {
                let l = &mut self.login;
                let f = [&mut l.url, &mut l.user, &mut l.pass, &mut self.new_pl];
                *f.into_iter()
                    .nth(i.into())
                    .expect("a login or playlist field") = v;
            }
            Message::Desc(a) => self
                .new_pl_desc
                .as_mut()
                .expect("an open editor")
                .perform(a),
            Message::LSubmit if self.login.busy => {}
            Message::LSubmit => {
                let l = &mut self.login;
                if let Some(n) = [&l.url, &l.user].iter().position(|v| v.trim().is_empty()) {
                    l.error = ["Enter the server address.", "Enter the username."][n].into();
                    return op::focus(format!("login{n}"));
                }
                (l.busy, l.error) = (true, String::new());
                return login(l.url.clone(), l.user.clone(), l.pass.clone());
            }
            Message::LogOut => {
                let gone = self.cfg.active.take().map(|id| {
                    self.cfg.servers.retain(|s| s.id != id);
                    blocking("", move || config::forget(&id))
                });
                let q = Task::batch(self.loaded.then(|| self.save_queue()));
                let h = self.reset();
                self.session = None;
                let (t, n) = (self.persist(), self.nav("login".into()));
                return Task::batch([q, h, t, n].into_iter().chain(gone));
            }
            Message::UseServer(id) => {
                let Some(srv) = self.cfg.servers.iter().find(|s| s.id == id).cloned() else {
                    return Task::none();
                };
                let q = Task::batch(self.loaded.then(|| self.save_queue()));
                let h = self.reset();
                self.login.busy = false;
                (self.session, self.cfg.active) = (Some(Session::new(srv)), Some(id));
                let (t, n) = (self.persist(), self.nav("home".into()));
                return Task::batch([q, h, t, n, self.restore()]);
            }
            Message::DropServer(id) => {
                self.cfg.servers.retain(|s| s.id != id);
                let gone = blocking("", move || config::forget(&id));
                return Task::batch([self.persist(), gone]);
            }
            Message::Do(Act(f)) => return f(self),
            Message::ClearCache => {
                let want = std::mem::take(&mut self.covers.want);
                (self.covers, self.covers.want) = (Covers::default(), want);
                let rm = tokio::fs::remove_dir_all(crate::api::cache_dir(""));
                return Task::perform(rm, |r| match r {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        say(format!("Could not clear the cache. {e}"))
                    }
                    _ => Message::Noop,
                })
                .chain(self.disk(None));
            }
            Message::Close => {
                let q = self.loaded.then(|| self.save_queue());
                let save = Task::batch(q.into_iter().chain([self.persist(), self.save_data()]));
                return save.collect().then(|_| iced::exit());
            }
            Message::Window(false) if self.tray.is_none() || !self.cfg.settings.tray => {
                let min = self.win.map(|id| iced::window::minimize(id, true));
                return Task::batch(min);
            }
            Message::Window(show) => {
                use iced::window::{self as w, Mode};
                let Some(id) = self.win else {
                    self.hidden = false;
                    return self.open();
                };
                if show {
                    return w::set_mode(id, Mode::Windowed).chain(w::gain_focus(id));
                }
                (self.hidden, self.full) = (true, false);
                self.win = self.win.filter(|_| cfg!(windows));
                let close = match cfg!(windows) {
                    true => w::set_mode(id, Mode::Hidden),
                    false => w::close(id),
                };
                let q = self.loaded.then(|| self.save_queue());
                return Task::batch(
                    q.into_iter()
                        .chain([self.persist(), self.save_data(), close]),
                );
            }
            Message::Warm(r) => {
                let top = r.strip_prefix("artist:").map(|id| format!("top:{id}"));
                let keys: Vec<_> = std::iter::once(self.key_of(&r)).chain(top).collect();
                return Task::batch(keys.iter().map(|k| self.request(k, 0)).collect::<Vec<_>>());
            }
            Message::PlayCtx(route, mode) => {
                self.menu = None;
                let mode = Some(mode).filter(|&m| m > 0);
                let mode = mode.unwrap_or(self.cfg.settings.play_mode);
                self.play_seq += u64::from(mode < 2);
                let (key, seq) = (route.clone(), self.play_seq);
                let play = move |a: &mut App, v, src| match mode < 2 {
                    true if a.play_seq != seq => Task::none(),
                    true => {
                        a.ctx.clone_from(&key);
                        a.play_songs(v, src, mode)
                    }
                    false => a.play_songs(v, src, mode),
                };
                return self.ctx_songs(route, "Nothing to play.", "Could not play.", play);
            }
            Message::LibMenu(route) => {
                let kind = match route.split_once(':') {
                    Some(("playlist", id)) => self.playlist(id).cloned().map(MenuKind::Playlist),
                    Some(("album", id)) => {
                        self.find_album(id).map(|a| MenuKind::Album(Box::new(a)))
                    }
                    Some(("artist", id)) => self.find_artist(id).cloned().map(MenuKind::Artist),
                    _ => None,
                };
                return kind.map_or_else(Task::none, |k| self.update(Message::Menu(Box::new(k))));
            }
            Message::AddCtx(route, pid) => {
                self.menu = None;
                let add = move |a: &mut App, v: Vec<Song>, src| {
                    let ids = v.into_iter().map(|s| s.id).collect();
                    a.update(match pid.is_empty() {
                        true => Message::PlNew(src, ids),
                        false => Message::PlAdd(pid.clone(), ids),
                    })
                };
                return self.ctx_songs(route, "Nothing to add.", "Could not add the songs.", add);
            }
            Message::RowMenu(key, n) => {
                if !self.sel.as_ref().is_some_and(|s| s.0 == key) || !self.picked.contains(&n) {
                    (self.sel, self.picked, self.anchor) = (Some((key.clone(), n)), [n].into(), n);
                }
                let Some((list, _)) = self.songs_of(&key) else {
                    return Task::none();
                };
                let row = |&i: &usize| list.get(i).is_some_and(|s| !s.is_dir);
                let idx: Vec<usize> = self.picked.iter().copied().filter(row).collect();
                let songs = idx.iter().map(|&i| list[i].clone()).collect();
                let rm = || Message::PlRemove(idx.iter().map(|&i| self.orig(&key, i)).collect());
                let kind = match self.mine_pl(&key) {
                    true => MenuKind::SongIn(songs, "Remove from this playlist", rm()),
                    false => MenuKind::Song(songs),
                };
                return self.update(Message::Menu(Box::new(kind)));
            }
            Message::SelectAll => {
                if let Some((list, _)) = self.songs_of(&self.route)
                    && let Some(first) = list.iter().position(|s| !s.is_dir)
                {
                    let all = (0..list.len()).filter(|&i| !list[i].is_dir).collect();
                    (self.sel, self.picked, self.anchor) =
                        (Some((self.route.clone(), first)), all, first);
                }
            }
            Message::DeleteSel => {
                let mine = self.sel.as_ref().is_some_and(|s| s.0 == self.route);
                if mine && self.mine_pl(&self.route) {
                    let idx = self
                        .picked
                        .iter()
                        .map(|&i| self.orig(&self.route, i))
                        .collect();
                    return self.update(Message::PlRemove(idx));
                }
            }
            Message::PlayAt(key, n) => {
                let Some((list, src)) = self.songs_of(&key) else {
                    return self.update(Message::PlayCtx(key, 0));
                };
                if list.get(n).is_none_or(|s| s.is_dir) {
                    return Task::none();
                }
                let n = n - list[..n].iter().filter(|s| s.is_dir).count();
                let list: Vec<Song> = list.iter().filter(|s| !s.is_dir).cloned().collect();
                let m = self.cfg.settings.play_mode;
                if let 2 | 3 = m {
                    return self.play_songs(vec![list[n].clone()], src, m);
                }
                if !self
                    .data::<Page<Song>>(&key)
                    .is_some_and(|p| p.0.len() < p.1)
                {
                    self.ctx = key;
                    return self.start(list, src, n, m == 1 || self.shuffle.is_some());
                }
                self.play_seq += 1;
                let (seq, k) = (self.play_seq, key.clone());
                let play = move |a: &mut App, v: Vec<Song>, src: String| {
                    if a.play_seq != seq {
                        return Task::none();
                    }
                    let id = &list[n].id;
                    let at = v.get(n).filter(|s| &s.id == id).map(|_| n);
                    let at = at.or_else(|| v.iter().position(|s| &s.id == id));
                    let (v, at) = at.map_or_else(|| (list.clone(), n), |at| (v, at));
                    a.ctx.clone_from(&k);
                    let sh = m == 1 || a.shuffle.is_some();
                    a.start(v, src, at, sh)
                };
                return self.ctx_songs(key, "Nothing to play.", "Could not play.", play);
            }
            Message::Arrow(up) if self.menu.is_some() => return self.focus_move(up),
            Message::Arrow(_) if self.modal.is_some() => {}
            Message::Arrow(up) => {
                use crate::pages::DIR_Y;
                let Some((key, n)) = self.sel.clone() else {
                    return Task::none();
                };
                let Some(list) = self.songs_of(&key).map(|x| x.0).filter(|l| !l.is_empty()) else {
                    return Task::none();
                };
                let (step, n) = (|&i: &usize| !list[i].is_dir, n.min(list.len() - 1));
                let n = match up {
                    true => (0..n).rev().find(step),
                    false => (n + 1..list.len()).find(step),
                }
                .unwrap_or(n);
                let multi = list.iter().any(|s| s.disc != list[0].disc);
                let pairs = list[..=n].windows(2);
                let discs = pairs.filter(|w| w[0].disc != w[1].disc).count();
                self.pick(key.clone(), n);
                let (kind, _) = key.split_once(':').unwrap_or((&key, ""));
                if key != self.route
                    || !matches!(kind, "album" | "playlist" | "liked" | "songs" | "folder")
                {
                    return Task::none();
                }
                let more = self
                    .cache
                    .get(&key)
                    .and_then(|(_, v)| {
                        pager!(v, p => (p.0.len() < p.1 && n + 20 >= p.0.len()).then_some(p.0.len()))
                    })
                    .map(|off| self.request(&key, off));
                let discs = (1 + discs) * usize::from(multi && kind == "album");
                let top = match kind {
                    "folder" => DIR_Y,
                    _ => crate::pages::list_y(self),
                };
                let top = top + (n + discs) as f32 * self.row_h();
                let (off, view) = self.scroll();
                let to = match () {
                    _ if top < off + 100.0 => top - 100.0,
                    _ if top + self.row_h() > off + view => top + self.row_h() - view,
                    _ => return Task::batch(more),
                };
                self.scrolls.insert(key, (to.max(0.0), view));
                return Task::batch(
                    [self.scroll_to(to.max(0.0)), self.covers_now()]
                        .into_iter()
                        .chain(more),
                );
            }
            Message::Resize(w, h) if w == 0.0 || h == 0.0 => {}
            Message::Resize(w, h) => {
                let s = self.scale();
                self.cfg.settings.window = (w * s, h * s);
                let h = self.view_size().1;
                self.scrolls.values_mut().for_each(|s| s.1 = h - 152.0);
                return self.covers_now();
            }
            Message::LibScrolled(off, view) => {
                let old = crate::view::lib_rows(self).1;
                self.lib_scroll = (off, view);
                if crate::view::lib_rows(self).1 != old {
                    return self.covers_now();
                }
            }
            Message::Modal(id, kind) => {
                self.menu = None;
                if kind == 5 {
                    let load = self.request(&format!("song:{id}"), 0);
                    self.modal = Some((id, kind));
                    return load;
                }
                if kind == 6 {
                    let load = self.request(&format!("artist:{id}"), 0);
                    self.modal = Some((id, kind));
                    return Task::batch([load, self.covers_now()]);
                }
                let Some(p) = self.playlist(&id) else {
                    return self.toast("The playlist is not available.");
                };
                let cover = (kind == 1).then(|| (p.cover.clone(), self.bucket(180.0)));
                if kind == 1 {
                    (self.new_pl, self.new_pl_desc, self.new_pl_public) = (
                        p.name.clone(),
                        Some(text_editor::Content::with_text(&p.comment)),
                        p.public,
                    );
                }
                self.modal = Some((id, kind));
                let all = op::select_all(iced::widget::Id::new("layer"));
                let focus = layer_focus(true, |_, n| n.min(2) - 1).chain(all);
                return Task::batch([self.want_covers(cover), focus]);
            }
            Message::Edit(id, name, comment, public) => {
                (self.menu, self.modal) = (None, None);
                let (name, comment) = (name.trim().to_string(), comment.trim().to_string());
                assert!(!name.is_empty(), "the dialog blocks an empty name");
                let key = format!("playlist:{id}");
                if self.ctx == key {
                    self.source.clone_from(&name);
                }
                let set = |p: &mut Playlist| {
                    (p.name, p.comment, p.public) = (name.clone(), comment.clone(), public)
                };
                self.edit_pl(&id, set);
                self.edit::<(Playlist, Vec<Song>)>(&key, |(p, _)| set(p));
                let undo = reload(&id);
                let edit = |s: Session| async move {
                    let (n, c) = (Some(name.as_str()), Some(comment.as_str()));
                    s.playlist_update(&id, n, c, Some(public), &[], &[]).await
                };
                return self.send(edit, "Could not change the playlist.", undo);
            }
        }
        Task::none()
    }

    pub fn q_drop(&mut self) -> Task<Message> {
        let ok = |&(f, t): &(usize, usize)| {
            f != t && f.min(t) > self.index && f.max(t) < self.queue.len()
        };
        let Some((f, t)) = self.qmove.take().filter(ok) else {
            return Task::none();
        };
        let v = self.shuffle.as_mut();
        match v {
            Some(o) => {
                let x = o.remove(f);
                o.insert(t, x);
            }
            None => {
                let x = self.queue.remove(f);
                self.queue.insert(t, x);
            }
        }
        self.queue_next_refresh();
        self.save_later()
    }

    pub fn q_clear(&mut self) -> Task<Message> {
        let done = self.halt();
        (self.queue, self.index, self.position) = (Vec::new(), 0, 0.0);
        (self.ctx, self.source, self.qmove) = (String::new(), String::new(), None);
        self.shuffle = self.shuffle.as_ref().map(|_| Vec::new());
        Task::batch([done, self.save_later()])
    }

    pub fn retry(&mut self) -> Task<Message> {
        let key = self.key_of(&self.route);
        self.cache.remove(&key);
        self.request(&key, 0)
    }

    fn focus_move(&self, back: bool) -> Task<Message> {
        let layer = self.modal.is_some() || self.menu.is_some();
        layer_focus(layer, move |cur, n| match cur {
            Some(i) => (i + if back { n - 1 } else { 1 }) % n,
            None if back => n - 1,
            None => 0,
        })
    }

    fn logged_in(&mut self, r: Result<Session, String>) -> Task<Message> {
        if !self.login.busy {
            return Task::none();
        }
        let s = match r {
            Ok(s) => s,
            Err(e) => {
                (self.login.busy, self.login.error) = (false, e);
                return Task::none();
            }
        };
        let (id, pass) = (s.server.id.clone(), self.login.pass.clone());
        let key = id.clone();
        let kept = match self.login.remember {
            true => blocking("", move || config::remember(&key, &pass)),
            false => blocking("", move || config::forget(&key)),
        };
        let h = Task::batch(self.session.is_some().then(|| self.reset()));
        self.cfg.servers.retain(|x| x.id != id);
        self.cfg.servers.push(s.server.clone());
        (self.cfg.active, self.session, self.login) = (Some(id), Some(s), Login::default());
        self.route.clear();
        self.history.clear();
        let to = shot_key(self.shot.as_deref()).unwrap_or_else(|| "home".into());
        let (t, n) = (self.persist(), self.nav(to));
        Task::batch([h, t, n, self.restore(), kept])
    }

    fn expired(&mut self, key: String) -> Task<Message> {
        self.pending.retain(|p| !p.starts_with(&format!("{key}#")));
        let Some(srv) = self.session.as_ref().map(|s| s.server.clone()) else {
            return Task::none();
        };
        self.expired.0.push(key);
        if self.expired.0.len() > 1 {
            return Task::none();
        }
        let (id, ok) = (
            srv.id.clone(),
            self.expired.1.is_none_or(|t| t.elapsed() > STALE),
        );
        let again = async move {
            anyhow::ensure!(ok, "The server refused the new session.");
            let pass = tokio::task::spawn_blocking(move || config::recall(&srv.id));
            match pass.await?? {
                Some(p) => Session::login(&srv.url, &srv.user, &p).await.map(Some),
                None => Ok(None),
            }
        };
        Task::perform(again, move |r| {
            let r = r.map_err(|e| e.to_string());
            task(move |a| {
                if a.session.as_ref().is_none_or(|s| s.server.id != id) {
                    return Task::none();
                }
                let keys = std::mem::take(&mut a.expired.0);
                let cur = a.session.as_mut().expect("a session");
                let note = match r.clone() {
                    Ok(Some(s)) => {
                        cur.renew(s.server);
                        let saved = a.cfg.servers.iter_mut().find(|v| v.id == id);
                        saved.into_iter().for_each(|v| *v = cur.server.clone());
                        a.expired.1 = Some(Instant::now());
                        None
                    }
                    Ok(None) => Some(
                        "The server session expired. Log in again to use all functions.".into(),
                    ),
                    Err(e) => Some(format!("Could not log in again. {e}")),
                };
                cur.server.native &= note.is_none();
                let toast = note.map(|n| a.toast(&n));
                keys.iter().for_each(|k| drop(a.cache.remove(k)));
                let load = keys.iter().map(|k| a.request(k, 0)).collect::<Vec<_>>();
                Task::batch(load.into_iter().chain(toast).chain([a.persist()]))
            })
        })
    }

    fn win(&mut self, id: iced::window::Id) -> Task<Message> {
        use iced::window::raw_window_handle::RawWindowHandle;
        use iced::window::{self as w, Mode};
        self.win = Some(id);
        if self.tray.is_none() {
            let tray = crate::tray::spawn(CH.0.clone());
            let tray = tray.inspect_err(|e| eprintln!("The tray icon is not available. {e}"));
            self.tray = tray.ok();
        }
        let scroll = self.scroll_to(self.scrolls.get(&self.route).map_or(0.0, |s| s.0));
        if self.media.is_some() {
            return scroll;
        }
        let hwnd = iced::window::run(id, |w| match w.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        });
        let min = self
            .hidden
            .then(|| match self.tray.is_some() && self.cfg.settings.tray {
                true => Task::done(Message::Window(false)),
                false => w::set_mode(id, Mode::Windowed).chain(w::minimize(id, true)),
            });
        scroll
            .chain(hwnd.map(|h| task(move |a| a.media_init(h))))
            .chain(Task::batch(min))
    }

    fn media_init(&mut self, hwnd: isize) -> Task<Message> {
        let (tx, h) = (CH.0.clone(), hwnd as *mut std::ffi::c_void);
        let c = souvlaki::PlatformConfig {
            display_name: "navify",
            dbus_name: "navify",
            hwnd: (hwnd != 0).then_some(h),
        };
        let m = souvlaki::MediaControls::new(c);
        let send = move |e: souvlaki::MediaControlEvent| {
            drop(tx.send(task(move |a| a.media(e.clone()))));
        };
        match m.and_then(|mut m| m.attach(send).map(|()| m)) {
            Ok(m) => {
                (self.media, self.os) = (Some(m), Default::default());
                Task::none()
            }
            Err(e) => self.toast(&format!("Media controls are not available. {e:?}")),
        }
    }

    fn media(&mut self, e: souvlaki::MediaControlEvent) -> Task<Message> {
        use souvlaki::{MediaControlEvent as E, SeekDirection as D};
        let dir = |d: D| [-1.0, 1.0][usize::from(matches!(d, D::Forward))];
        let m = match e {
            E::Toggle => Message::Toggle,
            E::Play if !self.playing => Message::Toggle,
            E::Pause | E::Stop if self.playing => Message::Toggle,
            E::Next => Message::Next(true),
            E::Previous => Message::Prev,
            E::Seek(d) => Message::Step(false, dir(d)),
            E::SeekBy(d, t) => Message::Seek((self.pos() + dir(d) * t.as_secs_f32()).max(0.0)),
            E::SetPosition(p) => Message::Seek(p.0.as_secs_f32()),
            E::SetVolume(v) => Message::Vol(v.clamp(0.0, 1.0) as f32),
            E::Quit => Message::Close,
            E::Raise => Message::Window(true),
            _ => return Task::none(),
        };
        self.update(m)
    }

    fn audio(&mut self, e: navify_audio::Event) -> Task<Message> {
        match e {
            navify_audio::Event::Started(id) => {
                let done = self.listen(false);
                let next = self.next_index(false);
                let n =
                    next.filter(|&n| self.queue.get(self.real_at(n)).is_some_and(|s| s.id == id));
                self.index = n.unwrap_or(self.index);
                let began = Task::batch(n.map(|_| self.began()));
                Task::batch([done, began, self.listen(self.playing)])
            }
            navify_audio::Event::Ended => self.update(Message::Next(false)),
            navify_audio::Event::Error(e) => {
                let done = self.listen(false);
                (self.position, self.playing, self.loaded) = (self.pos(), false, false);
                Task::batch([done, self.toast(&format!("Playback failed. {e}"))])
            }
        }
    }

    fn ctx_songs(
        &mut self,
        route: String,
        empty: &'static str,
        fail: &'static str,
        then: impl Fn(&mut App, Vec<Song>, String) -> Task<Message> + Clone + Send + Sync + 'static,
    ) -> Task<Message> {
        let part = self.data::<Page<Song>>(&route);
        let part = part.is_some_and(|p| p.0.len() < p.1);
        if let Some((v, src)) = self
            .songs_of(&route)
            .filter(|(v, _)| !v.is_empty() && !part)
        {
            let v: Vec<Song> = v.iter().filter(|s| !s.is_dir).cloned().collect();
            return match v.is_empty() {
                true => self.toast(empty),
                false => then(self, v, src),
            };
        }
        let load = |s: Session| async move {
            let (kind, id) = route.split_once(':').unwrap_or((&route, ""));
            let all = |sort| s.songs(sort, 0, PAGE * 50);
            let sort = if id.is_empty() { "name" } else { id };
            Ok::<_, anyhow::Error>(match kind {
                "album" => s.album(id).await.map(|(a, v)| (v, a.name))?,
                "playlist" => s.playlist(id).await.map(|(p, v)| (v, p.name))?,
                "artist" => {
                    let ((a, _), t) = tokio::try_join!(s.artist(id), s.top_songs(id))?;
                    (t, a.name)
                }
                "liked" => (all("starred").await?.0, "Liked Songs".into()),
                "songs" => (all(sort).await?.0, "Songs".into()),
                _ => (Vec::new(), String::new()),
            })
        };
        let ok = move |(v, src): (Vec<Song>, String)| match v.is_empty() {
            true => say(empty.into()),
            false => {
                let then = then.clone();
                task(move |a| then(a, v.clone(), src.clone()))
            }
        };
        self.call(load, fail, ok)
    }

    fn play_songs(&mut self, songs: Vec<Song>, src: String, mode: u8) -> Task<Message> {
        if mode >= 2 {
            return self.update(Message::Queue(songs, mode == 3));
        }
        let sh = mode == 1 || self.shuffle.is_some();
        self.start(songs, src, if sh { usize::MAX } else { 0 }, sh)
    }

    fn visible_covers(&self) -> Vec<(String, u32)> {
        let (off, view) = self.scroll();
        let (cols, cw) = self.grid();
        use crate::pages::{DIR_Y, PAD};
        let span = |n: usize, h: f32, per: usize, y0: f32| {
            let vis = (view / h) as usize;
            let lo = (((off - y0) / h).max(0.0) as usize).min(n.div_ceil(per).saturating_sub(vis));
            (lo.saturating_sub(1) * per).min(n)..((lo + vis + 3) * per).min(n)
        };
        let y0 = crate::pages::list_y(self);
        let rows = |v: &[Song]| match self.row_h() < 56.0 {
            true => 0..0,
            false => span(v.len(), 56.0, 1, y0),
        };
        let tiles = |n: usize| span(n, cw + 52.0, cols, crate::pages::grid_y(self));
        let (key, lib) = (self.route.as_str(), self.data::<Lib>("lib"));
        let (mut big, mut small): (Vec<&String>, Vec<&String>) = Default::default();
        let mut wide: [Vec<&String>; 4] = Default::default();
        let (_, gcols, tw, th) = crate::pages::browse(self);
        let (mut head, mut banner, mut about): (Vec<&String>, _, _) = Default::default();
        if let Some((a, _)) = self.data::<(Album, Vec<Song>)>(key) {
            head.push(&a.cover);
            big.extend(crate::pages::more_by(self, a).into_iter().map(|x| &x.cover));
            let art = a
                .artists
                .first()
                .map(|x| &x.cover)
                .filter(|c| !c.is_empty());
            small.push(art.unwrap_or(&a.artist_id));
        } else if let Some((p, s)) = self.data::<(Playlist, Vec<Song>)>(key) {
            let empty = s.is_empty() && p.song_count == 0;
            let empty = empty || (self.panel != 4 && self.pl_more.is_some());
            let s = self.songs_of(key).map_or(&s[..], |x| x.0);
            let q = self.pl_more.as_deref().unwrap_or("").trim();
            let hits = self.data::<Trio>(&format!("search:{q}"));
            let hits = hits.filter(|_| empty && !q.is_empty());
            let hits = hits.iter().flat_map(|t| t.2.iter().take(10));
            small.extend(hits.map(|s| &s.cover));
            head.push(&p.cover);
            small.extend(s[rows(s)].iter().map(|s| &s.cover));
        } else if let Some((s, _)) = self.data::<Page<Song>>(key) {
            small.extend(s[rows(s)].iter().map(|s| &s.cover));
        } else if let Some((_, s)) = self.data::<(String, Vec<Song>)>(key) {
            let s = &s[span(s.len(), self.row_h(), 1, DIR_Y)];
            small.extend(s.iter().filter(|s| !s.is_dir).map(|s| &s.cover));
        } else if key.starts_with("discography:") {
            big.extend(crate::pages::disc_view(self).iter().map(|a| &a.cover));
        } else if let Some((a, albums)) = self.data::<(Artist, Vec<Album>)>(key) {
            let id = &key["artist:".len()..];
            let top = self.data::<Vec<Song>>(&format!("top:{id}"));
            let info = self.data::<Info>(&format!("info:{id}"));
            let appears = self.data::<Vec<Album>>(&format!("appears:{id}"));
            let shelf = crate::pages::disc_shelf(self, albums);
            about = info
                .filter(|i| !i.biography.is_empty())
                .map(|_| (crate::pages::about_key(self, a), 640));
            banner = crate::pages::banner(self, a);
            if banner.is_none() {
                head.push(&a.cover);
            }
            let pop = [5, 10][usize::from(self.popular_all)];
            let top = top.iter().flat_map(|v| v.iter().take(pop));
            small.extend(top.map(|s| &s.cover));
            big.extend(shelf.iter().map(|x| &x.cover));
            let latest = crate::pages::by_date(albums.iter());
            big.extend(latest.first().map(|x| &x.cover));
            let similar = info.map_or(&[][..], |i| &i.similar);
            big.extend(similar.iter().take(cols).map(|x| &x.cover));
            let appears = appears.map_or(&[][..], |v| &v[..]);
            big.extend(appears.iter().take(cols).map(|x| &x.cover));
        } else if let Some((ar, sg)) = self.data::<(Vec<Artist>, Vec<Song>)>(key) {
            small.extend(sg.iter().map(|s| &s.cover));
            big.extend(ar.iter().take(cols).map(|a| &a.cover));
            big.extend(crate::pages::public(self).take(cols).map(|p| &p.cover));
        } else if let Some((p, _)) = self.data::<Page<Album>>(key) {
            big.extend(p[tiles(p.len())].iter().map(|a| &a.cover));
        } else if let Some((p, _)) = self.data::<Page<Artist>>(key) {
            big.extend(p[tiles(p.len())].iter().map(|a| &a.cover));
        } else if let Some((ar, al, sg)) = self.data::<Trio>(key) {
            let q = key.strip_prefix("search:").unwrap_or("");
            let cid = |(k, i): (usize, usize)| {
                [
                    &ar.get(i).map(|a| &a.cover),
                    &al.get(i).map(|a| &a.cover),
                    &sg.get(i).map(|s| &s.cover),
                ][k]
                    .expect("a search hit")
            };
            let pls = lib.map_or(&[][..], |l| &l.playlists[..]);
            let ql = q.to_lowercase();
            let pls = pls.iter().filter(|p| p.name.to_lowercase().contains(&ql));
            let pls: Vec<_> = pls.collect();
            match self.search_tab {
                0 if !ar.is_empty() || !al.is_empty() || !sg.is_empty() => {
                    let (top, mix) = crate::pages::search_hits((ar, al, sg), q);
                    wide[0].extend(top.map(cid));
                    wide[1].extend(mix.into_iter().map(cid));
                }
                1 => small.extend(sg.iter().map(|s| &s.cover)),
                2 => big.extend(al[tiles(al.len())].iter().map(|a| &a.cover)),
                3 => big.extend(ar[tiles(ar.len())].iter().map(|a| &a.cover)),
                _ => big.extend(pls[tiles(pls.len())].iter().map(|p| &p.cover)),
            }
        } else if let Some(gs) = self
            .data::<Vec<crate::api::Genre>>("genres")
            .filter(|_| key == "search")
        {
            let gs = &gs[span(gs.len(), th + 24.0, gcols, 104.0)];
            let art = gs.iter();
            let art = art.filter_map(|g| crate::pages::genre_cover(self, &g.name));
            wide[2].extend(art);
        } else if let Some(h) = self.data::<Home>(key) {
            let pls = lib.map_or(&[][..], |l| &l.playlists[..]);
            let pins = pls.iter().take(3).map(|p| &p.cover);
            wide[3].extend(pins.chain(h[0].iter().map(|a| &a.cover)).take(7));
            let hs = cw + 132.0;
            let (per, _, th, gap) = crate::pages::home_tiles(self);
            let tiles = (1 + pls.len().min(3) + h[0].len()).min(8);
            let y0 = PAD + tiles.div_ceil(per) as f32 * (th + gap) - gap + 40.0;
            let shelves = h.iter();
            let shelves = shelves.map(|l| l.iter().take(cols).map(|a| &a.cover).collect());
            let shelves = shelves.chain([pls.iter().take(cols).map(|p| &p.cover).collect()]);
            let shelves = shelves.filter(|l: &Vec<_>| !l.is_empty()).enumerate();
            for (n, l) in shelves {
                let y = y0 + n as f32 * hs;
                let shown = (off - hs..off + view + hs).contains(&y);
                big.extend(l.into_iter().filter(|_| shown));
            }
        }
        let now = self.current_song().map(|s| &s.cover);
        small.extend(now);
        let ahead = [0, 1, 100, 0, 0][self.panel as usize];
        let add = crate::pages::add_songs(self).filter(|_| self.panel == 4);
        small.extend(add.map(|s| &s.cover));
        let recent = crate::pages::recent(self).filter(|_| self.panel == 2 && self.recent_tab);
        small.extend(recent.map(|s| &s.cover));
        let next = (self.index + 1..self.queue.len()).take(ahead);
        small.extend(next.map(|n| &self.queue[self.real_at(n)].cover));
        let (rows, span) = crate::view::lib_rows(self);
        let (view, _, lw, _) = self.lib_layout();
        let lib = Vec::from_iter(rows[span].iter().filter(|_| view > 0).map(|r| r.0));
        let lib_px = [48.0, 48.0, lw - 8.0, lw - 24.0][view as usize];
        let now = Vec::from_iter(now.filter(|_| self.panel == 1 || self.full));
        let side = [self.right_w() - 32.0, 300.0][usize::from(self.full)];
        let lead = self.lead_artist().filter(|_| self.panel == 1 && !self.full);
        let lead = lead.map(|id| (format!("w:{}", self.artist_cover(id)), self.bucket(side)));
        let art = crate::pages::art_px(self);
        let [top, rows, genre, tiles] = wide;
        let lists = [
            (head, art),
            (tiles, crate::pages::home_tiles(self).2),
            (big, cw - 24.0),
            (lib, lib_px),
            (small, 56.0),
            (now, side),
            (top, 72.0),
            (rows, 64.0),
            (genre, (tw * 0.45).round()),
        ];
        let lists = lists.into_iter().map(|(v, px)| (v, self.bucket(px)));
        banner
            .filter(|k| !k.is_empty())
            .map(|k| (k, 640))
            .into_iter()
            .chain(lists.flat_map(|(v, b)| v.into_iter().map(move |c| (c.clone(), b))))
            .chain(lead)
            .chain(about)
            .chain(self.modal.as_ref().and_then(|(id, k)| match k {
                1 => Some((self.playlist(id)?.cover.clone(), self.bucket(180.0))),
                6 => Some((format!("b18:{}", self.artist_cover(id)), 640)),
                _ => None,
            }))
            .collect()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        let lyr = crate::pages::lyric_line(self);
        crate::view::smooth(crate::view::view(self), self.cfg.settings.smooth, lyr)
    }

    pub fn title(&self) -> String {
        let s = self.current_song().filter(|_| self.playing);
        s.map_or_else(
            || "Navify".into(),
            |s| format!("{} \u{2022} {}", s.title, s.artist),
        )
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        use iced::keyboard::Event as K;
        use iced::mouse::{Button, Event as M};
        use iced::{Event, event, window::Event as W};
        let mut subs = vec![
            event::listen_with(|e, status, _| match e {
                Event::Window(W::Focused) => Some(act(|a| a.hidden = false)),
                Event::Window(W::Unfocused) => Some(task(|_| {
                    let min = |m: Option<bool>| {
                        task(move |a| {
                            a.hidden = m == Some(true);
                            let tray = a.hidden && a.cfg.settings.min_tray && a.tray.is_some();
                            Task::batch(tray.then(|| a.update(Message::Window(false))))
                        })
                    };
                    later(0.3, Message::Noop)
                        .then(|_| iced::window::latest())
                        .and_then(iced::window::is_minimized)
                        .map(min)
                })),
                Event::Window(W::CloseRequested) => {
                    Some(task(|a| match a.tray.is_some() && a.cfg.settings.tray {
                        true => a.update(Message::Window(false)),
                        false => a.update(Message::Close),
                    }))
                }
                Event::Window(W::Resized(s)) => Some(Message::Resize(s.width, s.height)),
                Event::Keyboard(K::KeyPressed { key, modifiers, .. }) => {
                    keys(&key, modifiers, status)
                }
                Event::Mouse(M::ButtonPressed(Button::Back)) => Some(Message::Back),
                Event::Mouse(M::ButtonPressed(Button::Forward)) => Some(Message::Fwd),
                _ => None,
            }),
            iced::Subscription::run(|| {
                iced::stream::channel(8, async |mut tx| {
                    let rx = CH.1.lock().expect("event channel").take();
                    let mut rx = rx.expect("one listener");
                    while let Some(m) = rx.recv().await {
                        let _ = iced::futures::SinkExt::send(&mut tx, m).await;
                    }
                })
            }),
        ];
        if self.drag.is_some() {
            subs.push(event::listen_with(|e, _, _| match e {
                Event::Mouse(M::CursorMoved { position: p }) => Some(act(move |a| {
                    let (vw, s) = (a.view_size().0, &mut a.cfg.settings);
                    match a.drag {
                        Some(0) if p.x < 208.0 => s.lib_collapsed = true,
                        Some(0) => {
                            (s.panels.0, s.lib_collapsed) = ((p.x - 8.0).clamp(280.0, 480.0), false)
                        }
                        _ => s.panels.1 = (vw - 8.0 - p.x).clamp(280.0, 420.0),
                    }
                })),
                Event::Mouse(M::ButtonReleased(_)) => Some(Message::Drag(None)),
                _ => None,
            }));
        }
        if self.qmove.is_some() {
            subs.push(event::listen_with(|e, _, _| {
                matches!(e, Event::Mouse(M::ButtonReleased(_))).then(|| task(App::q_drop))
            }));
        }
        if self.playing
            && !self.hidden
            && (self.route == "lyrics" || self.panel == 1)
            && let Some(ms) = crate::pages::lyric_wait(self)
        {
            let tick = iced::time::every(Duration::from_millis(ms));
            subs.push(tick.map(|_| act(|a| a.position = a.pos())));
        }
        iced::Subscription::batch(subs)
    }
}

fn keys(key: &Key, m: Modifiers, status: iced::event::Status) -> Option<Message> {
    use iced::keyboard::key::Named::{Alt, Control, Meta, Shift, Super};
    let k = match key {
        Key::Named(Shift | Control | Alt | Super | Meta) | Key::Unidentified => return None,
        Key::Named(n) => format!("{n:?}"),
        Key::Character(c) => c.to_uppercase(),
    };
    let mods = [
        (m.command(), "C+"),
        (m.alt(), "Alt+"),
        (m.shift(), "Shift+"),
    ];
    let c = mods.iter().filter(|x| x.0).map(|x| x.1).collect::<String>() + &k;
    let alt = m.alt() && matches!(key, Key::Named(_)) && !cfg!(target_os = "macos");
    match (c.as_str(), status) {
        ("Escape", _) => Some(Message::MenuClose),
        (_, iced::event::Status::Captured) if !alt => None,
        _ => Some(Message::Key(c)),
    }
}

pub fn key_label(c: &str) -> String {
    let cmd = ["Ctrl+", "Cmd+"][usize::from(cfg!(target_os = "macos"))];
    let c = c.replace("C+", cmd).replace("ArrowLeft", "\u{2190}");
    let c = c.replace("ArrowUp", "\u{2191}");
    let c = c.replace("ArrowRight", "\u{2192}");
    c.replace("ArrowDown", "\u{2193}")
}

pub type KeyAction = (&'static str, &'static str, &'static str, fn() -> Message);

pub const KEY_GROUPS: [(&str, usize); 4] = [
    ("Basic", 10),
    ("Playback", 10),
    ("Navigation", 4),
    ("Layout", 3),
];

pub const KEYS: [KeyAction; 27] = [
    ("new_playlist", "Create new playlist", "C+N", || {
        Message::PlNew(String::new(), Vec::new())
    }),
    ("search", "Search", "C+K", || {
        task(|a| {
            let go = !a.route.starts_with("search");
            let go = go.then(|| a.update(Message::Nav("search".into())));
            Task::batch(go.into_iter().chain([op::focus("search")]))
        })
    }),
    ("select_all", "Select all", "C+A", || Message::SelectAll),
    ("filter", "Filter", "C+F", || {
        task(|a| {
            let id = match a.route.as_str() {
                "settings" => "setfind",
                r if r.starts_with("playlist:") => "plfind",
                _ => return Task::none(),
            };
            a.pl_find.get_or_insert_default();
            Task::batch([a.scroll_to(0.0), op::focus(id)])
        })
    }),
    ("lib_find", "Search in Your Library", "C+Alt+F", || {
        task(|a| {
            a.cfg.settings.lib_collapsed = false;
            a.lib_find.get_or_insert_default();
            Task::batch([a.covers_now(), op::focus("libfind")])
        })
    }),
    ("settings", "Settings", "C+,", || {
        Message::Nav("settings".into())
    }),
    ("shortcuts", "Keyboard shortcuts", "C+/", || {
        act(|a| match a.modal {
            Some((_, 3)) => a.modal = None,
            _ => a.modal = Some((String::new(), 3)),
        })
    }),
    ("close", "Close the window", "C+W", || {
        Message::Window(false)
    }),
    ("quit", "Quit", "C+Q", || Message::Close),
    ("log_out", "Log out", "Alt+Shift+F6", || Message::LogOut),
    ("toggle", "Play or pause", "Space", || Message::Toggle),
    ("like", "Like the current song", "Alt+Shift+B", || {
        task(|a| {
            let s = a.current_song().filter(|s| s.url.is_empty());
            let s = s.map(|s| (s.id.clone(), !a.is_starred(&s.id, s.starred)));
            Task::batch(s.map(|(id, on)| a.update(Message::Star(vec![id], on))))
        })
    }),
    ("shuffle", "Shuffle", "C+S", || Message::Shuffle),
    ("repeat", "Repeat", "C+R", || act(App::repeat)),
    ("prev", "Previous track", "C+ArrowLeft", || Message::Prev),
    ("next", "Next track", "C+ArrowRight", || Message::Next(true)),
    ("seek_back", "Seek backward", "Shift+ArrowLeft", || {
        Message::Step(false, -1.0)
    }),
    ("seek_fwd", "Seek forward", "Shift+ArrowRight", || {
        Message::Step(false, 1.0)
    }),
    ("vol_up", "Volume up", "C+ArrowUp", || {
        Message::Step(true, 1.0)
    }),
    ("vol_down", "Volume down", "C+ArrowDown", || {
        Message::Step(true, -1.0)
    }),
    ("home", "Home", "Alt+Shift+H", || {
        Message::Nav("home".into())
    }),
    ("back", "Go back", "Alt+ArrowLeft", || Message::Back),
    ("forward", "Go forward", "Alt+ArrowRight", || Message::Fwd),
    ("liked", "Liked Songs", "Alt+Shift+S", || {
        Message::Nav("liked".into())
    }),
    ("library", "Collapse Your Library", "Alt+Shift+L", || {
        task(App::lib_collapse)
    }),
    ("now_playing", "Now playing view", "Alt+Shift+J", || {
        Message::Panel(1)
    }),
    ("queue", "Queue", "Alt+Shift+Q", || Message::Panel(2)),
];

fn decode(
    raw: bytes::Bytes,
    k: &str,
    size: u32,
    color: Option<Color>,
    file: &std::path::Path,
) -> Result<Decoded, String> {
    use {::image::ImageReader, ::image::imageops::FilterType, std::hash::BuildHasher};
    let wide = k.strip_prefix('b').and_then(|r| r.split_once(':'));
    let wide = wide.and_then(|(n, _)| n.parse::<f32>().ok());
    let (size, round) = (
        if wide.is_some() { 1280 } else { size },
        k.starts_with("o:"),
    );
    let aspect = wide.map_or(5.0 / 3.0 * f32::from(k.starts_with("w:")), |n| n / 10.0);
    let png = raw.starts_with(b"\x89PNG");
    if aspect == 0.0
        && !round
        && !png
        && let Some(c) = color
        && let Some((w, h)) = ImageReader::new(std::io::Cursor::new(&raw))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok())
        && w <= size
        && h <= size
    {
        return Ok((c, image::Handle::from_bytes(raw.clone()), raw.len()));
    }
    static BIG: std::sync::Mutex<Vec<(u64, Decoded)>> = std::sync::Mutex::new(Vec::new());
    let hash = std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default();
    let key = hash.hash_one((&raw[..], size, round, aspect.to_bits()));
    let save = |img: &::image::RgbaImage, big: bool| {
        use ::image::{RgbImage, buffer::ConvertBuffer, codecs::jpeg::JpegEncoder};
        let (mut out, tmp) = (std::io::Cursor::new(Vec::new()), file.with_extension("tmp"));
        match img.pixels().all(|p| p[3] == 255) {
            true => ConvertBuffer::<RgbImage>::convert(img)
                .write_with_encoder(JpegEncoder::new_with_quality(&mut out, 90))
                .ok()?,
            false if big => img.write_to(&mut out, ::image::ImageFormat::Png).ok()?,
            false => return None,
        }
        std::fs::write(&tmp, out.get_ref()).ok()?;
        std::fs::rename(&tmp, file).ok()?;
        Some(bytes::Bytes::from(out.into_inner()))
    };
    if let Some((_, d)) = BIG.lock().expect("cover lock").iter().find(|e| e.0 == key) {
        if !round
            && let image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            } = &d.1
        {
            let img = ::image::RgbaImage::from_raw(*width, *height, pixels.to_vec());
            save(&img.expect("the pixels fit the size"), true);
        }
        return Ok(d.clone());
    }
    let mut img = ::image::load_from_memory(&raw).map_err(|e| e.to_string())?;
    if aspect > 0.0 {
        let h = ((img.width() as f32 / aspect) as u32).min(img.height());
        let w = ((h as f32 * aspect) as u32).min(img.width());
        img = img.crop_imm((img.width() - w) / 2, (img.height() - h) * 3 / 10, w, h);
    }
    let big = img.width() > size || img.height() > size;
    if big {
        img = img.thumbnail(size, size);
    }
    let color = color.unwrap_or_else(|| {
        let hsl = |[r, g, b]: [f32; 3]| {
            let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
            let (d, l) = (mx - mn, (mx + mn) / 2.0);
            let h = match () {
                _ if d == 0.0 => 0.0,
                _ if mx == r => ((g - b) / d).rem_euclid(6.0),
                _ if mx == g => (b - r) / d + 2.0,
                _ => (r - g) / d + 4.0,
            };
            (h * 60.0, d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6), l)
        };
        let ok = |c: [f32; 3]| {
            let (h, s, l) = hsl(c);
            l > 0.05 && l < 0.95 && !((10.0..=37.0).contains(&h) && s <= 0.82)
        };
        let rgb = |q: [u8; 3]| q.map(|c| f32::from(c << 3) / 255.0);
        let mut hist = std::collections::BTreeMap::<[u8; 3], u32>::new();
        for p in img
            .resize_exact(64, 64, FilterType::Nearest)
            .to_rgb8()
            .pixels()
        {
            *hist.entry(p.0.map(|c| c >> 3)).or_default() += 1;
        }
        let span = |b: &[([u8; 3], u32)], i: usize| {
            let v = b.iter().map(|x| x.0[i]);
            v.clone().max().unwrap_or(0) - v.min().unwrap_or(0)
        };
        let mut boxes = vec![
            hist.into_iter()
                .filter(|x| ok(rgb(x.0)))
                .collect::<Vec<_>>(),
        ];
        while boxes.len() < 16 {
            let vol = |b: &Vec<_>| (0..3).map(|i| u32::from(span(b, i)) + 1).product::<u32>();
            let k = (0..boxes.len()).max_by_key(|&k| vol(&boxes[k]));
            let k = k.expect("a box");
            let mut b = boxes.swap_remove(k);
            if b.len() < 2 {
                boxes.push(b);
                break;
            }
            let d = (0..3).max_by_key(|&i| span(&b, i)).expect("three channels");
            b.sort_by_key(|x| x.0[d]);
            let (all, mut acc) = (b.iter().map(|x| x.1).sum::<u32>(), 0);
            let at = b.iter().position(|x| {
                acc += x.1;
                2 * acc >= all
            });
            let rest = b.split_off(at.expect("a median").min(b.len() - 2) + 1);
            boxes.extend([b, rest]);
        }
        let sw: Vec<([f32; 3], f32)> = boxes
            .iter()
            .filter(|b| !b.is_empty())
            .map(|b| {
                let n = b.iter().map(|x| x.1).sum::<u32>();
                let avg = [0, 1, 2].map(|i| {
                    let t: u32 = b.iter().map(|x| u32::from(x.0[i]) * x.1).sum();
                    ((t + n / 2) / n) as u8
                });
                (rgb(avg), n as f32)
            })
            .filter(|w| ok(w.0))
            .collect();
        let bright = |c| hsl(c).2 >= 0.8 && hsl(c).1 < 0.25;
        let top = sw
            .iter()
            .filter(|w| !bright(w.0))
            .map(|w| w.1)
            .fold(0.0, f32::max);
        let pick = |l: [f32; 3], s: [f32; 3]| {
            let fit = sw
                .iter()
                .filter(|w| w.1 >= top * 0.1)
                .filter_map(|&(c, n)| {
                    let (_, cs, cl) = hsl(c);
                    let fit = (s[0]..=s[2]).contains(&cs) && (l[0]..=l[2]).contains(&cl);
                    let score = (1.0 - (cs - s[1]).abs()) * 0.24 + (1.0 - (cl - l[1]).abs()) * 0.52;
                    fit.then_some((score + (n / top).min(1.0) * 0.24, c))
                });
            fit.max_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1)
        };
        pick([0.3, 0.5, 0.7], [0.35, 1.0, 1.0])
            .or_else(|| pick([0.0, 0.26, 0.45], [0.35, 1.0, 1.0]))
            .or_else(|| pick([0.55, 0.74, 1.0], [0.35, 1.0, 1.0]))
            .or_else(|| pick([0.15, 0.26, 0.45], [0.05, 0.3, 0.4]))
            .map_or(Color::from_rgb8(83, 83, 83), |c| {
                let [r, g, b] = c.map(|x| x - (hsl(c).2 - 0.5).max(0.0));
                Color::from_rgb(r, g, b)
            })
    });
    if aspect == 0.0 && !big && !round {
        let raw = png
            .then(|| save(&img.into_rgba8(), false))
            .flatten()
            .unwrap_or(raw);
        return Ok((color, image::Handle::from_bytes(raw.clone()), raw.len()));
    }
    let mut img = img.into_rgba8();
    if big && aspect == 0.0 && !round {
        save(&img, true);
    }
    let (w, h) = img.dimensions();
    let r = w.min(h) as f32 / 2.0;
    for (x, y, p) in img.enumerate_pixels_mut().filter(|_| round) {
        let d = (x as f32 + 0.5 - w as f32 / 2.0).hypot(y as f32 + 0.5 - h as f32 / 2.0);
        p.0[3] = (p.0[3] as f32 * (r - d + 0.5).clamp(0.0, 1.0)) as u8;
    }
    let d = (
        color,
        image::Handle::from_rgba(w, h, img.into_raw()),
        (w * h * 4) as usize,
    );
    if big && aspect == 0.0 && size <= 300 {
        let mut all = BIG.lock().expect("cover lock");
        all.truncate(3);
        all.insert(0, (key, d.clone()));
    }
    Ok(d)
}

fn fetch(s: Session, key: String, offset: usize) -> Task<Message> {
    let (k, me) = (key.clone(), s.clone());
    let load = async move {
        let (kind, arg) = k.split_once(':').unwrap_or((&k, ""));
        let or = |d: &'static str| if arg.is_empty() { d } else { arg };
        Ok::<AnyData, anyhow::Error>(match kind {
            "home" => {
                let l = |t| s.albums(t, &[], 0, 12);
                let (r, a, f, x) =
                    tokio::try_join!(l("recent"), l("newest"), l("frequent"), l("random"))?;
                Arc::new([r.0, a.0, f.0, x.0]) as AnyData
            }
            "lib" => {
                let (playlists, (artists, albums, songs)) =
                    tokio::try_join!(s.playlists(), s.starred())?;
                Arc::new(Lib {
                    playlists,
                    artists,
                    albums,
                    liked: songs.len(),
                })
            }
            "album" => Arc::new(s.album(arg).await?),
            "artist" => Arc::new(s.artist(arg).await?),
            "top" => Arc::new(s.top_songs(arg).await?),
            "info" => Arc::new(s.artist_info(arg).await?),
            "appears" => Arc::new(s.appears(arg).await?),
            "banner" => {
                let url = s.banner(arg).await.unwrap_or_default();
                if !url.is_empty() {
                    let _ = s.cover(&url, 640).await;
                }
                Arc::new(url)
            }
            "playlist" => Arc::new(s.playlist(arg).await?),
            "liked" => Arc::new(s.songs("starred", offset, PAGE).await?),
            "albums" => Arc::new(s.albums(or("newest"), &[], offset, PAGE).await?),
            "artists" => Arc::new(s.artists(offset, PAGE).await?),
            "songs" => Arc::new(s.songs(or("name"), offset, PAGE).await?),
            "genre" => Arc::new(s.albums("newest", &[("genre", arg)], offset, PAGE).await?),
            "genres" => Arc::new(s.genres().await?),
            "folders" => Arc::new(s.indexes().await?),
            "radio" => Arc::new(s.radios().await?),
            "folder" => Arc::new(s.directory(arg).await?),
            "lyrics" => Arc::new(s.lyrics(arg).await?),
            "song" => Arc::new(s.song(arg).await?),
            "search" => Arc::new(s.search(arg, 20).await?),
            "profile" => Arc::new(s.top().await?),
            _ => anyhow::bail!("unknown key {k}"),
        })
    };
    Task::perform(load, move |r| {
        let io = r
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<std::io::Error>());
        let m = match io.map(std::io::Error::kind) {
            Some(std::io::ErrorKind::PermissionDenied) => {
                let k = key.clone();
                task(move |a| a.expired(k.clone()))
            }
            _ => Message::Loaded(key.clone(), offset, r.map_err(|e| e.to_string())),
        };
        ours(&me, m)
    })
}

fn layer_focus(
    layer: bool,
    pick: impl Fn(Option<usize>, usize) -> usize + Send + 'static,
) -> Task<Message> {
    use iced::advanced::widget::operation::{Focusable, Outcome};
    use iced::advanced::widget::{Id, Operation};
    type Pick = Box<dyn Fn(Option<usize>, usize) -> usize + Send>;
    struct Focus(bool, Option<Pick>, usize, Option<usize>);
    impl Operation<Message> for Focus {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation<Message>)) {
            f(self);
        }
        fn focusable(&mut self, id: Option<&Id>, _: iced::Rectangle, s: &mut dyn Focusable) {
            let (mine, n) = (!self.0 || id == Some(&Id::from("layer")), self.2);
            self.2 += mine as usize;
            match &self.1 {
                Some(_) if mine && s.is_focused() => self.3 = Some(n),
                Some(_) => {}
                None if mine && self.3 == Some(n) => s.focus(),
                None => s.unfocus(),
            }
        }
        fn finish(&self) -> Outcome<Message> {
            match &self.1 {
                Some(p) if self.2 > 0 => {
                    Outcome::Chain(Box::new(Focus(self.0, None, 0, Some(p(self.3, self.2)))))
                }
                _ => Outcome::None,
            }
        }
    }
    iced::advanced::widget::operate(Focus(layer, Some(Box::new(pick)), 0, None))
}

fn song_list(v: &AnyData) -> Option<(&[Song], &str)> {
    [
        v.downcast_ref::<(Album, Vec<Song>)>()
            .map(|(a, s)| (&s[..], a.name.as_str())),
        v.downcast_ref::<(Playlist, Vec<Song>)>()
            .map(|(p, s)| (&s[..], p.name.as_str())),
        v.downcast_ref::<Page<Song>>()
            .map(|(s, _)| (&s[..], "Songs")),
        v.downcast_ref::<Trio>().map(|(_, _, s)| (&s[..], "Search")),
        v.downcast_ref::<(String, Vec<Song>)>()
            .map(|(n, s)| (&s[..], n.as_str())),
        v.downcast_ref::<Vec<Song>>().map(|s| (&s[..], "Popular")),
        v.downcast_ref::<(Vec<Artist>, Vec<Song>)>()
            .map(|(_, s)| (&s[..], "Top tracks")),
    ]
    .into_iter()
    .flatten()
    .next()
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

pub static DOWN: AtomicBool = AtomicBool::new(false);
pub static CURSOR: AtomicU64 = AtomicU64::new(0);

pub fn cursor() -> (f32, f32) {
    let bits = CURSOR.load(Relaxed);
    (
        f32::from_bits((bits >> 32) as u32),
        f32::from_bits(bits as u32),
    )
}
pub static MODS: AtomicU32 = AtomicU32::new(0);
pub static ANCHOR: Mutex<iced::Rectangle> = Mutex::new(iced::Rectangle {
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
});

type Rx = Mutex<Option<UnboundedReceiver<Message>>>;
static CH: LazyLock<(UnboundedSender<Message>, Rx)> = LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, Mutex::new(Some(rx)))
});
