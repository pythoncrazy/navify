use md5::{Digest, Md5};
use navify_audio::{Track, render};

const RATE: u32 = 48_000;

fn rt() -> &'static tokio::runtime::Handle {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().expect("runtime")).handle()
}

fn server() -> Option<(String, String)> {
    let host = std::env::var("NAVIFY_TEST_LOCAL_URL").ok()?;
    let user = std::env::var("NAVIFY_TEST_LOCAL_USERNAME").ok()?;
    let pass = std::env::var("NAVIFY_TEST_LOCAL_PASSWORD").ok()?;
    let salt = "navifytest";
    let token: String =
        Md5::digest(format!("{pass}{salt}")).iter().map(|b| format!("{b:02x}")).collect();
    Some((host, format!("u={user}&t={token}&s={salt}&v=1.16.1&c=navify-test&f=json")))
}

fn tracks(titles: &[&str], format: Option<&str>) -> Option<Vec<Track>> {
    let (host, auth) = server()?;
    let v: serde_json::Value = reqwest::blocking::get(format!(
        "{host}/rest/search3?{auth}&query=&songCount=100&albumCount=0&artistCount=0"
    ))
    .expect("search")
    .json()
    .expect("json");
    let songs = v["subsonic-response"]["searchResult3"]["song"].as_array().expect("songs").clone();
    Some(
        titles
            .iter()
            .map(|t| {
                let s = songs.iter().find(|s| s["title"] == *t).expect("song");
                let id = s["id"].as_str().expect("id");
                let fmt = format.map(|f| format!("&format={f}")).unwrap_or_default();
                Track {
                    id: id.into(),
                    url: format!("{host}/rest/stream?{auth}&id={id}{fmt}"),
                    gain: 1.0,
                    transcoded: format.is_some(),
                    duration: s["duration"].as_f64().unwrap_or(0.0) as f32,
                }
            })
            .collect(),
    )
}

macro_rules! local {
    ($titles:expr, $format:expr) => {
        match tracks($titles, $format) {
            Some(t) => t,
            None => {
                eprintln!("skip: no NAVIFY_TEST_LOCAL_* vars");
                return;
            }
        }
    };
}

fn secs(pcm: &[f32]) -> f32 {
    pcm.len() as f32 / 2.0 / RATE as f32
}

fn near(got: f32, want: f32, tol: f32) {
    assert!((got - want).abs() <= tol, "length {got:.4} s, expected {want:.4} s");
}

#[test]
fn gapless_across_sample_rates() {
    let t = local!(&["One", "Two"], None);
    near(secs(&render(rt(), t, RATE, 0.0, 0.0).expect("render")), 5.0, 0.005);
}

#[test]
fn seek_in_raw_stream() {
    let t = local!(&["One"], None);
    near(secs(&render(rt(), t, RATE, 1.0, 0.0).expect("render")), 2.0, 0.005);
}

#[test]
fn crossfade_overlaps_tracks() {
    let t = local!(&["One", "Two"], None);
    near(secs(&render(rt(), t, RATE, 0.0, 1.0).expect("render")), 4.0, 0.02);
}

#[test]
fn mp3_and_opus_trim_padding() {
    let t = local!(&["Three", "Four"], None);
    near(secs(&render(rt(), t, RATE, 0.0, 0.0).expect("render")), 5.0, 0.03);
}

#[test]
fn seek_in_transcoded_stream() {
    let t = local!(&["One"], Some("mp3"));
    near(secs(&render(rt(), t, RATE, 1.0, 0.0).expect("render")), 2.0, 0.08);
}

#[cfg(target_os = "linux")]
#[test]
fn player_sends_events_on_null_device() {
    use navify_audio::{Event, Player};
    use std::time::{Duration, Instant};
    let t = local!(&["One", "Two"], None);
    let conf = std::env::temp_dir().join("navify-null-asound.conf");
    std::fs::write(&conf, "</usr/share/alsa/alsa.conf>\npcm.!default { type null }\n")
        .expect("conf");
    unsafe { std::env::set_var("ALSA_CONFIG_PATH", &conf) };
    let (p, mut rx) = Player::new(rt().clone());
    p.play(t[0].clone(), 0.0);
    p.queue_next(Some(t[1].clone()));
    let start = Instant::now();
    let mut got = Vec::new();
    while got.last() != Some(&Event::Ended) && start.elapsed() < Duration::from_secs(20) {
        match rt().block_on(async { tokio::time::timeout(Duration::from_secs(1), rx.recv()).await })
        {
            Ok(Some(e)) => got.push(e),
            Ok(None) => break,
            Err(_) => {}
        }
    }
    assert_eq!(got, [Event::Started(t[1].id.clone()), Event::Ended]);
}

#[test]
fn gain_scales_samples() {
    let t = local!(&["One"], None);
    let peak = |g: f32| {
        let t = vec![Track { gain: g, ..t[0].clone() }];
        render(rt(), t, RATE, 0.0, 0.0).expect("render").iter().fold(0f32, |m, s| m.max(s.abs()))
    };
    near(peak(0.25) / peak(1.0), 0.25, 0.001);
}
