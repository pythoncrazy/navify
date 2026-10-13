use std::sync::Arc;

use navify::api::{Album, Song, parse};
use navify::app::{AnyData, App, Message, Pager};
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
