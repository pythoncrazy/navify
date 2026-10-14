use std::{
    any::Any,
    collections::{BTreeSet, HashMap, HashSet},
    sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use iced::keyboard::{Key, Modifiers};
use iced::widget::operation::{self as op, AbsoluteOffset};
use iced::{Color, Task, widget::image};
use navify_audio::Track;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::api::{Album, Artist, Info, Playlist, Session, Song};
use crate::config::{self, Config};

pub const PAGE: usize = 100;
const STALE: Duration = Duration::from_secs(300);
const COVER_CAP: usize = 8 * 1024 * 1024;
const SIZES: [u32; 4] = [128, 192, 300, 640];
pub const ICON: &[u8] = include_bytes!("../assets/icons/hicolor/256x256/apps/navify.png");

#[derive(Clone, Debug, Default)]
pub struct Pager<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
}

pub type Home = [Vec<Album>; 4];

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
type Decoded = (Color, image::Handle, usize);
type Slot = Option<(u64, image::Handle, usize)>;
type Rows = (Vec<Song>, Vec<usize>);
type Stamp = (Option<String>, u8, usize);
type Run = dyn Fn(&mut App) -> Task<Message> + Send + Sync;

macro_rules! pager {
    ($v:expr, $p:ident => $e:expr) => {
        ($v.downcast_ref::<Pager<Album>>().map(|$p| $e))
            .or_else(|| $v.downcast_ref::<Pager<Artist>>().map(|$p| $e))
            .or_else(|| $v.downcast_ref::<Pager<Song>>().map(|$p| $e))
    };
}

#[derive(Clone, Debug)]
pub enum MenuKind {
    Song(Vec<Song>),
    SongIn(Vec<Song>, Vec<usize>),
    Album(Box<Album>),
    Artist(Artist),
    Playlist(Playlist),
    User,
    Device,
}

#[derive(Clone, Debug)]
pub struct Menu {
    pub x: f32,
    pub y: f32,
    pub kind: MenuKind,
    pub sub: u8,
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
        Message::Loaded("devices".into(), d)
    })
}

fn say(text: String) -> Message {
    task(move |a| a.toast(&text))
}

fn later(secs: f32, m: Message) -> Task<Message> {
    Task::future(tokio::time::sleep(Duration::from_secs_f32(secs))).map(move |()| m.clone())
}

#[derive(Clone, Debug)]
pub enum Message {
    Loaded(String, Result<AnyData, String>),
    Cover(String, u32, Result<Decoded, String>),
    Nav(String),
    Back,
    Fwd,
    Scrolled(f32, f32, f32),
    Follow,
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
    Do(Act),
    ClearCache,
    Focus(bool),
    Close,
    Noop,
    Tick,
    Edit(String, String, String),
    Modal(String, u8),
    PlayCtx(String, u8),
    RowMenu(String, usize),
    SelectAll,
    DeleteSel,
    PlayAt(String, usize),
    Retry,
    DiscChip(u8),
    DiscOrder,
    FocusMove(bool),
    Arrow(bool),
    Enter,
    Resize(f32, f32),
    SeekRelease,
    Audio(navify_audio::Event),
    Media(souvlaki::MediaControlEvent),
    MediaInit(isize),
    Window(bool),
    CloseWindow,
    Win(iced::window::Id),
    Full(bool),
}

#[derive(Debug, Default)]
pub struct Covers {
    map: HashMap<String, [Slot; 4]>,
    pub bytes: usize,
    tick: u64,
    pub color: HashMap<String, Color>,
    want: HashSet<(String, u32)>,
}

fn slot(size: u32) -> usize {
    let i = SIZES.iter().position(|s| *s == size);
    i.unwrap_or_else(|| panic!("cover size {size} is not a bucket"))
}

impl Covers {
    pub fn get(&self, id: &str, size: u32) -> Option<image::Handle> {
        let (e, mut near) = (self.map.get(id)?, [0, 1, 2, 3]);
        near.sort_unstable_by_key(|&j| SIZES[j].abs_diff(size));
        near.iter().find_map(|&j| Some(e[j].as_ref()?.1.clone()))
    }

    fn insert(&mut self, id: String, size: u32, (c, h, n): Decoded) {
        self.tick += 1;
        self.color.insert(id.clone(), c);
        let e = &mut self.map.entry(id).or_default()[slot(size)];
        let old = e.replace((self.tick, h, n));
        self.bytes = self.bytes + n - old.map_or(0, |o| o.2);
        while self.bytes > COVER_CAP {
            let all = self
                .map
                .iter()
                .flat_map(|(id, e)| (0..3).map(move |j| (&e[j], id, j)));
            let all = all.filter_map(|(s, id, j)| Some((s.as_ref()?.0, id, j)));
            let (_, id, j) = all.min().expect("the counted bytes belong to covers");
            let id = id.clone();
            let e = self.map.get_mut(&id).expect("the cover is cached");
            self.bytes -= e[j].take().expect("the slot is full").2;
            if e.iter().all(Option::is_none) {
                self.map.remove(&id);
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
    pub lyric: (usize, Option<f32>),
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
    pub lib_find: Option<String>,
    pub lib_scroll: (f32, f32),
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
    pub recent: Vec<Song>,
    pub recent_tab: bool,
    pub full: bool,
    pub pl_find: Option<String>,
    pub pl_sort: u8,
    pl_view: (String, Stamp, Option<Rows>),
}

fn shot_key(shot: Option<&str>) -> Option<String> {
    let k = shot?.split('|').next()?;
    (!k.is_empty() && k != "login" && k != "*").then(|| k.to_string())
}

fn login(u: String, n: String, p: String) -> Task<Message> {
    let run = async move { Session::login(&u, &n, &p).await };
    Task::perform(run, |r| Message::LoggedIn(r.map_err(|e| e.to_string())))
}

impl App {
    pub fn new() -> Self {
        let cfg = Config::load();
        let shot = std::env::var("NAVIFY_SHOT").ok().filter(|s| !s.is_empty());
        let active = cfg
            .servers
            .iter()
            .find(|s| cfg.active.as_ref() == Some(&s.id));
        let session = active.map(|s| Session::new(s.clone()));
        let route = if session.is_some() { "home" } else { "login" };
        let route = shot_key(shot.as_deref()).unwrap_or(route.into());
        let player = tokio::runtime::Handle::try_current().ok().map(|h| {
            let (p, mut rx) = navify_audio::Player::new(h.clone(), Session::client());
            p.set_device(&cfg.settings.device);
            h.spawn(async move {
                while let Some(e) = rx.recv().await {
                    let _ = CH.0.send(Message::Audio(e));
                }
            });
            p
        });
        let history = vec![if route == "login" { "login" } else { "home" }.into()];
        let volume = cfg.settings.volume;
        Self {
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
        let env = |k| std::env::var(k).ok();
        let creds = env("NAVIFY_URL")
            .zip(env("NAVIFY_USER"))
            .zip(env("NAVIFY_PASS"));
        let open = Task::batch(app.shot.is_none().then(|| app.open()));
        let start = if app.session.is_none()
            && !app.shot.as_deref().is_some_and(|s| s.starts_with("login"))
            && let Some(((u, n), p)) = creds
        {
            login(u, n, p)
        } else if app.session.is_some() || app.route != "login" {
            let r = std::mem::take(&mut app.route);
            Task::batch([app.nav(r), app.restore()])
        } else {
            app.show()
        };
        let prune = app.disk(Some(300 << 20));
        (app, Task::batch([prune, open, start]))
    }

    fn apply_shot_setup(&mut self) -> Task<Message> {
        let lists = self.cache.values().filter_map(|(_, v)| song_list(v));
        let songs = lists
            .map(|l| l.0)
            .find(|s| !s.is_empty())
            .unwrap_or(&[])
            .to_vec();
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
                    self.source = album.map_or("Liked Songs".into(), |s| s.album.clone());
                    self.panel = if s == "queue" { 2 } else { 1 };
                    let first = songs.first().map(|s| format!("lyrics:{}", s.id));
                    task = Task::batch(first.map(|k| self.request(&k, 0)));
                    let recent = self.data::<Home>("home").and_then(|h| h[0].first());
                    if let Some(a) = recent.filter(|_| songs.is_empty()) {
                        task = self.update(Message::PlayCtx(format!("album:{}", a.id), 0));
                    }
                }
                "menu" if !songs.is_empty() => {
                    CURSOR.store(
                        (480f32.to_bits() as u64) << 32 | 300f32.to_bits() as u64,
                        Relaxed,
                    );
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

    fn playlist(&self, id: &str) -> Option<&Playlist> {
        let lib = self.data::<Lib>("lib")?;
        lib.playlists.iter().find(|p| p.id == id)
    }

    pub fn is_starred(&self, id: &str, server: bool) -> bool {
        self.starred.get(id).copied().unwrap_or(server)
    }

    pub fn scroll(&self) -> (f32, f32) {
        let h = self.view_size().1 - 136.0;
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
            icon: Some(icon.expect("the bundled icon has a valid size")),
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: "navify".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        window::open(settings).1.map(Message::Win)
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

    pub fn lib_w(&self) -> f32 {
        let s = &self.cfg.settings;
        if s.lib_collapsed { 72.0 } else { s.panels.0 }
    }

    pub fn right_w(&self) -> f32 {
        self.cfg.settings.panels.1
    }

    pub fn main_w(&self) -> f32 {
        let right = self.right_w() + 8.0;
        self.view_size().0 - 24.0 - self.lib_w() - if self.panel > 0 { right } else { 0.0 }
    }

    pub fn grid(&self) -> (usize, f32) {
        let w = self.main_w() - 24.0;
        let n = ((w / 180.0) as usize).max(2);
        (n, w / n as f32)
    }

    pub(crate) fn songs_of(&self, key: &str) -> Option<(&[Song], String)> {
        if let Some(id) = key.strip_prefix("artist:") {
            let top = self.data::<Vec<Song>>(&format!("top:{id}"))?;
            return Some((top, self.find_artist(id)?.name));
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
        match self.pl_sort {
            1 => idx.sort_by_cached_key(|&n| low(&songs[n].title)),
            2 => idx.sort_by_cached_key(|&n| low(&songs[n].artist)),
            3 => idx.sort_by_cached_key(|&n| low(&songs[n].album)),
            4 => idx.sort_by_key(|&n| &songs[n].created),
            5 => idx.sort_by_key(|&n| (songs[n].duration * 1000.0) as u32),
            _ => {}
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

    fn find_song(&self, id: &str) -> Option<Song> {
        let mut lists = self.cache.values().filter_map(|(_, v)| song_list(v));
        let s = lists.find_map(|(l, _)| l.iter().find(|s| s.id == id));
        s.or_else(|| self.queue.iter().find(|s| s.id == id))
            .cloned()
    }

    fn find_album(&self, id: &str) -> Option<Album> {
        self.cache.values().find_map(|(_, v)| {
            let home = v.downcast_ref::<Home>().into_iter().flatten();
            let lists = [
                v.downcast_ref::<Pager<Album>>().map(|p| &p.items[..]),
                v.downcast_ref::<SearchRes>().map(|r| &r.albums[..]),
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

    fn find_artist(&self, id: &str) -> Option<Artist> {
        self.cache.values().find_map(|(_, v)| {
            let lists = [
                v.downcast_ref::<Pager<Artist>>().map(|p| &p.items[..]),
                v.downcast_ref::<SearchRes>().map(|r| &r.artists[..]),
                v.downcast_ref::<(Artist, Vec<Album>)>()
                    .map(|x| std::slice::from_ref(&x.0)),
                v.downcast_ref::<Info>().map(|x| &x.similar[..]),
                v.downcast_ref::<Lib>().map(|l| &l.artists[..]),
            ];
            let mut all = lists.into_iter().flatten().flatten();
            all.find(|a| a.id == id).cloned()
        })
    }

    fn star(&mut self, ids: &[String], on: bool) {
        for id in ids {
            self.starred.insert(id.clone(), on);
            if let Some(mut s) = self.find_song(id) {
                s.starred = on;
                self.edit::<Pager<Song>>("liked", |p| {
                    let had = p.items.iter().any(|x| x.id == s.id);
                    p.items.retain(|x| x.id != s.id);
                    p.total = (p.total + on as usize).saturating_sub(had as usize);
                    p.items.splice(0..0, on.then_some(s));
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
    }

    pub fn current_song(&self) -> Option<&Song> {
        (self.index < self.queue.len()).then(|| &self.queue[self.real_at(self.index)])
    }

    pub(crate) fn real_at(&self, i: usize) -> usize {
        self.shuffle.as_ref().map_or(i, |o| {
            assert_eq!(o.len(), self.queue.len(), "shuffle covers the queue");
            o[i]
        })
    }

    fn request(&mut self, key: &str, offset: usize) -> Task<Message> {
        let fresh = |(t, v): &(Instant, AnyData)| t.elapsed() < STALE && !v.is::<String>();
        if offset == 0 && self.cache.get(key).is_some_and(fresh) {
            return Task::none();
        }
        match self.session.clone() {
            Some(s) if self.pending.insert(format!("{key}#{offset}")) => {
                fetch(s, key.to_string(), offset)
            }
            _ => Task::none(),
        }
    }

    fn toast(&mut self, text: &str) -> Task<Message> {
        self.toast_seq += 1;
        let n = self.toast_seq;
        self.toast = Some((n, text.to_string()));
        let clear = act(move |a| a.toast = a.toast.take().filter(|t| t.0 != n));
        later(3.0, clear)
    }

    fn report(&mut self, r: anyhow::Result<()>) -> Task<Message> {
        r.map_or_else(|e| self.toast(&e.to_string()), |()| Task::none())
    }

    fn save_later(&mut self) -> Task<Message> {
        if !self.cfg.settings.queue_sync {
            return Task::none();
        }
        self.save_seq += 1;
        let n = self.save_seq;
        let save = task(move |a| Task::batch((n == a.save_seq).then(|| a.save_queue())));
        later(5.0, save)
    }

    fn save_queue(&self) -> Task<Message> {
        if !self.cfg.settings.queue_sync {
            return Task::none();
        }
        let ids: Vec<String> = self.queue.iter().map(|x| x.id.clone()).collect();
        let cur = self.current_song().map_or(String::new(), |x| x.id.clone());
        let pos = (self.pos() * 1000.0) as u64;
        let save = |s: Session| async move { s.save_play_queue(&ids, &cur, pos).await };
        self.send(save, "Could not save the play queue.")
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
            Err(e) => say(format!("Could not save the settings. {e}")),
        })
    }

    pub fn gain(&self, song: &Song) -> f32 {
        let (st, rg) = (&self.cfg.settings, &song.rg);
        let db = match st.replaygain.as_str() {
            "track" => rg.track_gain.or(rg.album_gain),
            "album" => rg.album_gain.or(rg.track_gain),
            _ => return 1.0,
        };
        let peak = rg.track_peak.or(rg.album_peak).unwrap_or(1.0).max(1e-3);
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
        Track {
            id,
            url,
            gain,
            transcoded,
            duration,
        }
    }

    fn disk(&self, covers: Option<u64>) -> Task<Message> {
        let dir = crate::api::cache_dir("");
        let scan = tokio::task::spawn_blocking(move || {
            covers.inspect(|&c| crate::api::prune(&dir.join("covers"), c));
            let files = crate::api::cache_files(&dir);
            files.iter().map(|f| f.2).sum::<u64>()
        });
        Task::perform(scan, |n| {
            let n: AnyData = Arc::new(n.expect("cache scan"));
            Message::Loaded("disk".into(), Ok(n))
        })
    }

    fn next_index(&self, manual: bool) -> Option<usize> {
        let (i, len) = (self.index, self.queue.len());
        if self.repeat == 2 && !manual {
            return (i < len).then_some(i);
        }
        let wrap = (self.repeat == 1 && len > 0).then_some(0);
        (i + 1 < len).then_some(i + 1).or(wrap)
    }

    fn start(&mut self, i: usize, shuffled: bool) -> Task<Message> {
        self.shuffle = shuffled.then(|| shuffle(0..self.queue.len(), i));
        self.play_index(if shuffled { 0 } else { i }, 0.0)
    }

    fn play_index(&mut self, i: usize, at: f32) -> Task<Message> {
        if self.queue.is_empty() {
            return Task::none();
        }
        let done = Task::batch(self.loaded.then(|| self.listen(false)));
        (self.index, self.playing) = (i.min(self.queue.len() - 1), true);
        let t = self.track(&self.queue[self.real_at(self.index)]);
        self.player.iter().for_each(|p| p.play(t.clone(), at));
        let began = self.began();
        self.position = at;
        Task::batch([done, began, self.listen(true)])
    }

    fn began(&mut self) -> Task<Message> {
        (self.position, self.scrobbled, self.heard, self.loaded) = (0.0, false, 0.0, true);
        self.lyric = (usize::MAX, Some(0.0));
        self.queue_next_refresh();
        let Some(song) = self.current_song().cloned() else {
            return Task::none();
        };
        self.recent.retain(|s| s.id != song.id);
        self.recent.insert(0, song.clone());
        self.recent.truncate(50);
        let lyr = self.request(&format!("lyrics:{}", song.id), 0);
        let now = |s: Session| async move { s.scrobble(&song.id, false).await };
        let now = self.send(now, "Could not send the playing track to the server.");
        let now = Task::batch(self.cfg.settings.scrobble.then_some(now));
        Task::batch([lyr, now, self.save_later()])
    }

    fn pos(&self) -> f32 {
        let p = self.player.as_ref().filter(|_| self.loaded);
        p.map_or(self.position, |p| p.position())
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
        let pct = self.cfg.settings.scrobble_pct / 100.0;
        let left = (song.duration * pct).min(240.0) - self.heard;
        let id = song.id.clone();
        if left <= 0.0 {
            self.scrobbled = true;
            let done = |s: Session| async move { s.scrobble(&id, true).await };
            return self.send(done, "Could not send the played track to the server.");
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
            act(move |a| {
                if !songs.is_empty() && !a.loaded {
                    a.position = pos as f32 / 1000.0;
                    a.index = songs.iter().position(|s| s.id == cur).unwrap_or(0);
                    (a.queue, a.shuffle) = (songs.clone(), None);
                }
            })
        };
        let fail = "Could not restore the play queue.";
        self.call(|s| async move { s.play_queue().await }, fail, ok)
    }

    fn covers_now(&mut self) -> Task<Message> {
        let ids = self.visible_covers();
        self.want_covers(ids)
    }

    fn want_covers(&mut self, ids: impl IntoIterator<Item = (String, u32)>) -> Task<Message> {
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        for (id, size) in ids {
            let e = self.covers.map.get(&id);
            let have = e.is_some_and(|e| e[slot(size)].is_some());
            if id.is_empty() || have || !self.covers.want.insert((id.clone(), size)) {
                continue;
            }
            let (s, k) = (s.clone(), id.clone());
            let load = async move {
                let b = s.cover(&k, size).await.map_err(|e| e.to_string())?;
                let wide = k.starts_with("https://");
                let px = if wide { 1280 } else { size };
                static ONE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
                let _one = ONE.acquire().await.expect("the decode semaphore is open");
                let d = tokio::task::spawn_blocking(move || decode(b, px, wide));
                d.await.expect("decode task")
            };
            let done = move |r| Message::Cover(id.clone(), size, r);
            tasks.push(Task::perform(load, done));
        }
        Task::batch(tasks)
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
        Task::perform(f(s), move |r| {
            r.map_or_else(|e| say(format!("{fail} {e}")), &ok)
        })
    }

    fn send<F>(&self, f: impl FnOnce(Session) -> F, fail: &'static str) -> Task<Message>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.call(f, fail, |()| Message::Noop)
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
        let albums = crate::pages::disc_view(self);
        let keys = albums.iter().filter(|_| !self.cfg.settings.disc_grid);
        let keys: Vec<String> = keys.map(|a| format!("album:{}", a.id)).collect();
        let mut tasks: Vec<_> = keys.iter().map(|k| self.request(k, 0)).collect();
        tasks.push(self.covers_now());
        Task::batch(tasks)
    }

    fn scroll_to(&self, y: f32) -> Task<Message> {
        op::scroll_to(crate::view::SCROLL, AbsoluteOffset { x: 0.0, y })
    }

    fn show(&mut self) -> Task<Message> {
        let route = self.history[self.hi].clone();
        (self.route, self.pl_find) = (route.clone(), None);
        (self.menu, self.hover, self.sel, self.picked) = (None, None, None, BTreeSet::new());
        (self.popular_all, self.disc_chip) = (false, 0);
        let key = self.key_of(&route);
        let id = route.strip_prefix("artist:");
        let parts = id.map(|id| ["top", "info", "appears"].map(|k| format!("{k}:{id}")));
        let parts = parts.as_ref().map_or(&[][..], |p| &p[..]);
        let keep = |k: &String| *k == key || k == "lib" || parts.contains(k);
        self.cache
            .retain(|k, (t, _)| t.elapsed() < STALE || keep(k));
        self.seeds.extend(self.seed(&key).map(|v| (key.clone(), v)));
        let keys = parts.iter().map(String::as_str);
        let keys = [key.as_str(), "lib"].into_iter().chain(keys);
        let mut tasks: Vec<_> = keys.map(|k| self.request(k, 0)).collect();
        tasks.push(self.disc_load());
        match route == "lyrics" {
            true => self.lyric = (usize::MAX, Some(0.0)),
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
        let (kind, id) = key
            .split_once(':')
            .filter(|_| !self.cache.contains_key(key))?;
        Some(match kind {
            "album" => Arc::new((self.find_album(id)?, Vec::<Song>::new())),
            "artist" => Arc::new((self.find_artist(id)?, Vec::<Album>::new())),
            "playlist" => Arc::new((self.playlist(id)?.clone(), Vec::<Song>::new())),
            _ => return None,
        })
    }

    fn settings(&mut self, o: config::Settings) -> Task<Message> {
        self.menu = None;
        let n = &self.cfg.settings;
        if let Some(p) = &self.player
            && o.device != n.device
        {
            p.set_device(&n.device);
        }
        let gain = |s: &config::Settings| {
            let t = (s.transcode_format.clone(), s.transcode_max_bitrate);
            (s.replaygain.clone(), s.preamp_db, s.fallback_db, t)
        };
        let heard = self.loaded && o.scrobble != n.scrobble;
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
        Task::batch([grid, zoom, heard, self.persist()])
    }

    fn halt(&mut self) -> Task<Message> {
        let l = self.listen(false);
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
        let data = self
            .cache
            .get(&self.route)
            .map_or(0, |c| Arc::as_ptr(&c.1).cast::<()>() as usize);
        let stamp = (self.pl_find.clone(), self.pl_sort, data);
        if (&self.pl_view.0, &self.pl_view.1) != (&self.route, &stamp) {
            if self.sel.as_ref().is_some_and(|s| s.0 == self.route) && self.pl_view.0 == self.route
            {
                (self.sel, self.picked) = (None, BTreeSet::new());
            }
            let rows = self
                .pl_view_of()
                .filter(|_| self.route.starts_with("playlist:"));
            self.pl_view = (self.route.clone(), stamp, rows);
        }
        let follow = self.lyric.1.is_some() && self.route == "lyrics";
        let line = crate::pages::lyric_scroll(self).filter(|l| follow && l.0 != self.lyric.0);
        if let Some((n, y)) = line {
            self.lyric = (n, Some(y));
            t = Task::batch([t, self.scroll_to(y)]);
        }
        if let Some(p) = &self.player {
            p.set_volume(if self.muted { 0.0 } else { self.volume });
            p.set_crossfade(self.cfg.settings.crossfade_secs);
        }
        let id = self.current_song().map_or("", |s| s.id.as_str());
        if (id, self.playing) == (self.os.0.as_str(), self.os.1) && !seek {
            return t;
        }
        self.os = (id.to_string(), self.playing);
        let (song, pos) = (self.current_song().cloned(), self.pos());
        let (song, s) = (song.as_ref(), self.session.as_ref());
        let art = song
            .zip(s)
            .map(|(s, x)| x.cover_file(&s.cover, self.bucket(56.0)));
        let art = art.filter(|f| f.exists());
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
        match m {
            Message::Loaded(key, Err(e)) => {
                self.pending.retain(|p| !p.starts_with(&format!("{key}#")));
                if self.shot.is_some() {
                    eprintln!("LOAD-ERR {key} {e}");
                }
                let known = self.cache.contains_key(&key) || self.seeds.contains_key(&key);
                if known || key != self.key_of(&self.route) {
                    return self.toast(&e);
                }
                self.cache.insert(key, (Instant::now(), Arc::new(e)));
            }
            Message::Loaded(key, Ok(v)) => {
                self.pending.retain(|p| !p.starts_with(&format!("{key}#")));
                self.seeds.remove(&key);
                let v = self.merge(&key, v);
                let banner = key.starts_with("artist:") && self.cfg.settings.banners;
                let banner = banner
                    .then(|| v.downcast_ref::<(Artist, Vec<Album>)>())
                    .flatten();
                let banner = banner.map(|(a, _)| a.banner_key());
                self.cache.insert(key.clone(), (Instant::now(), v));
                let banner = Task::batch(banner.map(|k| self.request(&k, 0)));
                let setup = shot_key(self.shot.as_deref()).is_some_and(|k| k == key);
                let setup = Task::batch(setup.then(|| self.apply_shot_setup()));
                return Task::batch([self.disc_load(), setup, banner]);
            }
            Message::Cover(id, size, r) => {
                self.covers.want.remove(&(id.clone(), size));
                match r {
                    Ok(d) => self.covers.insert(id, size, d),
                    Err(e) => return self.toast(&format!("Could not load a cover. {e}")),
                }
            }
            Message::Nav(r) => return self.nav(r),
            Message::Back if self.hi > 0 => {
                self.hi -= 1;
                return self.show();
            }
            Message::Fwd if self.hi + 1 < self.history.len() => {
                self.hi += 1;
                return self.show();
            }
            Message::Back | Message::Fwd => {}
            Message::Scrolled(off, view, content) => {
                let key = self.route.clone();
                self.scrolls.insert(key.clone(), (off, view));
                let max = (content - view).max(0.0);
                if key == "lyrics" && self.lyric.1.is_some_and(|y| (y.min(max) - off).abs() > 2.0) {
                    self.lyric.1 = None;
                }
                let more = self
                    .page_len(&key)
                    .filter(|_| content > 0.0 && off + view * 3.0 >= content);
                let more = Task::batch(more.map(|n| self.request(&key, n)));
                return Task::batch([more, self.disc_load()]);
            }
            Message::Search(q) => {
                (self.search_q, self.search_seq) = (q, self.search_seq + 1);
                return later(0.25, Message::SearchGo(self.search_seq));
            }
            Message::SearchGo(n) if n == self.search_seq => {
                let q = self.search_q.trim();
                let r = format!("search{}{q}", if q.is_empty() { "" } else { ":" });
                return match r == self.route {
                    true => self.request(&r, 0),
                    false => self.nav(r),
                };
            }
            Message::SearchGo(_) | Message::Noop => {}
            Message::Play(songs, i, source) => {
                (self.queue, self.source) = (songs, source);
                self.ctx.clear();
                return self.start(i, self.shuffle.is_some());
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
            Message::Prev if self.pos() > 3.0 => return self.update(Message::Seek(0.0)),
            Message::Prev if self.index > 0 => return self.play_index(self.index - 1, 0.0),
            Message::Prev => {}
            Message::Next(manual) => {
                return match self.next_index(manual) {
                    Some(n) => self.play_index(n, 0.0),
                    None => self.halt(),
                };
            }
            Message::Seek(pos) => {
                let again = pos < 5.0 && self.pos() > 10.0;
                (self.position, self.lyric) = (pos, (usize::MAX, Some(0.0)));
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
            Message::Repeat => {
                self.repeat = (self.repeat + 1) % 3;
                self.queue_next_refresh();
            }
            Message::Vol(v) => (self.volume, self.cfg.settings.volume, self.muted) = (v, v, false),
            Message::Mute => self.muted = !self.muted,
            Message::Star(ids, on) => {
                (self.menu, self.sel, self.picked) = (None, None, BTreeSet::new());
                self.star(&ids, on);
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
                let call = async move { (s.star(&ids, on).await.map_err(|e| e.to_string()), ids) };
                return Task::perform(call, move |(r, ids)| match r {
                    Ok(()) => Message::Noop,
                    Err(e) => task(move |a| {
                        a.star(&ids, !on);
                        a.toast(&format!("Could not update Your Library. {e}"))
                    }),
                });
            }
            Message::Rate(id, n) => {
                self.ratings.insert(id.clone(), n);
                self.menu = None;
                let rate = |s: Session| async move { s.rate(&id, n).await };
                return self.send(rate, "Could not set the rating.");
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
                return Task::batch([self.save_later(), self.toast("Added to queue")]);
            }
            Message::QDel(i) => {
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
            Message::QClear => {
                let done = self.halt();
                (self.queue, self.index) = (Vec::new(), 0);
                self.shuffle = self.shuffle.as_ref().map(|_| Vec::new());
                return Task::batch([done, self.save_later()]);
            }
            Message::Menu(kind) => {
                let bits = CURSOR.load(Relaxed);
                let x = f32::from_bits((bits >> 32) as u32);
                let y = f32::from_bits(bits as u32);
                let scan = matches!(*kind, MenuKind::Device).then(scan_devices);
                self.menu = Some(Menu {
                    x,
                    y,
                    kind: *kind,
                    sub: 0,
                });
                return Task::batch(scan);
            }
            Message::MenuClose if self.full && self.menu.is_none() && self.modal.is_none() => {
                return self.update(Message::Full(false));
            }
            Message::Full(on) => {
                use iced::window::{Mode, set_mode};
                self.full = on;
                let mode = [Mode::Windowed, Mode::Fullscreen][usize::from(on)];
                let mode = self.win.map(|id| set_mode(id, mode));
                return Task::batch([Task::batch(mode), self.covers_now()]);
            }
            Message::MenuClose => {
                if self.menu.is_none() && self.modal.is_none() {
                    (self.sel, self.picked) = (None, BTreeSet::new());
                }
                (self.menu, self.modal) = (None, None);
            }
            Message::PlAdd(pid, ids) => {
                self.menu = None;
                let songs: Vec<Song> = ids.iter().filter_map(|id| self.find_song(id)).collect();
                let n = songs.len() as u32;
                let key = format!("playlist:{pid}");
                self.edit::<(Playlist, Vec<Song>)>(&key, |(_, s)| s.extend(songs));
                self.edit_pl(&pid, |p| p.song_count += n);
                let done = format!("Added to {}.", self.playlist(&pid).map_or("", |p| &p.name));
                let done = self.toast(&done);
                let add = |s: Session| async move {
                    s.playlist_update(&pid, None, None, &ids, &[]).await
                };
                let add = self.send(add, "Could not add the song to the playlist.");
                return Task::batch([add, done]);
            }
            Message::PlNew(name, ids) => {
                self.menu = None;
                let make = |s: Session| async move { s.playlist_create(&name, &ids).await };
                return self.call(make, "Could not make the playlist.", |p: Playlist| {
                    task(move |a| {
                        a.edit::<Lib>("lib", |l| l.playlists.insert(0, p.clone()));
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
                let del = |s: Session| async move { s.playlist_delete(&id).await };
                return Task::batch([back, self.send(del, "Could not delete the playlist.")]);
            }
            Message::PlRemove(idx) => {
                self.menu = None;
                let Some(pid) = self.route.strip_prefix("playlist:").map(str::to_string) else {
                    return Task::none();
                };
                (self.sel, self.picked) = (None, BTreeSet::new());
                self.edit::<(Playlist, Vec<Song>)>(&self.route.clone(), |(_, s)| {
                    let mut n = 0;
                    s.retain(|_| (!idx.contains(&n), n += 1).0);
                });
                let k = idx.len() as u32;
                self.edit_pl(&pid, |p| p.song_count = p.song_count.saturating_sub(k));
                let rm = |s: Session| async move {
                    s.playlist_update(&pid, None, None, &[], &idx).await
                };
                return self.send(rm, "Could not remove the songs from the playlist.");
            }
            Message::Radio(id) => {
                self.menu = None;
                let similar = |s: Session| async move { s.similar_songs(&id, 30).await };
                let ok = |v: Vec<Song>| match v.is_empty() {
                    true => say("No similar songs were found.".into()),
                    false => Message::Play(v, 0, "Radio".into()),
                };
                return self.call(similar, "Could not start the radio.", ok);
            }
            Message::Panel(n) => {
                self.panel = if self.panel == n { 0 } else { n };
                return self.covers_now();
            }
            Message::Drag(d) => {
                self.drag = d;
                if d.is_none() {
                    return Task::batch([self.persist(), self.covers_now()]);
                }
            }
            Message::DragTo(x) => {
                let (vw, s) = (self.view_size().0, &mut self.cfg.settings);
                match self.drag {
                    Some(0) if x < 208.0 => s.lib_collapsed = true,
                    Some(0) => {
                        (s.panels.0, s.lib_collapsed) = ((x - 8.0).clamp(280.0, 480.0), false)
                    }
                    _ => s.panels.1 = (vw - 8.0 - x).clamp(280.0, 420.0),
                }
            }
            Message::LibCollapse => {
                self.cfg.settings.lib_collapsed = !self.cfg.settings.lib_collapsed;
                return self.covers_now();
            }
            Message::NewPlaylist => {
                let n = self.data::<Lib>("lib").map_or(0, |l| l.playlists.len()) + 1;
                return self.update(Message::PlNew(format!("My Playlist #{n}"), Vec::new()));
            }
            Message::LField(i, v) => {
                let l = &mut self.login;
                let f = [&mut l.url, &mut l.user, &mut l.pass, &mut self.new_pl];
                *f.into_iter().nth(i.into()).unwrap_or(&mut self.new_pl_desc) = v;
            }
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
            Message::LoggedIn(Err(e)) => (self.login.busy, self.login.error) = (false, e),
            Message::LoggedIn(Ok(s)) => {
                let (id, pass) = (s.server.id.clone(), &self.login.pass);
                let kept = self.login.remember.then(|| config::remember(&id, pass));
                self.cfg.servers.retain(|x| x.id != id);
                self.cfg.servers.push(s.server.clone());
                (self.cfg.active, self.session, self.login) = (Some(id), Some(s), Login::default());
                self.route.clear();
                self.history.clear();
                let to = shot_key(self.shot.as_deref()).unwrap_or_else(|| "home".into());
                let (t, n) = (self.persist(), self.nav(to));
                return Task::batch([t, n, self.restore(), self.report(kept.unwrap_or(Ok(())))]);
            }
            Message::LogOut => {
                let gone = self.cfg.active.take().map_or(Ok(()), |id| {
                    self.cfg.servers.retain(|s| s.id != id);
                    config::forget(&id)
                });
                let h = self.halt();
                self.session = None;
                self.queue.clear();
                self.cache.clear();
                let (t, n) = (self.persist(), self.nav("login".into()));
                return Task::batch([h, t, n, self.report(gone)]);
            }
            Message::UseServer(id) => {
                let Some(srv) = self.cfg.servers.iter().find(|s| s.id == id).cloned() else {
                    return Task::none();
                };
                let s = Session::new(srv);
                (self.session, self.cfg.active) = (Some(s.clone()), Some(id));
                self.cache.clear();
                self.pending.clear();
                let ping =
                    async move { s.rest::<serde_json::Value>("ping", &[], "").await.is_ok() };
                let check = Task::perform(ping, |ok| match ok {
                    true => Message::Noop,
                    false => task(|a| {
                        a.session.iter_mut().for_each(|s| s.server.native = false);
                        a.toast("The server session expired. Using the compatible API.")
                    }),
                });
                return Task::batch([self.persist(), self.nav("home".into()), check]);
            }
            Message::DropServer(id) => {
                self.cfg.servers.retain(|s| s.id != id);
                let gone = config::forget(&id);
                return Task::batch([self.persist(), self.report(gone)]);
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
            Message::Focus(true) => self.hidden = false,
            Message::Focus(false) => {
                let min = |m: Option<bool>| act(move |a| a.hidden = m == Some(true));
                let wait = later(0.3, Message::Noop).then(|_| iced::window::latest());
                return wait.and_then(iced::window::is_minimized).map(min);
            }
            Message::Close => {
                let save = Task::batch([self.persist(), self.save_queue()]);
                return save.collect().then(|_| iced::exit());
            }
            Message::CloseWindow if self.tray.is_some() && self.cfg.settings.tray => {
                return self.update(Message::Window(false));
            }
            Message::CloseWindow => return self.update(Message::Close),
            Message::Window(false) if self.tray.is_none() => {
                let min = self.win.map(|id| iced::window::minimize(id, true));
                return Task::batch(min);
            }
            Message::Window(show) => {
                use iced::window::{self as w, Mode};
                let Some(id) = self.win else {
                    return self.open();
                };
                if show {
                    return w::set_mode(id, Mode::Windowed).chain(w::gain_focus(id));
                }
                self.hidden = true;
                self.win = self.win.filter(|_| cfg!(windows));
                let close = match cfg!(windows) {
                    true => w::set_mode(id, Mode::Hidden),
                    false => w::close(id),
                };
                return Task::batch([self.persist(), self.save_queue(), close]);
            }
            Message::Win(id) => {
                use iced::window::raw_window_handle::RawWindowHandle;
                self.win = Some(id);
                if self.tray.is_none() {
                    let tray = crate::tray::spawn(CH.0.clone());
                    let tray =
                        tray.inspect_err(|e| eprintln!("The tray icon is not available. {e}"));
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
                return scroll.chain(hwnd.map(Message::MediaInit));
            }
            Message::PlayCtx(route, mode) => {
                self.menu = None;
                if mode < 2 {
                    self.ctx = route.clone();
                }
                if let Some((v, src)) = self.songs_of(&route).filter(|(v, _)| !v.is_empty()) {
                    return self.play_songs(v.to_vec(), src, mode);
                }
                let load = |s: Session| async move {
                    let (kind, id) = route.split_once(':').unwrap_or((&route, ""));
                    let all = |sort| s.songs(sort, 0, PAGE * 5);
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
                    true => say("Nothing to play.".into()),
                    false => task(move |a| a.play_songs(v.clone(), src.clone(), mode)),
                };
                return self.call(load, "Could not play.", ok);
            }
            Message::RowMenu(key, n) => {
                if !self.sel.as_ref().is_some_and(|s| s.0 == key) || !self.picked.contains(&n) {
                    (self.sel, self.picked, self.anchor) = (Some((key.clone(), n)), [n].into(), n);
                }
                let list = self.songs_of(&key).expect("a rendered row has its list").0;
                let idx = self.picked.iter().copied().filter(|&i| !list[i].is_dir);
                let idx: Vec<usize> = idx.collect();
                let songs = idx.iter().map(|&i| list[i].clone()).collect();
                let kind = match key.starts_with("playlist:") {
                    true => {
                        MenuKind::SongIn(songs, idx.iter().map(|&i| self.orig(&key, i)).collect())
                    }
                    false => MenuKind::Song(songs),
                };
                return self.update(Message::Menu(Box::new(kind)));
            }
            Message::SelectAll => {
                if let Some(n) = self.songs_of(&self.route).map(|(l, _)| l.len()) {
                    self.sel = Some((self.route.clone(), 0));
                    (self.picked, self.anchor) = ((0..n).collect(), 0);
                }
            }
            Message::DeleteSel => {
                let mine = self.sel.as_ref().is_some_and(|s| s.0 == self.route);
                if mine && self.route.starts_with("playlist:") {
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
                let head = &list[..n.min(list.len())];
                let n = n - head.iter().filter(|s| s.is_dir).count();
                let list = list.iter().filter(|s| !s.is_dir).cloned().collect();
                let t = self.update(Message::Play(list, n, src));
                self.ctx = key;
                return t;
            }
            Message::Retry => {
                let key = self.key_of(&self.route);
                self.cache.remove(&key);
                return self.request(&key, 0);
            }
            Message::FocusMove(back) => {
                let layer = self.modal.is_some() || self.menu.is_some();
                return layer_focus(layer, move |cur, n| match cur {
                    Some(i) => (i + if back { n - 1 } else { 1 }) % n,
                    None if back => n - 1,
                    None => 0,
                });
            }
            Message::Arrow(up) if self.menu.is_some() => {
                return self.update(Message::FocusMove(up));
            }
            Message::Arrow(up) => {
                use crate::pages::{BAR, HEAD, PAD, ROW};
                let Some((key, n)) = self.sel.clone() else {
                    return Task::none();
                };
                let list = self.songs_of(&key).expect("a selected row has its list").0;
                let last = list.len().saturating_sub(1);
                let n = if up { n.max(1) - 1 } else { (n + 1).min(last) };
                let multi = list.iter().any(|s| s.disc != list[0].disc);
                let pairs = list[..=n].windows(2);
                let discs = pairs.filter(|w| w[0].disc != w[1].disc).count();
                self.pick(key.clone(), n);
                let (kind, _) = key.split_once(':').unwrap_or((&key, ""));
                if key != self.route || !matches!(kind, "album" | "playlist" | "liked" | "songs") {
                    return Task::none();
                }
                let discs = (1 + discs) * usize::from(multi && kind == "album");
                let top = crate::pages::art_px(self) + 2.0 * PAD + BAR + HEAD;
                let top = top + (n + discs) as f32 * ROW;
                let (off, view) = self.scroll();
                let to = match () {
                    _ if top < off + 100.0 => top - 100.0,
                    _ if top + ROW > off + view => top + ROW - view,
                    _ => return Task::none(),
                };
                self.scrolls.insert(key, (to.max(0.0), view));
                return self.scroll_to(to.max(0.0));
            }
            Message::Enter => {
                if let Some((k, n)) = self.sel.clone()
                    && self.menu.is_none()
                    && self.modal.is_none()
                {
                    return self.update(Message::PlayAt(k, n));
                }
            }
            Message::Resize(w, h) => {
                let s = self.scale();
                self.cfg.settings.window = (w * s, h * s);
                let h = self.view_size().1;
                self.scrolls.values_mut().for_each(|s| s.1 = h - 136.0);
                return self.covers_now();
            }
            Message::DiscChip(n) => {
                self.disc_chip = n;
                return self.covers_now();
            }
            Message::DiscOrder => {
                self.disc_old = !self.disc_old;
                return self.disc_load();
            }
            Message::SeekRelease => {
                let seek = self.seek_preview.take();
                return Task::batch(seek.map(|v| self.update(Message::Seek(v))));
            }
            Message::Tick => self.position = self.pos(),
            Message::Follow => self.lyric = (usize::MAX, Some(0.0)),
            Message::LibScrolled(off, view) => {
                let old = crate::view::lib_rows(self).2;
                self.lib_scroll = (off, view);
                if crate::view::lib_rows(self).2 != old {
                    return self.covers_now();
                }
            }
            Message::Modal(id, kind) => {
                self.menu = None;
                let p = self.playlist(&id).expect("the dialog has a playlist");
                let cover = p.cover.clone();
                (self.new_pl, self.new_pl_desc) = (p.name.clone(), p.comment.clone());
                self.modal = Some((id, kind));
                let all = op::select_all(iced::widget::Id::new("layer"));
                let focus = layer_focus(true, |_, n| n.min(2) - 1).chain(all);
                return Task::batch([self.want_covers([(cover, self.bucket(180.0))]), focus]);
            }
            Message::Edit(id, name, comment) => {
                self.modal = None;
                let (name, comment) = (name.trim().to_string(), comment.trim().to_string());
                assert!(!name.is_empty(), "the dialog blocks an empty name");
                let set = |p: &mut Playlist| (p.name, p.comment) = (name.clone(), comment.clone());
                self.edit_pl(&id, set);
                self.edit::<(Playlist, Vec<Song>)>(&format!("playlist:{id}"), |(p, _)| set(p));
                let edit = |s: Session| async move {
                    let (n, c) = (Some(name.as_str()), Some(comment.as_str()));
                    s.playlist_update(&id, n, c, &[], &[]).await
                };
                return self.send(edit, "Could not change the playlist.");
            }
            Message::MediaInit(hwnd) => {
                let (tx, h) = (CH.0.clone(), hwnd as *mut std::ffi::c_void);
                let c = souvlaki::PlatformConfig {
                    display_name: "navify",
                    dbus_name: "navify",
                    hwnd: (hwnd != 0).then_some(h),
                };
                let m = souvlaki::MediaControls::new(c);
                let send = move |e| drop(tx.send(Message::Media(e)));
                let m = m.and_then(|mut m| m.attach(send).map(|()| m));
                match m {
                    Ok(m) => (self.media, self.os) = (Some(m), Default::default()),
                    Err(e) => {
                        return self.toast(&format!("Media controls are not available. {e:?}"));
                    }
                }
            }
            Message::Media(e) => {
                use souvlaki::{MediaControlEvent as E, SeekDirection as D};
                let pos = self.pos();
                let by = |d: D, t: f32| match d {
                    D::Forward => Message::Seek(pos + t),
                    D::Backward => Message::Seek((pos - t).max(0.0)),
                };
                let m = match e {
                    E::Toggle => Message::Toggle,
                    E::Play if !self.playing => Message::Toggle,
                    E::Pause | E::Stop if self.playing => Message::Toggle,
                    E::Next => Message::Next(true),
                    E::Previous => Message::Prev,
                    E::Seek(d) => by(d, self.cfg.settings.seek_secs),
                    E::SeekBy(d, t) => by(d, t.as_secs_f32()),
                    E::SetPosition(p) => Message::Seek(p.0.as_secs_f32()),
                    E::SetVolume(v) => Message::Vol(v.clamp(0.0, 1.0) as f32),
                    E::Quit => Message::Close,
                    E::Raise => Message::Window(true),
                    _ => return Task::none(),
                };
                return self.update(m);
            }
            Message::Audio(navify_audio::Event::Started(id)) => {
                let done = self.listen(false);
                let next = self.next_index(false);
                let n =
                    next.filter(|&n| self.queue.get(self.real_at(n)).is_some_and(|s| s.id == id));
                self.index = n.unwrap_or(self.index);
                let began = Task::batch(n.map(|_| self.began()));
                return Task::batch([done, began, self.listen(true)]);
            }
            Message::Audio(navify_audio::Event::Ended) => return self.update(Message::Next(false)),
            Message::Audio(navify_audio::Event::Error(e)) => {
                let done = self.listen(false);
                (self.playing, self.loaded) = (false, false);
                return Task::batch([done, self.toast(&format!("Playback failed. {e}"))]);
            }
        }
        Task::none()
    }

    fn play_songs(&mut self, songs: Vec<Song>, src: String, mode: u8) -> Task<Message> {
        if mode == 2 {
            return self.update(Message::Queue(songs, false));
        }
        (self.queue, self.source) = (songs, src);
        self.start(0, mode == 1)
    }

    fn merge(&self, key: &str, v: AnyData) -> AnyData {
        let Some((_, old)) = self.cache.get(key) else {
            return v;
        };
        pager!(v, b => old.downcast_ref::<Pager<_>>().filter(|_| b.offset > 0).map(|a| {
            let items = [&a.items[..], &b.items[..]].concat();
            Arc::new(Pager { items, total: b.total, offset: 0 }) as AnyData
        }))
        .flatten()
        .unwrap_or(v)
    }

    fn page_len(&self, key: &str) -> Option<usize> {
        let v = &self.cache.get(key)?.1;
        pager!(v, p => (p.items.len() < p.total).then_some(p.items.len())).flatten()
    }

    fn visible_covers(&self) -> Vec<(String, u32)> {
        let (off, view) = self.scroll();
        let (cols, cw) = self.grid();
        let span = |n: usize, h: f32, per: usize| {
            let lo = (off / h).max(0.0) as usize;
            (lo.saturating_sub(1) * per).min(n)..((lo + (view / h) as usize + 3) * per).min(n)
        };
        let rows = |v: &[Song]| span(v.len(), crate::pages::ROW, 1);
        let tiles = |n: usize| span(n, cw + 52.0, cols);
        let (key, lib) = (self.route.as_str(), self.data::<Lib>("lib"));
        let (mut big, mut small): (Vec<&String>, Vec<&String>) = Default::default();
        let (mut huge, mut head): (Vec<&String>, Vec<&String>) = Default::default();
        if let Some((a, _)) = self.data::<(Album, Vec<Song>)>(key) {
            head.push(&a.cover);
            big.extend(crate::pages::more_by(self, a).into_iter().map(|x| &x.cover));
            small.push(a.artists.first().map_or(&a.artist_id, |x| &x.id));
        } else if let Some((p, s)) = self.data::<(Playlist, Vec<Song>)>(key) {
            let s = self.songs_of(key).map_or(&s[..], |x| x.0);
            head.push(&p.cover);
            small.extend(s[rows(s)].iter().map(|s| &s.cover));
        } else if let Some(s) = self.data::<Pager<Song>>(key).map(|p| &p.items) {
            small.extend(s[rows(s)].iter().map(|s| &s.cover));
        } else if let Some(s) = self.data::<Vec<Song>>(key) {
            small.extend(s[rows(s)].iter().map(|s| &s.cover));
        } else if key.starts_with("discography:") {
            big.extend(crate::pages::disc_view(self).iter().map(|a| &a.cover));
        } else if let Some((a, albums)) = self.data::<(Artist, Vec<Album>)>(key) {
            let id = &key["artist:".len()..];
            let top = self.data::<Vec<Song>>(&format!("top:{id}"));
            let info = self.data::<Info>(&format!("info:{id}"));
            let appears = self.data::<Vec<Album>>(&format!("appears:{id}"));
            let shelf = crate::pages::disc_shelf(self, albums);
            head.push(&a.cover);
            let banner = self.data::<String>(&a.banner_key());
            huge.extend(banner.filter(|u| self.cfg.settings.banners && !u.is_empty()));
            small.extend(top.iter().flat_map(|v| v.iter().take(10)).map(|s| &s.cover));
            big.extend(shelf.iter().map(|x| &x.cover));
            let similar = info.map_or(&[][..], |i| &i.similar);
            big.extend(similar.iter().take(cols).map(|x| &x.cover));
            let appears = appears.map_or(&[][..], |v| &v[..]);
            big.extend(appears.iter().take(cols).map(|x| &x.cover));
        } else if let Some(p) = self.data::<Pager<Album>>(key).map(|p| &p.items) {
            big.extend(p[tiles(p.len())].iter().map(|a| &a.cover));
        } else if let Some(p) = self.data::<Pager<Artist>>(key).map(|p| &p.items) {
            big.extend(p[tiles(p.len())].iter().map(|a| &a.cover));
        } else if let Some(r) = self.data::<SearchRes>(key) {
            big.extend(r.artists.iter().take(cols).map(|a| &a.cover));
            big.extend(r.albums.iter().take(cols).map(|a| &a.cover));
            small.extend(r.songs.iter().take(4).map(|s| &s.cover));
        } else if let Some(h) = self.data::<Home>(key) {
            let pls = lib.map_or(&[][..], |l| &l.playlists[..]);
            small.extend(pls.iter().take(3).map(|p| &p.cover));
            small.extend(h[0].iter().take(7).map(|a| &a.cover));
            let hs = cw + 132.0;
            let near = |n: usize| (off - hs..off + view + hs).contains(&(184.0 + n as f32 * hs));
            for (_, l) in h.iter().enumerate().filter(|x| near(x.0)) {
                big.extend(l.iter().take(cols).map(|a| &a.cover));
            }
            big.extend(pls.iter().take(cols).filter(|_| near(4)).map(|p| &p.cover));
        }
        let now = self.current_song().map(|s| &s.cover);
        small.extend(now);
        let ahead = [0, 1, 100][self.panel as usize];
        let next = (self.index + 1..self.queue.len()).take(ahead);
        small.extend(next.map(|n| &self.queue[self.real_at(n)].cover));
        let (_, rows, span) = crate::view::lib_rows(self);
        small.extend(rows[span].iter().map(|r| r.0));
        let now = Vec::from_iter(now.filter(|_| self.panel == 1 || self.full));
        let side = [self.right_w() - 32.0, 300.0][usize::from(self.full)];
        let art = crate::pages::art_px(self);
        let lists = [(big, cw - 24.0), (head, art), (now, side), (small, 56.0)];
        let lists = lists.into_iter().map(|(v, px)| (v, self.bucket(px)));
        let all = lists.chain([(huge, 640)]);
        all.flat_map(|(v, b)| v.into_iter().map(move |c| (c.clone(), b)))
            .collect()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        crate::view::view(self)
    }

    pub fn title(&self) -> String {
        let s = self.current_song();
        s.map_or("Navify".into(), |s| {
            format!("{} \u{2022} {}", s.title, s.artist)
        })
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        use iced::keyboard::Event as K;
        use iced::mouse::{Button, Event as M};
        use iced::{Event, event, window::Event as W};
        let mut subs = vec![
            event::listen_with(|e, status, _| match e {
                Event::Mouse(M::CursorMoved { position: p }) => {
                    CURSOR.store((p.x.to_bits() as u64) << 32 | p.y.to_bits() as u64, Relaxed);
                    None
                }
                Event::Keyboard(K::ModifiersChanged(m)) => {
                    MODS.store(m.bits(), Relaxed);
                    None
                }
                Event::Window(W::Focused) => Some(Message::Focus(true)),
                Event::Window(W::Unfocused) => Some(Message::Focus(false)),
                Event::Window(W::CloseRequested) => Some(Message::CloseWindow),
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
                Event::Mouse(M::CursorMoved { position }) => Some(Message::DragTo(position.x)),
                Event::Mouse(M::ButtonReleased(_)) => Some(Message::Drag(None)),
                _ => None,
            }));
        }
        if self.playing && !self.hidden {
            let ms = if self.route == "lyrics" { 250 } else { 1000 };
            subs.push(iced::time::every(Duration::from_millis(ms)).map(|_| Message::Tick));
        }
        iced::Subscription::batch(subs)
    }
}

fn keys(key: &Key, m: Modifiers, status: iced::event::Status) -> Option<Message> {
    let k = match key {
        Key::Named(n) => format!("{n:?}"),
        Key::Character(c) => c.to_lowercase(),
        Key::Unidentified => return None,
    };
    let vol = |d: f32| task(move |a| a.update(Message::Vol((a.volume + d).clamp(0.0, 1.0))));
    let seek = |d: f32| {
        let to = move |a: &App| (a.pos() + d * a.cfg.settings.seek_secs).max(0.0);
        task(move |a| a.update(Message::Seek(to(a))))
    };
    let like = |a: &mut App| {
        let s = a
            .current_song()
            .map(|s| (s.id.clone(), !a.is_starred(&s.id, s.starred)));
        Task::batch(s.map(|(id, on)| a.update(Message::Star(vec![id], on))))
    };
    Some(match ((m.command(), m.alt(), m.shift()), k.as_str()) {
        (_, "Escape") => Message::MenuClose,
        _ if status == iced::event::Status::Captured => return None,
        ((true, ..), "k" | "l") => task(|_| op::focus("search")),
        ((true, ..), "s") => Message::Shuffle,
        ((true, ..), "r") => Message::Repeat,
        ((true, ..), "a") => Message::SelectAll,
        ((true, ..), ",") => Message::Nav("settings".into()),
        ((true, ..), "/") => task(|a| {
            a.modal = Some((String::new(), 3));
            Task::none()
        }),
        ((true, ..), "q") => Message::Close,
        ((true, ..), "w") => Message::Window(false),
        ((true, ..), "ArrowLeft") => Message::Prev,
        ((true, ..), "ArrowRight") => Message::Next(true),
        ((true, ..), "ArrowUp") => vol(0.1),
        ((true, ..), "ArrowDown") => vol(-0.1),
        ((true, ..), _) => return None,
        ((_, _, shift), "Tab") => Message::FocusMove(shift),
        (_, "Space") => Message::Toggle,
        ((_, true, _), "ArrowLeft") => Message::Back,
        ((_, true, _), "ArrowRight") => Message::Fwd,
        ((_, _, true), "ArrowLeft") => seek(-1.0),
        ((_, _, true), "ArrowRight") => seek(1.0),
        ((_, true, true), "h") => Message::Nav("home".into()),
        ((_, true, true), "s") => Message::Nav("liked".into()),
        ((_, true, true), "j") => Message::Panel(1),
        ((_, true, true), "q") => Message::Panel(2),
        ((_, true, true), "l") => Message::LibCollapse,
        ((_, true, true), "b") => task(like),
        (_, "ArrowUp") => Message::Arrow(true),
        (_, "ArrowDown") => Message::Arrow(false),
        (_, "Enter") => Message::Enter,
        (_, "Delete" | "Backspace") => Message::DeleteSel,
        _ => return None,
    })
}

fn decode(raw: bytes::Bytes, size: u32, wide: bool) -> Result<Decoded, String> {
    use ::image::imageops::FilterType;
    let mut img = ::image::load_from_memory(&raw).map_err(|e| e.to_string())?;
    let h = (img.width() / 3).min(img.height());
    if wide {
        img = img.crop_imm(0, (img.height() - h) * 3 / 10, img.width(), h);
    }
    if img.width() > size || img.height() > size {
        img = img.thumbnail(size, size);
    }
    let small = img.resize_exact(24, 24, FilterType::Triangle).to_rgb8();
    let (mut hue, mut all) = ([[0f32; 4]; 12], [0f32; 3]);
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
    let top = hue.into_iter().max_by(|a, b| a[0].total_cmp(&b[0]));
    let [w, r, g, b] = top.expect("12 hue bins");
    let l = ((all[0] * 0.3 + all[1] * 0.59 + all[2] * 0.11) / 576.0).clamp(0.15, 0.35);
    let k = 0.5 / (r / w).max(g / w).max(b / w).max(0.05) / w;
    let color = match w < 576.0 * 0.03 {
        true => Color::from_rgb(l, l, l),
        false => Color::from_rgb((r * k).min(1.0), (g * k).min(1.0), (b * k).min(1.0)),
    };
    if !wide {
        return Ok((color, image::Handle::from_bytes(raw.clone()), raw.len()));
    }
    let (w, h, px) = (img.width(), img.height(), img.into_rgba8().into_raw());
    let n = (w * h * 4) as usize;
    Ok((color, image::Handle::from_rgba(w, h, px), n))
}

fn pager<T: Send + Sync + 'static>((items, total): (Vec<T>, usize), offset: usize) -> AnyData {
    Arc::new(Pager {
        items,
        total,
        offset,
    })
}

fn fetch(s: Session, key: String, offset: usize) -> Task<Message> {
    let k = key.clone();
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
                let (playlists, (artists, albums, _)) =
                    tokio::try_join!(s.playlists(), s.starred())?;
                Arc::new(Lib {
                    playlists,
                    artists,
                    albums,
                })
            }
            "album" => Arc::new(s.album(arg).await?),
            "artist" => Arc::new(s.artist(arg).await?),
            "top" => Arc::new(s.top_songs(arg).await?),
            "info" => Arc::new(s.artist_info(arg).await?),
            "appears" => Arc::new(s.appears(arg).await?),
            "banner" => Arc::new(s.banner(arg).await?),
            "playlist" => Arc::new(s.playlist(arg).await?),
            "liked" => pager(s.songs("starred", offset, PAGE).await?, offset),
            "albums" => pager(s.albums(or("newest"), &[], offset, PAGE).await?, offset),
            "artists" => pager(s.artists(offset, PAGE).await?, offset),
            "songs" => pager(s.songs(or("name"), offset, PAGE).await?, offset),
            "genre" => {
                let f = [("genre", arg)];
                pager(s.albums("newest", &f, offset, PAGE).await?, offset)
            }
            "genres" => Arc::new(s.genres().await?),
            "folders" => Arc::new(s.indexes().await?),
            "folder" => Arc::new(s.directory(arg).await?),
            "lyrics" => Arc::new(s.lyrics(arg).await?),
            "search" => {
                let (artists, albums, songs) = s.search(arg, 20).await?;
                Arc::new(SearchRes {
                    artists,
                    albums,
                    songs,
                })
            }
            _ => anyhow::bail!("unknown key {k}"),
        })
    };
    Task::perform(load, move |r| {
        Message::Loaded(key.clone(), r.map_err(|e| e.to_string()))
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
    let album = v
        .downcast_ref::<(Album, Vec<Song>)>()
        .map(|(a, s)| (s, a.name.as_str()));
    let pl = || {
        v.downcast_ref::<(Playlist, Vec<Song>)>()
            .map(|(p, s)| (s, p.name.as_str()))
    };
    let page = || v.downcast_ref::<Pager<Song>>().map(|p| (&p.items, "Songs"));
    let search = || v.downcast_ref::<SearchRes>().map(|r| (&r.songs, "Search"));
    let folder = || v.downcast_ref::<Vec<Song>>().map(|s| (s, "Folder"));
    let l = album
        .or_else(pl)
        .or_else(page)
        .or_else(search)
        .or_else(folder);
    l.map(|(s, n)| (&s[..], n))
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

pub static CURSOR: AtomicU64 = AtomicU64::new(0);
static MODS: AtomicU32 = AtomicU32::new(0);

type Rx = Mutex<Option<UnboundedReceiver<Message>>>;
static CH: LazyLock<(UnboundedSender<Message>, Rx)> = LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, Mutex::new(Some(rx)))
});
