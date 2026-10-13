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
use tokio::time::sleep;

use crate::api::{Album, Artist, Info, Playlist, Session, Song};
use crate::config::{self, Config};

pub const PAGE: usize = 100;
const STALE: Duration = Duration::from_secs(300);
const COVER_CAP: usize = 64 * 1024 * 1024;
const SIZES: [u32; 3] = [128, 300, 640];
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
type Decoded = (u32, u32, Color, Vec<u8>);
type Slot = Option<(u64, image::Handle, usize)>;

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

type Run = dyn Fn(&mut App) -> Task<Message> + Send + Sync;

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

fn say(text: String) -> Message {
    task(move |a| a.toast(&text))
}

#[derive(Clone, Debug)]
pub enum Message {
    Loaded(String, Result<AnyData, String>),
    Cover(String, u32, Result<Decoded, String>),
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
    MenuSub(u8),
    MenuClose,
    PlAdd(String, Vec<String>),
    PlNew(String, Vec<String>),
    PlDel(String),
    PlRemove(Vec<usize>),
    Radio(String),
    Panel(u8),
    LibFilter(u8),
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
    SeekPreview(f32),
    SeekRelease,
    Audio(navify_audio::Event),
    Media(souvlaki::MediaControlEvent),
    MediaInit(isize),
    Window(bool),
    CloseWindow,
    Win(iced::window::Id),
}

#[derive(Debug, Default)]
pub struct Covers {
    map: HashMap<String, [Slot; 3]>,
    pub bytes: usize,
    tick: u64,
    pub color: HashMap<String, Color>,
    want: HashSet<(String, u32)>,
}

fn slot(size: u32) -> usize {
    SIZES
        .iter()
        .position(|s| *s == size)
        .unwrap_or_else(|| panic!("cover size {size} is not a bucket"))
}

impl Covers {
    pub fn get(&self, id: &str, size: u32) -> Option<image::Handle> {
        let e = self.map.get(id)?;
        let mut near = [0, 1, 2];
        near.sort_unstable_by_key(|&j| SIZES[j].abs_diff(size));
        near.iter()
            .find_map(|&j| e[j].as_ref())
            .map(|x| x.1.clone())
    }

    fn insert(&mut self, id: String, size: u32, (w, h, c, px): Decoded) {
        self.tick += 1;
        self.color.insert(id.clone(), c);
        let n = (w * h * 4) as usize;
        let e = &mut self.map.entry(id).or_default()[slot(size)];
        let old = e.replace((self.tick, image::Handle::from_rgba(w, h, px), n));
        self.bytes = self.bytes + n - old.map_or(0, |o| o.2);
        while self.bytes > COVER_CAP {
            let (_, id, j) = self
                .map
                .iter()
                .flat_map(|(id, e)| (0..3).filter_map(move |j| Some((e[j].as_ref()?.0, id, j))))
                .min()
                .map(|(t, id, j)| (t, id.clone(), j))
                .expect("the counted bytes belong to cached covers");
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

fn login(u: String, n: String, p: String) -> Task<Message> {
    Task::perform(
        async move { Session::login(&u, &n, &p).await.map_err(|e| e.to_string()) },
        Message::LoggedIn,
    )
}

impl App {
    pub fn new() -> Self {
        let cfg = Config::load();
        let shot = std::env::var("NAVIFY_SHOT").ok().filter(|s| !s.is_empty());
        let (session, route) = cfg
            .active
            .as_ref()
            .and_then(|id| cfg.servers.iter().find(|s| &s.id == id))
            .map_or((None, "login"), |s| (Some(Session::new(s.clone())), "home"));
        let route = shot_key(shot.as_deref()).unwrap_or(route.into());
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

    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self::new();
        let env = |k| std::env::var(k).ok();
        let creds = env("NAVIFY_URL")
            .zip(env("NAVIFY_USER"))
            .zip(env("NAVIFY_PASS"));
        let open = match app.shot {
            Some(_) => Task::none(),
            None => app.open(),
        };
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
        let songs = self
            .cache
            .values()
            .filter_map(|(_, v)| song_list(v))
            .find(|(s, _)| !s.is_empty())
            .map_or(Vec::new(), |(s, _)| s.to_vec());
        let setups = self.shot.take().unwrap_or_default();
        let mut task = Task::none();
        for s in setups.split_once('|').map_or("", |x| x.1).split(',') {
            match s {
                "queue" | "panel1" => {
                    (self.queue, self.index) = (songs.iter().take(20).cloned().collect(), 0);
                    self.source = "Liked Songs".into();
                    self.panel = if s == "queue" { 2 } else { 1 };
                }
                "menu" => {
                    self.menu = songs.first().map(|sg| Menu {
                        x: 480.0,
                        y: 300.0,
                        kind: MenuKind::Song(vec![sg.clone()]),
                        sub: 0,
                    })
                }
                "collapsed" => self.cfg.settings.lib_collapsed = true,
                "toast" => self.toast = Some((u64::MAX, "Added to queue".into())),
                "modal1" | "modal2" => {
                    let lib = self.data::<Lib>("lib");
                    let id = lib.and_then(|l| l.playlists.first()).map(|p| p.id.clone());
                    let m = Message::Modal(
                        id.expect("the library has a playlist"),
                        s.as_bytes()[5] - b'0',
                    );
                    task = self.update(m);
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
        let h = self.view_size().1 - 136.0;
        self.scrolls.get(&self.route).copied().unwrap_or((0.0, h))
    }

    pub fn scale(&self) -> f32 {
        self.cfg.settings.zoom
    }

    fn open(&self) -> Task<Message> {
        let icon = ::image::load_from_memory(ICON).expect("the bundled icon is a PNG");
        let icon = icon.into_rgba8();
        let ((w, h), s) = (self.cfg.settings.window, self.scale());
        let settings = iced::window::Settings {
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
        };
        iced::window::open(settings).1.map(Message::Win)
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
        let right = if self.panel > 0 {
            self.right_w() + 8.0
        } else {
            0.0
        };
        self.view_size().0 - 24.0 - self.lib_w() - right
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
        let name = if key == "liked" { "Liked Songs" } else { name };
        Some((s.to_vec(), name.to_string()))
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
        self.edit::<Lib>("lib", |l| {
            l.playlists.iter_mut().filter(|p| p.id == id).for_each(f)
        });
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
            let home = v.downcast_ref::<Home>();
            let home = home.map(|h| [&h.recent[..], &h.added[..], &h.frequent[..], &h.random[..]]);
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
            .chain(home.into_iter().flatten())
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

    fn star(&mut self, ids: &[String], on: bool) {
        for id in ids {
            self.starred.insert(id.clone(), on);
            if let Some(mut s) = self.find_song(id) {
                s.starred = on;
                self.edit::<Pager<Song>>("liked", |p| {
                    let had = p.items.iter().any(|x| x.id == s.id);
                    p.items.retain(|x| x.id != s.id);
                    p.total = (p.total + on as usize).saturating_sub(had as usize);
                    if on {
                        p.items.insert(0, s);
                    }
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
            assert_eq!(
                o.len(),
                self.queue.len(),
                "the shuffle order covers the queue"
            );
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
        Task::perform(sleep(Duration::from_secs(3)), move |_| {
            act(move |a| a.toast = a.toast.take().filter(|t| t.0 != n))
        })
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
        Task::perform(sleep(Duration::from_secs(5)), move |_| {
            task(move |a| match n == a.save_seq {
                true => a.save_queue(),
                false => Task::none(),
            })
        })
    }

    fn save_queue(&self) -> Task<Message> {
        if !self.cfg.settings.queue_sync {
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
        10f32
            .powf((db.unwrap_or(st.fallback_db) + st.preamp_db) / 20.0)
            .min(1.0 / peak)
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
            crate::api::cache_files(&dir)
                .iter()
                .map(|f| f.2)
                .sum::<u64>()
        });
        Task::perform(scan, |n| {
            Message::Loaded(
                "disk".into(),
                Ok(Arc::new(n.expect("cache scan")) as AnyData),
            )
        })
    }

    fn next_index(&self, manual: bool) -> Option<usize> {
        let (i, len) = (self.index, self.queue.len());
        if self.repeat == 2 && !manual {
            return (i < len).then_some(i);
        }
        (i + 1 < len)
            .then_some(i + 1)
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
        (self.index, self.playing) = (i.min(self.queue.len() - 1), true);
        let t = self.track(&self.queue[self.real_at(self.index)]);
        if let Some(p) = &self.player {
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
        let now = match self.cfg.settings.scrobble {
            true => self.send(
                move |s| async move { s.scrobble(&song.id, false).await },
                "Could not send the playing track to the server.",
            ),
            false => Task::none(),
        };
        Task::batch([lyr, now, self.save_later()])
    }

    fn pos(&self) -> f32 {
        let p = self.player.as_ref().filter(|_| self.loaded);
        p.map_or(self.position, |p| p.position())
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
            return self.send(
                move |s| async move { s.scrobble(&id, true).await },
                "Could not send the played track to the server.",
            );
        }
        let n = self.listen_seq;
        let heard = task(move |a| match n == a.listen_seq {
            true => a.listen(a.since.is_some()),
            false => Task::none(),
        });
        match on {
            true => Task::perform(sleep(Duration::from_secs_f32(left / speed)), move |_| heard),
            false => Task::none(),
        }
    }

    fn restore(&self) -> Task<Message> {
        let ok = self.cfg.settings.queue_sync && self.shot.is_none();
        let Some(s) = self.session.clone().filter(|_| ok) else {
            return Task::none();
        };
        Task::perform(async move { s.play_queue().await }, |r| match r {
            Ok((songs, cur, pos)) => act(move |a| {
                if !songs.is_empty() && !a.loaded {
                    a.position = pos as f32 / 1000.0;
                    a.index = songs.iter().position(|s| s.id == cur).unwrap_or(0);
                    (a.queue, a.shuffle) = (songs.clone(), None);
                }
            }),
            Err(e) => say(format!("Could not restore the play queue. {e}")),
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
        let have =
            |c: &Covers, id: &str, size| c.map.get(id).is_some_and(|e| e[slot(size)].is_some());
        Task::batch(
            ids.into_iter()
                .filter(|(id, size)| {
                    !id.is_empty()
                        && !have(&self.covers, id, *size)
                        && self.covers.want.insert((id.clone(), *size))
                })
                .map(|(id, size)| {
                    let (s, k) = (s.clone(), id.clone());
                    let load = async move {
                        let b = s.cover(&k, size).await.map_err(|e| e.to_string())?;
                        let d = tokio::task::spawn_blocking(move || decode(&b, size));
                        d.await.expect("decode task")
                    };
                    Task::perform(load, move |r| Message::Cover(id.clone(), size, r))
                })
                .collect::<Vec<_>>(),
        )
    }

    fn send<F>(&self, f: impl FnOnce(Session) -> F, fail: &'static str) -> Task<Message>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let Some(s) = self.session.clone() else {
            return Task::none();
        };
        Task::perform(f(s), move |r| match r {
            Ok(()) => Message::Noop,
            Err(e) => say(format!("{fail} {e}")),
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
        let mut tasks: Vec<_> = keys.iter().map(|k| self.request(k, 0)).collect();
        tasks.push(self.covers_now());
        Task::batch(tasks)
    }

    fn scroll_to(&self, y: f32) -> Task<Message> {
        op::scroll_to(crate::view::SCROLL, AbsoluteOffset { x: 0.0, y })
    }

    fn show(&mut self) -> Task<Message> {
        let route = self.history[self.hi].clone();
        self.route = route.clone();
        (self.menu, self.hover, self.sel, self.picked) = (None, None, None, BTreeSet::new());
        (self.popular_all, self.disc_chip) = (false, 0);
        let key = self.key_of(&route);
        let parts = route.strip_prefix("artist:").map_or(Vec::new(), |id| {
            ["top", "info", "appears"]
                .map(|k| format!("{k}:{id}"))
                .to_vec()
        });
        self.cache.retain(|k, (t, _)| {
            t.elapsed() < STALE || *k == key || k == "lib" || parts.contains(k)
        });
        self.seed(&key);
        let keys = [key.as_str(), "lib"]
            .into_iter()
            .chain(parts.iter().map(String::as_str));
        let mut tasks: Vec<_> = keys.map(|k| self.request(k, 0)).collect();
        tasks.push(self.disc_load());
        tasks.push(self.scroll_to(self.scrolls.get(&route).map_or(0.0, |s| s.0)));
        if route == "login" {
            let n = [&self.login.url, &self.login.user]
                .iter()
                .position(|v| v.is_empty());
            tasks.push(op::focus(format!("login{}", n.unwrap_or(2))));
        }
        if route == "settings" {
            let scan = tokio::task::spawn_blocking(navify_audio::devices);
            tasks.push(Task::perform(scan, |d| {
                let d = d.expect("device scan").map(|v| Arc::new(v) as AnyData);
                Message::Loaded("devices".into(), d)
            }));
            tasks.push(self.disk(None));
        }
        Task::batch(tasks)
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
                .map(|a| Arc::new((a, Vec::<Song>::new())) as _),
            "artist" => self
                .find_artist(id)
                .map(|a| Arc::new((a, Vec::<Album>::new())) as _),
            "playlist" => self
                .data::<Lib>("lib")
                .and_then(|l| l.playlists.iter().find(|p| p.id == id))
                .map(|p| Arc::new((p.clone(), Vec::<Song>::new())) as _),
            _ => None,
        };
        self.seeds.extend(v.map(|v| (key.to_string(), v)));
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
        let heard = self.loaded && (o.speed, o.scrobble) != (n.speed, n.scrobble);
        let (grid, zoom) = (o.disc_grid != n.disc_grid, o.zoom != n.zoom);
        if gain(&o) != gain(n)
            && let (Some(p), Some(s)) = (&self.player, self.current_song())
        {
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
            let n = self.next_index(false);
            p.queue_next(n.map(|n| self.track(&self.queue[self.real_at(n)])));
        }
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        let seek = matches!(m, Message::Seek(_));
        let t = self.handle(m);
        if let Some(p) = &self.player {
            p.set_volume(if self.muted { 0.0 } else { self.volume });
            p.set_speed(self.cfg.settings.speed);
            p.set_crossfade(self.cfg.settings.crossfade_secs);
        }
        let id = self.current_song().map_or("", |s| s.id.as_str());
        if (id, self.playing) == (self.os.0.as_str(), self.os.1) && !seek {
            return t;
        }
        self.os = (id.to_string(), self.playing);
        let (song, pos) = (self.current_song().cloned(), self.pos());
        let song = song.as_ref();
        if let Some(m) = &mut self.media {
            let art = song
                .zip(self.session.as_ref())
                .map(|(s, x)| x.cover_file(&s.cover, 300))
                .filter(|f| f.exists())
                .map(|f| format!("file://{}", f.display()));
            let _ = m.set_metadata(souvlaki::MediaMetadata {
                title: song.map(|s| s.title.as_str()),
                album: song.map(|s| s.album.as_str()),
                artist: song.map(|s| s.artist.as_str()),
                cover_url: art.as_deref(),
                duration: song.map(|s| Duration::from_secs_f32(s.duration.max(0.0))),
            });
            let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f32(
                pos.max(0.0),
            )));
            let _ = m.set_playback(match (song, self.playing) {
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
                match r {
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
                    }
                    Ok(v) => {
                        self.seeds.remove(&key);
                        let v = self.merge(&key, v);
                        self.cache.insert(key.clone(), (Instant::now(), v));
                        let setup = shot_key(self.shot.as_deref()).is_some_and(|k| k == key);
                        let setup = if setup {
                            self.apply_shot_setup()
                        } else {
                            Task::none()
                        };
                        return Task::batch([self.disc_load(), setup]);
                    }
                }
            }
            Message::Cover(id, size, r) => {
                self.covers.want.remove(&(id.clone(), size));
                match r {
                    Ok(d) => self.covers.insert(id, size, d),
                    Err(e) => return self.toast(&format!("Could not load a cover. {e}")),
                }
            }
            Message::Nav(r) => return self.nav(r),
            Message::Back | Message::Fwd => {
                let h = match m {
                    Message::Back => self.hi.wrapping_sub(1),
                    _ => self.hi + 1,
                };
                if h < self.history.len() {
                    self.hi = h;
                    return self.show();
                }
            }
            Message::Scrolled(off, view, content) => {
                let key = self.route.clone();
                self.scrolls.insert(key.clone(), (off, view));
                let more = match self.page_len(&key) {
                    Some(n) if content > 0.0 && off + view * 3.0 >= content => {
                        self.request(&key, n)
                    }
                    _ => Task::none(),
                };
                return Task::batch([more, self.disc_load()]);
            }
            Message::Search(q) => {
                (self.search_q, self.search_seq) = (q, self.search_seq + 1);
                let n = self.search_seq;
                return Task::perform(sleep(Duration::from_millis(250)), move |_| {
                    Message::SearchGo(n)
                });
            }
            Message::SearchGo(n) if n == self.search_seq => {
                let q = self.search_q.trim();
                let r = if q.is_empty() {
                    "search".into()
                } else {
                    format!("search:{q}")
                };
                return if r != self.route {
                    self.nav(r)
                } else {
                    self.request(&r, 0)
                };
            }
            Message::SearchGo(_) => {}
            Message::Play(songs, i, source) => {
                (self.queue, self.source) = (songs, source);
                self.ctx.clear();
                return self.start(i, self.shuffle.is_some());
            }
            Message::PlayQueue(i) => return self.play_index(i, 0.0),
            Message::Toggle if !self.loaded => return self.play_index(self.index, self.position),
            Message::Toggle => {
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
                self.position = pos;
                if let Some(p) = &self.player {
                    p.seek(pos);
                }
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
                let (bits, speed) = (CURSOR.load(Relaxed), matches!(*kind, MenuKind::Speed));
                let (x, y) = (
                    f32::from_bits((bits >> 32) as u32),
                    f32::from_bits(bits as u32),
                );
                self.menu = Some(Menu {
                    x,
                    y,
                    kind: *kind,
                    sub: 0,
                });
                if speed {
                    let row = (self.cfg.settings.speed * 10.0).round() - 5.0;
                    let y = (row * 40.0 - (crate::pages::SPEED_H - 88.0) / 2.0).max(0.0);
                    return op::scroll_to("speed", AbsoluteOffset { x: 0.0, y });
                }
            }
            Message::MenuSub(n) => self.menu.iter_mut().for_each(|m| m.sub = n),
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
                let name = self
                    .data::<Lib>("lib")
                    .and_then(|l| l.playlists.iter().find(|p| p.id == pid))
                    .map_or(String::new(), |p| p.name.clone());
                let add = |s: Session| async move {
                    s.playlist_update(&pid, None, None, &ids, &[]).await
                };
                return Task::batch([
                    self.send(add, "Could not add the song to the playlist."),
                    self.toast(&format!("Added to {name}.")),
                ]);
            }
            Message::PlNew(name, ids) => {
                self.menu = None;
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
                let make = async move { s.playlist_create(&name, &ids).await };
                return Task::perform(make, |r| match r {
                    Ok(p) => task(move |a| {
                        a.edit::<Lib>("lib", |l| l.playlists.insert(0, p.clone()));
                        a.nav(format!("playlist:{}", p.id))
                    }),
                    Err(e) => say(format!("Could not make the playlist. {e}")),
                });
            }
            Message::PlDel(id) => {
                (self.menu, self.modal) = (None, None);
                let r = format!("playlist:{id}");
                self.edit::<Lib>("lib", |l| l.playlists.retain(|p| p.id != id));
                self.cache.remove(&r);
                let back = if self.route == r {
                    self.nav("home".into())
                } else {
                    Task::none()
                };
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
                return self.send(
                    |s| async move { s.playlist_update(&pid, None, None, &[], &idx).await },
                    "Could not remove the songs from the playlist.",
                );
            }
            Message::Radio(id) => {
                self.menu = None;
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
                let similar = async move { s.similar_songs(&id, 30).await };
                return Task::perform(similar, |r| match r {
                    Ok(v) if !v.is_empty() => Message::Play(v, 0, "Radio".into()),
                    Ok(_) => say("No similar songs were found.".into()),
                    Err(e) => say(format!("Could not start the radio. {e}")),
                });
            }
            Message::Panel(n) => {
                self.panel = if self.panel == n { 0 } else { n };
                return self.covers_now();
            }
            Message::LibFilter(n) => self.lib_filter = if self.lib_filter == n { 0 } else { n },
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
                let kept = match self.login.remember {
                    true => config::remember(&s.server.id, &self.login.pass),
                    false => Ok(()),
                };
                self.cfg.servers.retain(|x| x.id != s.server.id);
                self.cfg.servers.push(s.server.clone());
                self.cfg.active = Some(s.server.id.clone());
                (self.session, self.login) = (Some(s), Login::default());
                self.route.clear();
                self.history.clear();
                let to = shot_key(self.shot.as_deref()).unwrap_or_else(|| "home".into());
                return Task::batch([
                    self.persist(),
                    self.nav(to),
                    self.restore(),
                    self.report(kept),
                ]);
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
                self.covers = Covers {
                    want,
                    ..Default::default()
                };
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
                return Task::perform(sleep(Duration::from_millis(300)), |_| ())
                    .then(|_| iced::window::latest())
                    .and_then(iced::window::is_minimized)
                    .map(|m| act(move |a| a.hidden = m == Some(true)));
            }
            Message::Close => {
                return Task::batch([self.persist(), self.save_queue()])
                    .collect()
                    .then(|_| iced::exit());
            }
            Message::CloseWindow => {
                let hide = self.tray.is_some() && self.cfg.settings.tray;
                return self.update(if hide {
                    Message::Window(false)
                } else {
                    Message::Close
                });
            }
            Message::Window(false) if self.tray.is_none() => {
                return self
                    .win
                    .map_or(Task::none(), |id| iced::window::minimize(id, true));
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
                let close = match cfg!(windows) {
                    true => w::set_mode(id, Mode::Hidden),
                    false => {
                        self.win = None;
                        w::close(id)
                    }
                };
                return Task::batch([self.persist(), self.save_queue(), close]);
            }
            Message::Win(id) => {
                use iced::window::raw_window_handle::RawWindowHandle;
                self.win = Some(id);
                if self.tray.is_none() {
                    self.tray = crate::tray::spawn(CH.0.clone())
                        .inspect_err(|e| eprintln!("The tray icon is not available. {e}"))
                        .ok();
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
            Message::Noop => {}
            Message::PlayCtx(route, mode) => {
                self.menu = None;
                if mode < 2 {
                    self.ctx = route.clone();
                }
                if let Some((v, src)) = self.songs_of(&route).filter(|(v, _)| !v.is_empty()) {
                    return self.play_songs(v, src, mode);
                }
                let Some(s) = self.session.clone() else {
                    return Task::none();
                };
                let load = async move {
                    let (kind, id) = route.split_once(':').unwrap_or((&route, ""));
                    Ok::<_, anyhow::Error>(match kind {
                        "album" => s.album(id).await.map(|(a, v)| (v, a.name))?,
                        "playlist" => s.playlist(id).await.map(|(p, v)| (v, p.name))?,
                        "artist" => {
                            let ((a, _), t) = tokio::try_join!(s.artist(id), s.top_songs(id))?;
                            (t, a.name)
                        }
                        "liked" => (
                            s.songs("starred", 0, PAGE * 5).await?.0,
                            "Liked Songs".into(),
                        ),
                        "songs" => {
                            let sort = if id.is_empty() { "name" } else { id };
                            (s.songs(sort, 0, PAGE * 5).await?.0, "Songs".into())
                        }
                        _ => (Vec::new(), String::new()),
                    })
                };
                return Task::perform(load, move |r| match r {
                    Ok((v, src)) if !v.is_empty() => {
                        task(move |a| a.play_songs(v.clone(), src.clone(), mode))
                    }
                    Ok(_) => say("Nothing to play.".into()),
                    Err(e) => say(format!("Could not play. {e}")),
                });
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
                return self.update(Message::Menu(Box::new(
                    match key.starts_with("playlist:") {
                        true => MenuKind::SongIn(songs, idx),
                        false => MenuKind::Song(songs),
                    },
                )));
            }
            Message::SelectAll => {
                if let Some((list, _)) = self.songs_of(&self.route) {
                    self.sel = Some((self.route.clone(), 0));
                    (self.picked, self.anchor) = ((0..list.len()).collect(), 0);
                }
            }
            Message::DeleteSel => {
                if self.sel.as_ref().is_some_and(|s| s.0 == self.route)
                    && self.route.starts_with("playlist:")
                {
                    return self.update(Message::PlRemove(self.picked.iter().copied().collect()));
                }
            }
            Message::PlayAt(key, n) => {
                let Some((list, src)) = self.songs_of(&key) else {
                    return self.update(Message::PlayCtx(key, 0));
                };
                let n = n - list[..n.min(list.len())]
                    .iter()
                    .filter(|s| s.is_dir)
                    .count();
                let list = list.into_iter().filter(|s| !s.is_dir).collect();
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
                return layer_focus(
                    self.modal.is_some() || self.menu.is_some(),
                    move |cur, n| match cur {
                        Some(i) => (i + if back { n - 1 } else { 1 }) % n,
                        None if back => n - 1,
                        None => 0,
                    },
                );
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
                let discs = match kind == "album" && list.iter().any(|s| s.disc != list[0].disc) {
                    true => {
                        1 + list[..=n]
                            .windows(2)
                            .filter(|w| w[0].disc != w[1].disc)
                            .count()
                    }
                    false => 0,
                };
                let top =
                    crate::pages::art_px(self) + 2.0 * PAD + BAR + HEAD + (n + discs) as f32 * ROW;
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
            Message::SeekPreview(v) => self.seek_preview = Some(v),
            Message::SeekRelease => {
                if let Some(v) = self.seek_preview.take() {
                    return self.update(Message::Seek(v));
                }
            }
            Message::Tick => self.position = self.pos(),
            Message::Modal(id, kind) => {
                self.menu = None;
                let p = self
                    .data::<Lib>("lib")
                    .and_then(|l| l.playlists.iter().find(|p| p.id == id))
                    .expect("dialogs open only for playlists in the library");
                let cover = p.cover.clone();
                (self.new_pl, self.new_pl_desc) = (p.name.clone(), p.comment.clone());
                self.modal = Some((id, kind));
                let focus = layer_focus(true, |_, n| n.min(2) - 1)
                    .chain(op::select_all(iced::widget::Id::new("layer")));
                return Task::batch([self.want_covers([(cover, 300)]), focus]);
            }
            Message::Edit(id, name, comment) => {
                self.modal = None;
                let (name, comment) = (name.trim().to_string(), comment.trim().to_string());
                assert!(!name.is_empty(), "the dialog blocks an empty name");
                let set = |p: &mut Playlist| (p.name, p.comment) = (name.clone(), comment.clone());
                self.edit_pl(&id, set);
                self.edit::<(Playlist, Vec<Song>)>(&format!("playlist:{id}"), |(p, _)| set(p));
                return self.send(
                    |s| async move {
                        let (n, c) = (Some(name.as_str()), Some(comment.as_str()));
                        s.playlist_update(&id, n, c, &[], &[]).await
                    },
                    "Could not change the playlist.",
                );
            }
            Message::MediaInit(hwnd) => {
                let tx = CH.0.clone();
                let m = souvlaki::MediaControls::new(souvlaki::PlatformConfig {
                    display_name: "navify",
                    dbus_name: "navify",
                    hwnd: (hwnd != 0).then_some(hwnd as *mut std::ffi::c_void),
                });
                let m = m.and_then(|mut m| {
                    m.attach(move |e| drop(tx.send(Message::Media(e))))
                        .map(|()| m)
                });
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
                    E::Seek(d) => by(d, 10.0),
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
                let n = self
                    .next_index(false)
                    .filter(|&n| self.queue.get(self.real_at(n)).is_some_and(|s| s.id == id));
                let began = match n {
                    Some(n) => {
                        self.index = n;
                        self.began()
                    }
                    None => Task::none(),
                };
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

    fn visible_covers<'a>(&'a self) -> Vec<(String, u32)> {
        let (off, view) = self.scroll();
        let (cols, cw) = self.grid();
        let span = |n: usize, h: f32, per: usize| {
            let lo = (off / h).max(0.0) as usize;
            (lo.saturating_sub(1) * per).min(n)..((lo + (view / h) as usize + 3) * per).min(n)
        };
        let rows = |v: &'a [Song]| {
            v[span(v.len(), crate::pages::ROW, 1)]
                .iter()
                .map(|s| &s.cover)
        };
        let key = self.route.as_str();
        let lib = self.data::<Lib>("lib");
        let (mut big, mut small): (Vec<&String>, Vec<&String>) = Default::default();
        if let Some((a, _)) = self.data::<(Album, Vec<Song>)>(key) {
            big.push(&a.cover);
            small.push(a.artists.first().map_or(&a.artist_id, |x| &x.id));
        } else if let Some((p, s)) = self.data::<(Playlist, Vec<Song>)>(key) {
            big.push(&p.cover);
            small.extend(rows(s));
        } else if let Some(p) = self.data::<Pager<Song>>(key) {
            small.extend(rows(&p.items));
        } else if let Some(s) = self.data::<Vec<Song>>(key) {
            small.extend(rows(s));
        } else if key.starts_with("discography:") {
            big.extend(crate::pages::disc_view(self).iter().map(|a| &a.cover));
        } else if let Some((a, albums)) = self.data::<(Artist, Vec<Album>)>(key) {
            let id = &key["artist:".len()..];
            let top = self.data::<Vec<Song>>(&format!("top:{id}"));
            let similar = self.data::<Info>(&format!("info:{id}"));
            let appears = self.data::<Vec<Album>>(&format!("appears:{id}"));
            big.push(&a.cover);
            small.extend(top.iter().flat_map(|v| v.iter().take(10)).map(|s| &s.cover));
            big.extend(
                crate::pages::disc_shelf(self, albums)
                    .iter()
                    .map(|x| &x.cover),
            );
            big.extend(
                similar
                    .iter()
                    .flat_map(|i| i.similar.iter().take(cols))
                    .map(|x| &x.cover),
            );
            big.extend(
                appears
                    .iter()
                    .flat_map(|v| v.iter().take(cols))
                    .map(|x| &x.cover),
            );
        } else if let Some(p) = self.data::<Pager<Album>>(key) {
            big.extend(
                p.items[span(p.items.len(), cw + 52.0, cols)]
                    .iter()
                    .map(|a| &a.cover),
            );
        } else if let Some(p) = self.data::<Pager<Artist>>(key) {
            big.extend(
                p.items[span(p.items.len(), cw + 52.0, cols)]
                    .iter()
                    .map(|a| &a.cover),
            );
        } else if let Some(r) = self.data::<SearchRes>(key) {
            big.extend(r.artists.iter().take(cols).map(|a| &a.cover));
            big.extend(r.albums.iter().take(cols).map(|a| &a.cover));
            small.extend(r.songs.iter().take(4).map(|s| &s.cover));
        } else if let Some(h) = self.data::<Home>(key) {
            let pls = lib.map_or(&[][..], |l| &l.playlists[..]);
            small.extend(pls.iter().take(3).map(|p| &p.cover));
            small.extend(h.recent.iter().take(7).map(|a| &a.cover));
            for l in [&h.recent, &h.added, &h.frequent, &h.random] {
                big.extend(l.iter().take(cols).map(|a| &a.cover));
            }
            big.extend(pls.iter().take(cols).map(|p| &p.cover));
        }
        if let Some(s) = self.current_song() {
            big.push(&s.cover);
            small.push(&s.cover);
        }
        let ahead = if self.panel == 2 {
            100
        } else {
            self.panel as usize
        };
        let next = (self.index + 1..self.queue.len()).take(ahead);
        small.extend(next.map(|n| &self.queue[self.real_at(n)].cover));
        if let Some(l) = lib {
            small.extend(l.playlists.iter().take(40).map(|p| &p.cover));
            small.extend(l.albums.iter().take(40).map(|a| &a.cover));
            small.extend(l.artists.iter().take(40).map(|a| &a.cover));
        }
        let big = big.into_iter().map(|c| (c.clone(), 300));
        big.chain(small.into_iter().map(|c| (c.clone(), 128)))
            .collect()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        crate::view::view(self)
    }

    pub fn title(&self) -> String {
        self.current_song().map_or("Navify".into(), |s| {
            format!("{} \u{2022} {}", s.title, s.artist)
        })
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        use iced::keyboard::{Event as K, key::Named};
        use iced::{Event, event, mouse, window::Event as W};
        let mut subs = vec![
            event::listen_with(|e, status, _| match e {
                Event::Mouse(mouse::Event::CursorMoved { position: p }) => {
                    CURSOR.store(
                        (u64::from(p.x.to_bits()) << 32) | u64::from(p.y.to_bits()),
                        Relaxed,
                    );
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
                Event::Keyboard(K::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                }) => Some(Message::MenuClose),
                Event::Keyboard(K::KeyPressed { key, modifiers, .. })
                    if status == event::Status::Ignored =>
                {
                    keys(&key, modifiers)
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => {
                    Some(Message::Back)
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
                    Some(Message::Fwd)
                }
                _ => None,
            }),
            iced::Subscription::run(|| {
                iced::stream::channel(8, async |mut tx| {
                    let mut rx =
                        CH.1.lock()
                            .expect("event channel")
                            .take()
                            .expect("one listener");
                    while let Some(m) = rx.recv().await {
                        let _ = iced::futures::SinkExt::send(&mut tx, m).await;
                    }
                })
            }),
        ];
        if self.drag.is_some() {
            subs.push(event::listen_with(|e, _, _| match e {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::DragTo(position.x))
                }
                Event::Mouse(mouse::Event::ButtonReleased(_)) => Some(Message::Drag(None)),
                _ => None,
            }));
        }
        if self.playing && !self.hidden {
            subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick));
        }
        iced::Subscription::batch(subs)
    }
}

fn keys(key: &Key, m: Modifiers) -> Option<Message> {
    let k = match key {
        Key::Named(n) => format!("{n:?}"),
        Key::Character(c) => c.to_lowercase(),
        Key::Unidentified => return None,
    };
    let vol = |d: f32| task(move |a| a.update(Message::Vol((a.volume + d).clamp(0.0, 1.0))));
    let seek = |d: f32| task(move |a| a.update(Message::Seek((a.pos() + d).max(0.0))));
    let faster = |d: f32| {
        task(move |a| {
            let n = ((a.cfg.settings.speed * 10.0).round() + d).clamp(5.0, 35.0) / 10.0;
            a.update(set(|s, v| s.speed = v, n))
        })
    };
    Some(match ((m.command(), m.alt(), m.shift()), k.as_str()) {
        ((true, ..), "k" | "l") => task(|_| op::focus("search")),
        ((true, ..), "s") => Message::Shuffle,
        ((true, ..), "r") => Message::Repeat,
        ((true, ..), "a") => Message::SelectAll,
        ((true, ..), ",") => Message::Nav("settings".into()),
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
        ((_, _, true), "ArrowLeft") => seek(-5.0),
        ((_, _, true), "ArrowRight") => seek(5.0),
        ((_, true, true), "h") => Message::Nav("home".into()),
        ((_, true, true), "s") => Message::Nav("liked".into()),
        ((_, true, true), "j") => Message::Panel(1),
        ((_, true, true), "q") => Message::Panel(2),
        ((_, true, true), "l") => Message::LibCollapse,
        ((_, true, true), "b") => task(|a| match a.current_song() {
            Some(s) => {
                let star = Message::Star(vec![s.id.clone()], !a.is_starred(&s.id, s.starred));
                a.update(star)
            }
            None => Task::none(),
        }),
        ((_, false, true), "<" | ",") => faster(-1.0),
        ((_, false, true), ">" | ".") => faster(1.0),
        (_, "ArrowUp") => Message::Arrow(true),
        (_, "ArrowDown") => Message::Arrow(false),
        (_, "Enter") => Message::Enter,
        (_, "Delete" | "Backspace") => Message::DeleteSel,
        _ => return None,
    })
}

fn decode(b: &[u8], size: u32) -> Result<Decoded, String> {
    let img = ::image::load_from_memory(b).map_err(|e| e.to_string())?;
    let img = match img.width() > size || img.height() > size {
        true => img.resize(size, size, ::image::imageops::FilterType::Lanczos3),
        false => img,
    };
    let small = img
        .resize_exact(24, 24, ::image::imageops::FilterType::Triangle)
        .to_rgb8();
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
    let [w, r, g, b] = hue
        .into_iter()
        .max_by(|a, b| a[0].total_cmp(&b[0]))
        .expect("12 hue bins");
    let color = if w < 576.0 * 0.03 {
        let l = ((all[0] * 0.3 + all[1] * 0.59 + all[2] * 0.11) / 576.0).clamp(0.15, 0.35);
        Color::from_rgb(l, l, l)
    } else {
        let [r, g, b] = [r / w, g / w, b / w];
        let k = 0.5 / r.max(g).max(b).max(0.05);
        Color::from_rgb((r * k).min(1.0), (g * k).min(1.0), (b * k).min(1.0))
    };
    Ok((img.width(), img.height(), color, img.to_rgba8().into_raw()))
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
                Arc::new(Home {
                    recent: r.0,
                    added: a.0,
                    frequent: f.0,
                    random: x.0,
                })
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

pub static CURSOR: AtomicU64 = AtomicU64::new(0);
static MODS: AtomicU32 = AtomicU32::new(0);

type Chan = (
    UnboundedSender<Message>,
    Mutex<Option<UnboundedReceiver<Message>>>,
);
static CH: LazyLock<Chan> = LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, Mutex::new(Some(rx)))
});
