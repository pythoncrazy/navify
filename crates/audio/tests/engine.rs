use navify_audio::{Track, render};
use std::sync::OnceLock;
use tokio::runtime::{Handle, Runtime};

const RATE: u32 = 48_000;

fn setup(titles: &str, format: Option<&str>) -> ((Handle, reqwest::Client), Vec<Track>) {
    static RT: OnceLock<Runtime> = OnceLock::new();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let [host, user, pass] = ["URL", "USERNAME", "PASSWORD"].map(|k| {
        let k = format!("NAVIFY_TEST_LOCAL_{k}");
        std::env::var(&k).unwrap_or_else(|_| panic!("{k} is not set"))
    });
    let hex: String = pass.bytes().map(|b| format!("{b:02x}")).collect();
    let auth = format!("u={user}&p=enc:{hex}&v=1.16.1&c=navify-test&f=json");
    let q = format!("{host}/rest/search3?{auth}&query=&songCount=100&albumCount=0&artistCount=0");
    let r = reqwest::blocking::get(q).expect("search");
    let v: serde_json::Value = r.json().expect("json");
    let songs = v["subsonic-response"]["searchResult3"]["song"].as_array();
    let songs = songs.expect("songs");
    let fmt = format.map(|f| format!("&format={f}")).unwrap_or_default();
    let track = |t| {
        let s = songs.iter().find(|s| s["title"] == t).expect("song");
        let id = s["id"].as_str().expect("id").to_string();
        let url = format!("{host}/rest/stream?{auth}&id={id}{fmt}");
        let duration = s["duration"].as_f64().expect("duration") as f32;
        let (gain, transcoded) = (1.0, format.is_some());
        Track {
            id,
            url,
            gain,
            transcoded,
            duration,
        }
    };
    let rt = RT.get_or_init(|| Runtime::new().expect("runtime"));
    let tracks = titles.split(' ').map(track).collect();
    ((rt.handle().clone(), reqwest::Client::new()), tracks)
}

#[test]
#[ignore = "needs the local test server"]
fn render_gives_exact_length_and_gain() {
    for (titles, format, seek, fade, want, tol) in [
        ("One Two", None, 0.0, 0.0, 5.0, 0.005),
        ("Three Four", None, 0.0, 0.0, 5.0, 0.03),
        ("One Two", None, 0.0, 1.0, 4.0, 0.02),
        ("One", None, 1.0, 0.0, 2.0, 0.005),
        ("Three", None, 1.0, 0.0, 2.0, 0.03),
        ("Four", None, 1.0, 0.0, 1.0, 0.03),
        ("One", None, 9.0, 0.0, 0.0, 0.03),
        ("Three", None, 9.0, 0.0, 0.0, 0.03),
        ("Four", None, 9.0, 0.0, 0.0, 0.03),
        ("One", Some("mp3"), 1.0, 0.0, 2.0, 0.08),
    ] {
        let (io, t) = setup(titles, format);
        let case = format!("{titles} {format:?} seek {seek} fade {fade}");
        let pcm = render(io, t, RATE, seek, fade).unwrap_or_else(|e| panic!("{case}: {e}"));
        let got = pcm.len() as f32 / 2.0 / RATE as f32;
        assert!(
            (got - want).abs() <= tol,
            "{case}: {got:.4} s, not {want:.4} s"
        );
    }
    let (io, t) = setup("One", None);
    let peak = |gain| {
        let mut t = t.clone();
        t[0].gain = gain;
        let pcm = render(io.clone(), t, RATE, 0.0, 0.0).expect("render");
        pcm.iter().fold(0f32, |m, s| m.max(s.abs()))
    };
    let ratio = peak(0.25) / peak(1.0);
    assert!((ratio - 0.25).abs() <= 0.001, "gain ratio {ratio}");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs the local test server"]
fn player_sends_events_on_null_device() {
    use navify_audio::{Event, Player};
    use std::time::Duration;
    let alsa = std::env::var_os("ALSA_CONFIG_PATH");
    let alsa = alsa.filter(|p| std::path::Path::new(p).is_file());
    assert!(
        alsa.is_some(),
        "set ALSA_CONFIG_PATH to the full path of tests/null.conf"
    );
    let ((rt, http), t) = setup("One Two", None);
    let (p, mut rx) = Player::new(rt.clone(), http);
    p.play(t[0].clone(), 0.0);
    p.queue_next(Some(t[1].clone()));
    let mut got = Vec::new();
    while got.last() != Some(&Event::Ended) {
        let e =
            rt.block_on(async { tokio::time::timeout(Duration::from_secs(20), rx.recv()).await });
        let e = e.unwrap_or_else(|_| panic!("no event in 20 s after {got:?}"));
        got.push(e.expect("engine is running"));
    }
    assert_eq!(got, [Event::Started(t[1].id.clone()), Event::Ended]);
}
