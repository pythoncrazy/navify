use std::sync::Arc;

use navify::api::{Album, Server, Session, Song};
use navify::app::{App, Message};

#[test]
fn album_reopen_in_stale_time_sends_no_request() {
    let dir = std::env::temp_dir().join(format!("navify-cache-test-{}", std::process::id()));
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
    let server: Server = serde_json::from_value(serde_json::json!({
        "id": "t", "name": "t", "url": "http://127.0.0.1:9", "user": "u",
        "salt": "s", "token": "t", "jwt": "", "native": false
    }))
    .expect("server");
    let mut app = App::new();
    app.session = Some(Session::new(server));
    let key = "album:a1".to_string();
    let pend = format!("{key}#0");
    let _ = app.update(Message::Nav(key.clone()));
    assert!(app.pending.contains(&pend));
    let data = Arc::new((Album { id: "a1".into(), ..Album::default() }, Vec::<Song>::new()));
    let _ = app.update(Message::Loaded(key.clone(), Ok(data)));
    assert!(!app.pending.contains(&pend));
    let _ = app.update(Message::Nav("home".into()));
    let _ = app.update(Message::Nav(key.clone()));
    assert_eq!(app.route, key);
    assert!(!app.pending.contains(&pend));
    let _ = std::fs::remove_dir_all(dir);
}
