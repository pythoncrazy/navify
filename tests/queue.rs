use navify::api::{Server, Session, Song};
use navify::app::{App, Message};

fn songs(ids: &[&str]) -> Vec<Song> {
    ids.iter().map(|id| Song { id: id.to_string(), ..Song::default() }).collect()
}

fn cur(app: &App) -> String {
    app.current_song().expect("a song plays").id.clone()
}

#[test]
fn shuffle_keeps_the_current_song() {
    let dir = std::env::temp_dir().join(format!("navify-queue-test-{}", std::process::id()));
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
    let server: Server = serde_json::from_value(serde_json::json!({
        "id": "t", "name": "t", "url": "http://127.0.0.1:9", "user": "u",
        "salt": "s", "token": "t", "jwt": "", "native": false
    }))
    .expect("server");
    let mut app = App::new();
    app.session = Some(Session::new(server));
    let ids = ["a", "b", "c", "d", "e", "f"];
    let _ = app.update(Message::Play(songs(&ids), 3, "t".into()));
    assert_eq!(cur(&app), "d");
    let _ = app.update(Message::Shuffle);
    assert_eq!(cur(&app), "d");
    let _ = app.update(Message::Play(songs(&ids), 2, "t".into()));
    assert_eq!(cur(&app), "c");
    let _ = app.update(Message::QEnd(songs(&["z"])));
    let order = app.shuffle.clone().expect("shuffle is on");
    assert_eq!(app.queue[*order.last().expect("order")].id, "z");
    let _ = app.update(Message::PlayQueue(2));
    let playing = cur(&app);
    let _ = app.update(Message::QDel(0));
    assert_eq!(cur(&app), playing);
    let _ = app.update(Message::Shuffle);
    assert_eq!(cur(&app), playing);
    assert!(app.shuffle.is_none());
    let _ = std::fs::remove_dir_all(dir);
}
