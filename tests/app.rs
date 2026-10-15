use std::sync::Arc;

use navify::api::{Album, Song, parse};
use navify::app::{AnyData, App, Message, act};
use serde_json::json;

fn app() -> App {
    static DIR: std::sync::Once = std::sync::Once::new();
    let dir = std::env::temp_dir().join(format!("navify-app-test-{}", std::process::id()));
    DIR.call_once(|| unsafe { std::env::set_var("XDG_CONFIG_HOME", dir) });
    let mut app = App::new();
    let server = serde_json::from_value(json!({"url": "http://127.0.0.1:9"}));
    app.session = Some(navify::api::Session::new(server.expect("server")));
    app
}

fn items<T: navify::api::Data>(ids: impl IntoIterator<Item = String>) -> T {
    parse(ids.into_iter().map(|id| json!({"id": id})).collect()).expect("items")
}

#[test]
fn album_reopen_in_stale_time_sends_no_request() {
    let mut app = app();
    let (key, pend) = ("album:a1".to_string(), "album:a1#0");
    let _ = app.update(Message::Nav(key.clone()));
    assert!(app.pending.contains(pend));
    let album: Album = parse(json!({"id": "a1"})).expect("album");
    let data = Arc::new((album, Vec::<Song>::new()));
    let _ = app.update(Message::Loaded(key.clone(), 0, Ok(data)));
    let _ = app.update(Message::Nav("home".into()));
    let _ = app.update(Message::Nav(key.clone()));
    assert!(app.route == key && !app.pending.contains(pend));
    for (offset, n) in [(0, 100), (100, 50)] {
        let items: Vec<Album> = items((offset..offset + n).map(|i| i.to_string()));
        let page: AnyData = Arc::new((items, 150usize));
        let _ = app.update(Message::Loaded("albums".into(), offset, Ok(page)));
    }
    let p = app
        .data::<(Vec<Album>, usize)>("albums")
        .expect("merged page");
    assert!(p.0.len() == 150 && p.0[120].id == "120");
    let _ = app.update(Message::Nav("search:long".into()));
    assert_eq!(app.search_q, "long");
    let _ = app.update(Message::Back);
    assert!(app.search_q.is_empty());
}

#[tokio::test]
async fn shuffle_keeps_the_current_song() {
    let mut app = app();
    let songs = |ids: &str| items::<Vec<Song>>(ids.chars().map(String::from));
    let cur = |app: &App| app.current_song().expect("a song plays").id.clone();
    let _ = app.update(Message::Play(songs("abcdef"), 3, "t".into()));
    assert_eq!(cur(&app), "d");
    let _ = app.update(Message::Shuffle);
    assert_eq!(cur(&app), "d");
    let _ = app.update(Message::Play(songs("abcdef"), 2, "t".into()));
    assert_eq!(cur(&app), "c");
    let _ = app.update(Message::Queue(songs("z"), false));
    let order = app.shuffle.clone().expect("shuffle is on");
    assert_eq!(app.queue[*order.last().expect("order")].id, "z");
    let _ = app.update(Message::PlayQueue(2));
    let playing = cur(&app);
    let _ = app.update(Message::QDel(0));
    assert_eq!(cur(&app), playing);
    let _ = app.update(Message::QDel(99));
    assert!(app.real_at(99) == 99 && cur(&app) == playing);
    let _ = app.update(Message::Shuffle);
    assert!(cur(&app) == playing && app.shuffle.is_none());
    let _ = app.update(Message::Play(songs("ab"), 0, "t".into()));
    let _ = app.update(Message::Prev);
    assert!(cur(&app) == "a" && app.playing);
    let _ = app.update(Message::Next(true));
    let _ = app.update(Message::Next(false));
    assert!(cur(&app) == "a" && !app.playing && app.position == 0.0);
    assert_eq!(app.title(), "Navify");
    let _ = app.update(Message::Toggle);
    assert!(app.playing && app.title().starts_with(&app.queue[0].title));
    let _ = app.update(Message::Play(songs("abcd"), 0, "t".into()));
    app.qmove = Some((1, 3));
    let _ = app.q_drop();
    let ids: String = app.queue.iter().map(|s| s.id.as_str()).collect();
    assert!(ids == "acdb" && app.qmove.is_none());
    app.qmove = Some((0, 2));
    let _ = app.q_drop();
    assert_eq!(app.queue[0].id, "a");
}

#[tokio::test]
async fn play_button_keeps_shuffle_and_shuffle_starts_anywhere() {
    let mut app = app();
    let key = "album:a1".to_string();
    let album: Album = parse(json!({"id": "a1"})).expect("album");
    let songs = items::<Vec<Song>>("abcdef".chars().map(String::from));
    let _ = app.update(Message::Loaded(
        key.clone(),
        0,
        Ok(Arc::new((album, songs))),
    ));
    let _ = app.update(Message::PlayCtx(key.clone(), 0));
    assert!(app.shuffle.is_none());
    let _ = app.update(Message::Shuffle);
    let _ = app.update(Message::PlayCtx(key.clone(), 0));
    assert!(app.shuffle.is_some());
    let _ = app.update(Message::Shuffle);
    let first = (0..30).map(|_| {
        let _ = app.update(Message::PlayCtx(key.clone(), 1));
        app.current_song().expect("a song plays").id.clone()
    });
    assert!(first.collect::<std::collections::HashSet<_>>().len() > 1);
    assert_eq!(navify::view::fmt_dur(3725.0), "1:02:05");
}

#[test]
fn gain_follows_the_replaygain_settings() {
    let mut app = app();
    let rg = json!({"id": "s", "replayGain": {"trackGain": 6.0, "trackPeak": 1.5}});
    let song: Song = parse(rg).expect("song");
    let db = |app: &App| 20.0 * app.gain(&song).log10();
    assert!((db(&app) + 20.0 * 1.5f32.log10()).abs() < 0.01);
    app.cfg.settings.rg_clip = false;
    assert!((db(&app) - 6.0).abs() < 0.01);
    app.cfg.settings.preamp_db = -2.0;
    assert!((db(&app) - 4.0).abs() < 0.01);
    app.cfg.settings.replaygain = "off".into();
    assert_eq!(app.gain(&song), 1.0);
    let rg =
        json!({"id": "a", "replayGain": {"albumGain": 6.0, "albumPeak": 2.0, "trackPeak": 1.0}});
    let song: Song = parse(rg).expect("song");
    (app.cfg.settings.replaygain, app.cfg.settings.rg_clip) = ("album".into(), true);
    app.cfg.settings.preamp_db = 0.0;
    assert!((20.0 * app.gain(&song).log10() + 20.0 * 2.0f32.log10()).abs() < 0.01);
}

#[tokio::test]
async fn log_out_clears_the_play_state() {
    let mut app = app();
    let songs = items::<Vec<Song>>(["a", "b", "c"].map(String::from));
    let _ = app.update(Message::Play(songs, 1, "t".into()));
    let _ = app.update(Message::Shuffle);
    app.starred.insert("a".into(), true);
    let _ = app.update(Message::LogOut);
    assert!(app.queue.is_empty() && app.shuffle.is_none() && app.starred.is_empty());
    assert!(app.current_song().is_none() && app.route == "login");
}

#[tokio::test]
async fn playlist_search_and_sort_keep_the_server_order() {
    let mut app = app();
    let key = "playlist:p1".to_string();
    let _ = app.update(Message::Nav(key.clone()));
    let songs: Vec<Song> = ["c", "a", "b"]
        .map(|t| json!({"id": t, "title": t, "duration": 60}))
        .to_vec()
        .into_iter()
        .map(|v| parse(v).expect("song"))
        .collect();
    let pl: navify::api::Playlist = parse(json!({"id": "p1", "name": "P"})).expect("playlist");
    let _ = app.update(Message::Loaded(key.clone(), 0, Ok(Arc::new((pl, songs)))));
    let _ = app.update(act(|a| a.pl_sort = 1));
    let _ = app.update(Message::PlayAt(key.clone(), 0));
    let ids: String = app.queue.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, "abc");
    app.cfg.settings.play_mode = 3;
    let _ = app.update(Message::PlayAt(key.clone(), 2));
    let ids: String = app.queue.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, "acbc");
    let _ = app.update(act(|a| a.pl_find = Some("B".into())));
    let _ = app.update(Message::SelectAll);
    let _ = app.update(Message::DeleteSel);
    let (p, left) = app
        .data::<(navify::api::Playlist, Vec<Song>)>(&key)
        .expect("playlist");
    assert_eq!(left.iter().map(|s| s.id.as_str()).collect::<String>(), "ca");
    assert_eq!((p.song_count, p.duration), (2, 120.0));
}

#[tokio::test]
async fn queue_filter_skips_songs_and_replace_asks_first() {
    let mut app = app();
    let songs: Vec<Song> = ["a", "b (Live)", "c"]
        .map(|t| parse(json!({"id": t, "title": t})).expect("song"))
        .into();
    let cur = |app: &App| app.current_song().expect("a song plays").id.clone();
    app.cfg.settings.queue_skip = " live ,".into();
    let _ = app.update(Message::Play(songs.clone(), 2, "t".into()));
    assert_eq!(app.queue.len(), 2);
    assert_eq!(cur(&app), "c");
    app.cfg.settings.confirm_queue = true;
    let _ = app.update(Message::Play(songs, 0, "u".into()));
    assert!(app.modal.as_ref().is_some_and(|m| m.1 == 4) && cur(&app) == "c");
    let _ = app.update(Message::Confirm);
    assert!(app.modal.is_none() && cur(&app) == "a");
    app.sel = Some(("album:gone".into(), 3));
    let _ = app.update(Message::Arrow(false));
    let _ = app.update(Message::Queue(Vec::new(), false));
    assert!(app.toast.is_none());
    let dir: Vec<Song> = [json!({"id": "d1", "isDir": true}), json!({"id": "s1"})]
        .map(|v| parse(v).expect("entry"))
        .into();
    let fkey = "folder:f1".to_string();
    let _ = app.update(Message::Loaded(
        fkey.clone(),
        0,
        Ok(Arc::new(("F".to_string(), dir))),
    ));
    app.sel = Some((fkey.clone(), 1));
    let _ = app.update(Message::Arrow(true));
    assert_eq!(app.sel.as_ref().map(|s| s.1), Some(1));
    let _ = app.update(Message::PlayAt(fkey, 0));
    assert!(app.toast.is_none() && cur(&app) == "a");
    let _ = app.update(Message::Seek(99.0));
    assert_eq!(app.position, 0.0);
}

#[tokio::test]
async fn queue_and_pages_stay_in_order() {
    let mut app = app();
    let songs = |ids: &str| items::<Vec<Song>>(ids.chars().map(String::from));
    let ids = |app: &App| app.queue.iter().map(|s| s.id.as_str()).collect::<String>();
    let _ = app.update(Message::Play(songs("abcdef"), 1, "t".into()));
    let _ = app.update(Message::Shuffle);
    let _ = app.update(Message::Queue(songs("xy"), true));
    let _ = app.update(Message::Shuffle);
    assert_eq!(ids(&app), "abxycdef");
    let _ = app.update(Message::Next(true));
    assert_eq!(app.current_song().map(|s| s.id.as_str()), Some("x"));
    app.cfg.settings.queue_skip = "a,b,c,d,e,f".into();
    let _ = app.update(Message::Shuffle);
    let titled = ["a", "b", "c"].map(|t| parse(json!({"id": t, "title": t})).expect("song"));
    let _ = app.update(Message::Play(titled.into(), 0, "t".into()));
    let _ = app.update(Message::Queue(songs("z"), false));
    assert!(ids(&app) == "z" && app.shuffle.as_ref().is_some_and(|o| o.len() == 1));
    let _ = app.update(Message::Nav("albums".into()));
    let page = |o: usize, n: usize| -> AnyData {
        Arc::new((
            items::<Vec<Album>>((o..o + n).map(|i| i.to_string())),
            300usize,
        ))
    };
    let _ = app.update(Message::Loaded("albums".into(), 0, Ok(page(0, 100))));
    app.sel = Some(("albums".into(), 5));
    let _ = app.update(Message::Loaded("albums".into(), 100, Ok(page(100, 100))));
    let _ = app.update(Message::Loaded("albums".into(), 100, Ok(page(100, 100))));
    let p = app.data::<(Vec<Album>, usize)>("albums").expect("pages");
    assert!(p.0.len() == 200 && p.0[150].id == "150");
    assert_eq!(app.sel, Some(("albums".into(), 5)));
}

#[tokio::test]
async fn recorded_shortcut_replaces_the_old_binding() {
    let mut app = app();
    let _ = app.update(Message::Key("C+S".into()));
    assert!(app.shuffle.is_some());
    let _ = app.update(act(|a| a.recording = Some("shuffle")));
    let _ = app.update(Message::Key("C+R".into()));
    assert!(app.binding("shuffle") == "C+R" && app.binding("repeat").is_empty());
    let _ = app.update(Message::Key("C+R".into()));
    assert!(app.shuffle.is_none() && app.repeat == 0);
}

#[test]
fn cover_cache_drops_large_covers_over_the_limit() {
    let mut app = app();
    for k in 0..4 {
        let h = iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]);
        let d = (iced::Color::BLACK, h, 3 << 20);
        app.covers.insert(format!("c{k}"), 640, d);
    }
    assert!(app.covers.bytes <= 8 << 20 && app.covers.get("c3", 640).is_some());
}

#[test]
fn library_collapses_when_the_main_panel_is_too_narrow() {
    let mut app = app();
    let _ = app.update(Message::Resize(1000.0, 800.0));
    assert!(!app.lib_mini() && app.lib_w() == 300.0);
    let _ = app.update(Message::Panel(2));
    assert!(app.lib_mini() && app.main_w() >= 400.0);
}

#[test]
fn settings_check_rejects_values_out_of_range() {
    let mut s = navify::config::Settings::default();
    assert!(s.check().is_ok());
    s.crossfade_secs = 60.0;
    assert!(s.check().is_err());
    s = serde_json::from_value(json!({"zoom": 3.0})).expect("settings");
    assert!(s.check().is_err());
}
