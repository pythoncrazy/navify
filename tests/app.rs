use std::sync::Arc;

use navify::api::{Album, Song, parse};
use navify::app::{AnyData, App, Message, Pager, act};
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
    let _ = app.update(Message::Loaded(key.clone(), Ok(data)));
    let _ = app.update(Message::Nav("home".into()));
    let _ = app.update(Message::Nav(key.clone()));
    assert!(app.route == key && !app.pending.contains(pend));
    for (offset, n) in [(0, 100), (100, 50)] {
        let items = items((offset..offset + n).map(|i| i.to_string()));
        let mut page = Pager::<Album>::default();
        (page.items, page.total, page.offset) = (items, 150, offset);
        let page: AnyData = Arc::new(page);
        let _ = app.update(Message::Loaded("albums".into(), Ok(page)));
    }
    let p = app.data::<Pager<Album>>("albums").expect("merged page");
    assert!(p.items.len() == 150 && p.offset == 0 && p.items[120].id == "120");
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
    let _ = app.update(Message::Shuffle);
    assert!(cur(&app) == playing && app.shuffle.is_none());
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
}

#[tokio::test]
async fn playlist_search_and_sort_keep_the_server_order() {
    let mut app = app();
    let key = "playlist:p1".to_string();
    let _ = app.update(Message::Nav(key.clone()));
    let songs: Vec<Song> = ["c", "a", "b"]
        .map(|t| json!({"id": t, "title": t}))
        .to_vec()
        .into_iter()
        .map(|v| parse(v).expect("song"))
        .collect();
    let pl: navify::api::Playlist = parse(json!({"id": "p1", "name": "P"})).expect("playlist");
    let _ = app.update(Message::Loaded(key.clone(), Ok(Arc::new((pl, songs)))));
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
    let left = app
        .data::<(navify::api::Playlist, Vec<Song>)>(&key)
        .expect("playlist")
        .1
        .iter();
    assert_eq!(left.map(|s| s.id.as_str()).collect::<String>(), "ca");
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
}
