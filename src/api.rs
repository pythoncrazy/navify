use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use anyhow::{Result, anyhow, ensure};
use bytes::Bytes;
use md5::Digest;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::fs;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub url: String,
    pub user: String,
    pub salt: String,
    pub token: String,
    pub jwt: String,
    pub native: bool,
    pub ext: Vec<String>,
}

#[derive(Clone)]
pub struct Session {
    pub server: Server,
    jwt: Arc<Mutex<String>>,
    http: reqwest::Client,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Rg {
    #[serde(alias = "rgTrackGain")]
    pub track_gain: Option<f32>,
    #[serde(alias = "rgAlbumGain")]
    pub album_gain: Option<f32>,
    #[serde(alias = "rgTrackPeak")]
    pub track_peak: Option<f32>,
    #[serde(alias = "rgAlbumPeak")]
    pub album_peak: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Genre {
    #[serde(alias = "value")]
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "round")]
    pub cover: String,
    pub album_count: u32,
    pub song_count: u32,
    pub play_count: u32,
    pub starred: bool,
    #[serde(alias = "mbzArtistId")]
    pub music_brainz_id: String,
}

fn round<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    Ok(if s.is_empty() { s } else { format!("o:{s}") })
}

impl Artist {
    pub fn banner_key(&self) -> String {
        match self.music_brainz_id.as_str() {
            "" => format!("banner:{}", self.name),
            m => format!("banner:mb:{m}"),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub name: String,
    #[serde(alias = "albumArtist")]
    pub artist: String,
    #[serde(alias = "albumArtistId")]
    pub artist_id: String,
    #[serde(alias = "maxYear")]
    pub year: u32,
    pub song_count: u32,
    pub duration: f32,
    pub play_count: u32,
    pub starred: bool,
    pub cover: String,
    #[serde(alias = "isCompilation")]
    pub compilation: bool,
    pub release_types: Vec<String>,
    pub release_date: String,
    pub genre: String,
    pub labels: Vec<String>,
    pub artists: Vec<Artist>,
    pub album_artists: Vec<Artist>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    pub album: String,
    pub album_id: String,
    pub artist: String,
    pub artist_id: String,
    pub genre: String,
    pub year: u32,
    #[serde(alias = "trackNumber")]
    pub track: u32,
    #[serde(rename = "discNumber")]
    pub disc: u32,
    pub duration: f32,
    pub play_count: u32,
    #[serde(alias = "rating")]
    pub user_rating: u8,
    pub starred: bool,
    pub starred_at: String,
    #[serde(alias = "createdAt")]
    pub created: String,
    pub cover: String,
    pub is_dir: bool,
    pub artists: Vec<Artist>,
    #[serde(alias = "streamUrl")]
    pub url: String,
    #[serde(flatten)]
    pub rg: Rg,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub comment: String,
    #[serde(alias = "ownerName")]
    pub owner: String,
    pub public: bool,
    pub song_count: u32,
    pub duration: f32,
    pub cover: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Info {
    pub biography: String,
    #[serde(rename = "similarArtist")]
    pub similar: Vec<Artist>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Lyrics {
    pub synced: bool,
    pub offset: i64,
    #[serde(rename = "line")]
    pub lines: Vec<(Option<u64>, String)>,
}

pub type Trio = (Vec<Artist>, Vec<Album>, Vec<Song>);
type Paged<T> = Result<(Vec<T>, usize)>;
type Q<'a> = [(&'a str, &'a str)];

pub trait Data: DeserializeOwned + Default {}
impl<T: DeserializeOwned + Default> Data for T {}

const INVALID: &str = "The server returned an invalid response.";

const SORTS: [[&str; 4]; 11] = [
    ["recent", "recent", "play_date", "play_date"],
    ["newest", "newest", "recently_added", "created_at"],
    ["frequent", "frequent", "play_count", "play_count"],
    ["random", "random", "random", "random"],
    ["name", "alphabeticalByName", "name", "title"],
    ["artist", "alphabeticalByArtist", "album_artist", "artist"],
    ["album", "alphabeticalByName", "name", "album"],
    ["starred", "starred", "starred_at", "starred_at"],
    ["year", "byYear", "max_year", "year"],
    ["genre", "byGenre", "genre", "genre"],
    ["rating", "highest", "rating", "rating"],
];

fn normalize(v: &mut Value) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(normalize),
        Value::Object(m) => {
            m.retain(|_, v| !v.is_null());
            if let Some(Value::Object(mut p)) = m.remove("participants") {
                for (role, key) in [("artist", "artists"), ("albumartist", "albumArtists")] {
                    m.extend(p.remove(role).map(|l| (key.to_string(), l)));
                }
            }
            if !m.contains_key("artists")
                && let (Some(id), Some(name)) = (m.get("artistId"), m.get("artist"))
                && id.is_string()
                && name.is_string()
            {
                let one = json!([{"id": id, "name": name}]);
                m.insert("artists".into(), one);
            }
            if let Some(Value::Object(rg)) = m.remove("replayGain") {
                m.extend(rg);
            }
            let tags = m.get("tags").and_then(|t| t.get("releasetype")).cloned();
            let mbz = m.get("mbzAlbumType").and_then(Value::as_str).map(|t| {
                let types = t.split(['/', ';', ',']);
                types.map(|x| Value::from(x.trim())).collect()
            });
            if let Some(t) = tags.or(mbz).filter(|_| !m.contains_key("releaseTypes")) {
                m.insert("releaseTypes".into(), t);
            }
            let tags = m.get("tags").and_then(|t| t.get("recordlabel")).cloned();
            let named = m.remove("recordLabels").and_then(|v| match v {
                Value::Array(a) => Some(a.iter().filter_map(|l| l.get("name").cloned()).collect()),
                _ => None,
            });
            m.extend(tags.or(named).map(|l| ("labels".to_string(), l)));
            if let Some(Value::Object(d)) = m.get("releaseDate") {
                let n = |k| d.get(k).and_then(Value::as_u64).unwrap_or_default();
                let date = format!("{:04}-{:02}-{:02}", n("year"), n("month"), n("day"));
                m.insert("releaseDate".into(), date.into());
            }
            if let Some(s) = m.get_mut("starred").filter(|s| !s.is_boolean()) {
                let at = std::mem::replace(s, s.as_str().is_some_and(|s| !s.is_empty()).into());
                m.entry("starredAt").or_insert(at);
            }
            if let Some(Value::Array(lines)) = m.get_mut("line") {
                for l in lines.iter_mut().filter(|l| l.is_object()) {
                    *l = json!([l["start"], l["value"].as_str().unwrap_or_default()]);
                }
            }
            if m.contains_key("streamUrl")
                && let Some(n) = m.remove("name")
            {
                m.extend([("title".into(), n), ("cover".into(), "".into())]);
            }
            let has = m.get("hasCoverArt") != Some(&false.into());
            let id = m.get(["albumId", "id"][usize::from(has)]).cloned();
            let (id, hash) = (id.filter(Value::is_string), m.remove("imageHash"));
            let id = id.filter(|_| !m.contains_key("cover"));
            if let Some(c) = m.remove("coverArt").filter(|c| c != "").or(id) {
                let nd = |c: &str| {
                    let (k, rest) = c.split_once('-')?;
                    if k == "dc" {
                        return rest.split(':').next().map(Value::from);
                    }
                    let (id, h) = rest.rsplit_once('_')?;
                    matches!(k, "al" | "ar" | "mf" | "pl").then(|| format!("{id}#{h}").into())
                };
                let c = match (c.as_str(), hash.as_ref().and_then(Value::as_str)) {
                    (Some(c), Some(h)) => format!("{c}#{h}").into(),
                    (Some(c), None) => nd(c).unwrap_or_else(|| c.into()),
                    _ => c,
                };
                m.insert("cover".into(), c);
            }
            m.values_mut().for_each(normalize);
        }
        _ => {}
    }
}

pub fn parse<T: Data>(mut v: Value) -> Result<T> {
    normalize(&mut v);
    let e = |e| anyhow!("The server returned an unexpected response: {e}");
    Ok(serde_json::from_value::<Option<T>>(v)
        .map_err(e)?
        .unwrap_or_default())
}

fn net_err(e: reqwest::Error) -> anyhow::Error {
    anyhow!(match () {
        _ if e.is_timeout() => "The request timed out.",
        _ if e.is_connect() => "Could not connect to the server.",
        _ if e.is_redirect() => "The server redirected the request.",
        _ => "The request failed.",
    })
}

fn pairs(q: &Value) -> Vec<(&str, String)> {
    let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    let all = q.as_object().into_iter().flatten();
    all.flat_map(|(k, v)| {
        let vs = v.as_array().map_or(std::slice::from_ref(v), Vec::as_slice);
        let vs = vs.iter().filter(|v| !v.is_null());
        vs.map(move |v| (k.as_str(), text(v)))
    })
    .collect()
}

fn sort_row(sort: &str) -> &'static [&'static str; 4] {
    SORTS.iter().find(|r| r[0] == sort).expect("a known sort")
}

fn sorted(sort: &str, col: usize, off: usize, n: usize) -> Value {
    let asc = matches!(sort, "name" | "artist" | "album" | "genre" | "year");
    let order = if asc { "ASC" } else { "DESC" };
    let mut q =
        json!({"_sort": sort_row(sort)[col], "_order": order, "_start": off, "_end": off + n});
    let only = match (sort, col) {
        ("starred", _) => "starred",
        ("rating", _) => "has_rating",
        ("recent" | "frequent", 2) => "recently_played",
        _ => return q,
    };
    q[only] = true.into();
    q
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({})", self.server.id)
    }
}

impl Session {
    pub fn client() -> reqwest::Client {
        static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
        HTTP.get_or_init(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let (ua, t) = (
                concat!("navify/", env!("CARGO_PKG_VERSION")),
                Duration::from_secs,
            );
            let same = reqwest::redirect::Policy::custom(|a| {
                let first = &a.previous()[0];
                let native = ["/api/", "/auth/"].iter().any(|p| first.path().contains(p));
                match native && first.origin() != a.url().origin() {
                    true => a.stop(),
                    false => a.follow(),
                }
            });
            let b = reqwest::Client::builder().user_agent(ua).redirect(same);
            let b = b.connect_timeout(t(10)).read_timeout(t(30));
            b.build().expect("HTTP client")
        })
        .clone()
    }

    pub fn new(server: Server) -> Self {
        let (jwt, http) = (Arc::new(Mutex::new(server.jwt.clone())), Self::client());
        Self { server, jwt, http }
    }

    pub fn same(&self, o: &Session) -> bool {
        Arc::ptr_eq(&self.jwt, &o.jwt)
    }

    pub fn renew(&mut self, new: Server) {
        *self.jwt.lock().expect("JWT lock") = new.jwt.clone();
        let old = &mut self.server;
        (old.salt, old.token, old.jwt) = (new.salt, new.token, new.jwt);
    }

    pub fn jwt(&self) -> String {
        self.jwt.lock().expect("JWT lock").clone()
    }

    pub async fn login(url: &str, user: &str, pass: &str) -> Result<Session> {
        let url = url.trim().split(['#', '?']).next().unwrap_or_default();
        let url = url.trim_end_matches('/');
        let url = url.strip_suffix("/app").unwrap_or(url);
        let mut url = format!("{}{url}", if url.contains("://") { "" } else { "https://" });
        let mut parsed = reqwest::Url::parse(&url).ok();
        if let Some(u) = parsed
            .as_mut()
            .filter(|u| u.has_authority() && u.username() != "")
        {
            let _ = (u.set_username(""), u.set_password(None));
            url = u.as_str().trim_end_matches('/').into();
        }
        let host = parsed.as_ref().and_then(reqwest::Url::host_str);
        let name = host.ok_or_else(|| anyhow!("The server address is not valid."))?;
        let [a, b] = [1, 2].map(|n| RandomState::new().hash_one(n));
        let salt = format!("{a:016x}{b:016x}");
        let digest = md5::Md5::digest(format!("{pass}{salt}"));
        let id = BuildHasherDefault::<DefaultHasher>::default().hash_one((&url, user));
        let mut s = Session::new(Server {
            id: format!("{id:016x}"),
            name: name.to_string(),
            user: user.to_string(),
            token: format!("{digest:x}"),
            url,
            salt,
            ..Server::default()
        });
        let ping: Value = s.get("ping", (), "").await?;
        let ext = async {
            if ping["openSubsonic"] != true {
                return Ok(Vec::new());
            }
            let ptr = "/openSubsonicExtensions";
            s.get::<Vec<Value>>("getOpenSubsonicExtensions", (), ptr)
                .await
        };
        let native = async {
            if ping["type"] != "navidrome" {
                return Ok(Value::Null);
            }
            let body = json!({"username": user, "password": pass});
            let req = s.http.post(format!("{}/auth/login", s.server.url));
            let res = req.json(&body).send().await.map_err(net_err)?;
            let (ok, status) = (res.status().is_success(), res.status().as_u16());
            if !ok && status != 401 {
                return Ok(Value::Null);
            }
            let body: Value = res.json().await.unwrap_or_default();
            let refused = "The server refused the login. Check the username and password.";
            ensure!(ok, "{}", body["message"].as_str().unwrap_or(refused));
            Ok(body)
        };
        let (ext, body) = tokio::try_join!(ext, native)?;
        let names = ext.iter().filter_map(|e| e["name"].as_str());
        s.server.ext = names.map(str::to_string).collect();
        if body.is_null() {
            return Ok(s);
        }
        let get = |k: &str| match body[k].as_str() {
            Some(t) if !t.is_empty() => Ok(t.to_string()),
            _ => Err(anyhow!("The server login response has no {k}.")),
        };
        (s.server.salt, s.server.token) = (get("subsonicSalt")?, get("subsonicToken")?);
        (s.server.jwt, s.server.native) = (get("token")?, true);
        Ok(Session::new(s.server))
    }

    pub fn url(&self, method: &str, q: &Value) -> String {
        let (s, url) = (&self.server, format!("{}/rest/{method}", self.server.url));
        let auth = json!({"u": s.user, "t": s.token, "s": s.salt,
            "v": "1.16.1", "c": "navify", "f": "json"});
        let all = pairs(&auth).into_iter().chain(pairs(q));
        let url = reqwest::Url::parse_with_params(&url, all);
        url.expect("the server URL is valid").to_string()
    }

    pub async fn rest<T: Data>(&self, method: &str, q: &Q<'_>, ptr: &str) -> Result<T> {
        let q = Value::from_iter(q.iter().copied());
        self.get(method, q, ptr).await
    }

    async fn get<T: Data>(&self, method: &str, q: impl Into<Value>, ptr: &str) -> Result<T> {
        let url = self.url(method, &q.into());
        let res = self.http.get(url).send().await.map_err(net_err)?;
        let status = res.status().as_u16();
        let http = move || anyhow!("The server returned HTTP {status}.");
        let bad = move |_| {
            if status < 300 {
                anyhow!(INVALID)
            } else {
                http()
            }
        };
        let mut v: Value = res.json().await.map_err(bad)?;
        let mut v = take(&mut v, "subsonic-response");
        ensure!(v.is_object(), http());
        let e = take(&mut v, "error");
        let msg = match e["code"].as_u64().unwrap_or_default() {
            40 => "Wrong username or password.",
            41 => "The server does not allow token authentication.",
            50 => "You do not have permission for this action.",
            _ => e["message"].as_str().unwrap_or("Unknown error."),
        };
        let failed = format!("The server reported error {}: {msg}", e["code"]);
        ensure!(v["status"] != "failed", failed);
        parse(v.pointer_mut(ptr).map(Value::take).unwrap_or_default())
    }

    async fn nd<T: Data>(&self, path: &str, q: impl Into<Value>) -> Result<(T, usize)> {
        let (url, mut q) = (format!("{}/api/{path}", self.server.url), q.into());
        if matches!(path, "album" | "song" | "artist") {
            q["missing"] = false.into();
        }
        let req = self
            .http
            .get(reqwest::Url::parse_with_params(&url, pairs(&q))?);
        let req = req.header("x-nd-authorization", format!("Bearer {}", self.jwt()));
        let res = req.send().await.map_err(net_err)?;
        let header = |k| res.headers().get(k).and_then(|v| v.to_str().ok());
        if let Some(t) = header("x-nd-authorization").filter(|t| !t.is_empty()) {
            *self.jwt.lock().expect("JWT lock") = t.to_string();
        }
        let total = header("x-total-count").and_then(|v| v.parse().ok());
        let status = res.status().as_u16();
        if status == 401 {
            let msg = "The server session expired. Log in again.";
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg).into());
        }
        let http = format!("The server returned HTTP {status}.");
        ensure!(res.status().is_success(), http);
        let v = res.json().await.map_err(|_| anyhow!(INVALID))?;
        Ok((parse(v)?, total.unwrap_or_default()))
    }

    async fn act(&self, method: &str, q: impl Into<Value>) -> Result<()> {
        self.get::<Value>(method, q, "").await.map(drop)
    }

    async fn detail<T: Data, U: Data>(
        &self,
        kind: &str,
        id: &str,
        key: &str,
    ) -> Result<(T, Vec<U>)> {
        let method = format!("get{}{}", kind[..1].to_uppercase(), &kind[1..]);
        let ptr = format!("/{kind}");
        let mut v: Value = self.get(&method, json!({"id": id}), &ptr).await?;
        let list = parse(take(&mut v, key))?;
        Ok((parse(v)?, list))
    }

    async fn index(&self, method: &str, ptr: &str) -> Result<Vec<Artist>> {
        let v: Vec<Value> = self.get(method, (), ptr).await?;
        let lists = v.into_iter().map(|mut i| parse(take(&mut i, "artist")));
        Ok(lists.collect::<Result<Vec<Vec<_>>>>()?.concat())
    }

    async fn trio(&self, method: &str, q: Value, ptr: &str) -> Result<Trio> {
        let mut v: Value = self.get(method, q, ptr).await?;
        let (ar, al, so) = (
            take(&mut v, "artist"),
            take(&mut v, "album"),
            take(&mut v, "song"),
        );
        Ok((parse(ar)?, parse(al)?, parse(so)?))
    }

    pub fn stream_url(&self, song: &Song, format: Option<&str>, br: u32) -> (String, bool) {
        if !song.url.is_empty() {
            return (song.url.clone(), true);
        }
        let q = json!({"id": song.id, "format": format, "maxBitRate": (br > 0).then_some(br)});
        let transcoded = br > 0 || format.is_some_and(|f| f != "raw");
        (self.url("stream", &q), transcoded)
    }

    pub fn cover_file(&self, id: &str, size: u32) -> PathBuf {
        let name = format!("{}-{size}", id.replace([':', '/', '\\', '#'], "_"));
        cache_dir("covers").join(&self.server.id).join(name)
    }

    pub async fn cover(&self, id: &str, size: u32) -> Result<Bytes> {
        let id = match id.find("https://") {
            Some(i) => &id[i..],
            None => {
                let wide = id.strip_prefix('b').and_then(|r| r.split_once(':'));
                let wide = wide.filter(|(n, _)| n.parse::<f32>().is_ok());
                ["w:", "o:"].iter().fold(wide.map_or(id, |w| w.1), |id, p| {
                    id.strip_prefix(p).unwrap_or(id)
                })
            }
        };
        let file = self.cover_file(id, size);
        if let Ok(b) = fs::read(&file).await
            && !b.is_empty()
        {
            return Ok(b.into());
        }
        static NET: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
        let _net = NET.acquire().await.expect("the cover semaphore is open");
        let url = match id.starts_with("https://") {
            true => id.to_string(),
            false => {
                let id = id.split('#').next();
                self.url("getCoverArt", &json!({"id": id, "size": size}))
            }
        };
        let res = self.http.get(url).send().await.map_err(net_err)?;
        let status = res.status().as_u16();
        let http = format!("The server returned HTTP {status} for cover art.");
        ensure!(status < 300, http);
        let kind = res.headers().get(reqwest::header::CONTENT_TYPE);
        let kind = kind.and_then(|k| k.to_str().ok()).unwrap_or_default();
        let text = ["json", "xml", "text"].iter().any(|t| kind.contains(t));
        ensure!(!text, "The server did not return a cover image.");
        let b = res.bytes().await.map_err(net_err)?;
        ensure!(!b.is_empty(), "The server returned an empty cover image.");
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let tmp = file.with_extension(format!("{}.{n}.tmp", std::process::id()));
        let dir = file.parent().expect("the cover file has a directory");
        if fs::create_dir_all(dir).await.is_ok() && fs::write(&tmp, &b).await.is_ok() {
            let _ = fs::rename(&tmp, &file).await;
        }
        Ok(b)
    }

    pub async fn albums(&self, sort: &str, filter: &Q<'_>, off: usize, n: usize) -> Paged<Album> {
        if self.server.native {
            let mut q = sorted(sort, 2, off, n);
            filter.iter().for_each(|&(k, v)| q[k] = v.into());
            return self.nd("album", q).await;
        }
        let genre = filter.iter().any(|f| f.0 == "genre");
        let t = if genre { "byGenre" } else { sort_row(sort)[1] };
        let y = t == "byYear";
        let mut q = json!({"type": t, "size": n, "offset": off,
            "fromYear": y.then_some(0), "toYear": y.then_some(9999)});
        filter.iter().for_each(|&(k, v)| q[k] = v.into());
        let list: Vec<Album> = self.get("getAlbumList2", q, "/albumList2/album").await?;
        let total = off + list.len() + usize::from(list.len() == n);
        Ok((list, total))
    }

    pub async fn album(&self, id: &str) -> Result<(Album, Vec<Song>)> {
        let (album, mut songs) = self.detail("album", id, "song").await?;
        songs.sort_by_key(|s: &Song| (s.disc, s.track));
        Ok((album, songs))
    }

    pub async fn artists(&self, off: usize, n: usize) -> Paged<Artist> {
        if self.server.native {
            let mut q = sorted("name", 2, off, n);
            q["role"] = "albumartist".into();
            return self.nd("artist", q).await;
        }
        let all = self.index("getArtists", "/artists/index").await?;
        Ok((all.iter().skip(off).take(n).cloned().collect(), all.len()))
    }

    pub async fn artist(&self, id: &str) -> Result<(Artist, Vec<Album>)> {
        let (mut a, albums): (Artist, Vec<Album>) = self.detail("artist", id, "album").await?;
        a.song_count = albums.iter().map(|x| x.song_count).sum();
        Ok((a, albums))
    }

    pub async fn appears(&self, id: &str) -> Result<Vec<Album>> {
        if !self.server.native {
            return Ok(Vec::new());
        }
        let q = json!({"role_artist_id": id, "_sort": "max_year", "_order": "DESC"});
        let (v, _): (Vec<Album>, _) = self.nd("album", q).await?;
        let own = |a: &Album| a.artist_id == id || a.album_artists.iter().any(|x| x.id == id);
        Ok(v.into_iter().filter(|a| !own(a)).collect())
    }

    pub async fn top_songs(&self, id: &str) -> Result<Vec<Song>> {
        if self.server.native {
            let mut q = sorted("frequent", 3, 0, 10);
            q["artists_id"] = id.into();
            return Ok(self.nd("song", q).await?.0);
        }
        let q = match self.server.ext.iter().any(|e| e == "topSongsByArtistId") {
            true => json!({"id": id, "count": 20}),
            false => json!({"artist": self.artist(id).await?.0.name, "count": 20}),
        };
        self.get("getTopSongs", q, "/topSongs/song").await
    }

    pub async fn artist_info(&self, id: &str) -> Result<Info> {
        let q = json!({"id": id, "count": 20});
        let mut i: Info = self.get("getArtistInfo2", q, "/artistInfo2").await?;
        i.similar.retain(|a| a.album_count > 0);
        Ok(i)
    }

    pub async fn banner(&self, q: &str) -> Result<String> {
        let base = "https://www.theaudiodb.com/api/v1/json/123";
        let (path, k, v) = match q.strip_prefix("mb:") {
            Some(m) => ("artist-mb", "i", m),
            None => ("search", "s", q),
        };
        let url = reqwest::Url::parse_with_params(&format!("{base}/{path}.php"), [(k, v)])?;
        let err = |e| anyhow!("TheAudioDB: {}", net_err(e));
        let res = self.http.get(url).send().await.map_err(err)?;
        let status = res.status().as_u16();
        ensure!(status < 300, "TheAudioDB returned HTTP {status}.");
        let v: Value = serde_json::from_slice(&res.bytes().await.map_err(err)?)?;
        let low = Some(q.to_lowercase());
        let same = |a: &&Value| a["strArtist"].as_str().map(str::to_lowercase) == low;
        let mut hit = v["artists"].as_array().into_iter().flatten();
        let hit = hit.find(|a| q.starts_with("mb:") || same(a));
        Ok(hit
            .and_then(|a| a["strArtistFanart"].as_str())
            .unwrap_or("")
            .into())
    }

    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        if self.server.native {
            let q = json!({"_sort": "name", "_order": "ASC"});
            return Ok(self.nd("playlist", q).await?.0);
        }
        self.get("getPlaylists", (), "/playlists/playlist").await
    }

    pub async fn playlist(&self, id: &str) -> Result<(Playlist, Vec<Song>)> {
        self.detail("playlist", id, "entry").await
    }

    pub async fn songs(&self, sort: &str, off: usize, n: usize) -> Paged<Song> {
        if self.server.native {
            let (mut list, total): (Vec<Song>, _) =
                self.nd("song", sorted(sort, 3, off, n)).await?;
            let played = matches!(sort, "recent" | "frequent");
            return Ok(
                match list.iter().position(|s| played && s.play_count == 0) {
                    Some(i) => (list.drain(..i).collect(), off + i),
                    None => (list, total),
                },
            );
        }
        if sort == "starred" {
            let all = self.starred().await?.2;
            return Ok((all.iter().skip(off).take(n).cloned().collect(), all.len()));
        }
        let q = json!({"query": "", "songCount": n, "songOffset": off,
            "artistCount": 0, "albumCount": 0});
        let list: Vec<Song> = self.get("search3", q, "/searchResult3/song").await?;
        let total = off + list.len() + usize::from(list.len() == n);
        Ok((list, total))
    }

    pub async fn top(&self) -> Result<(Vec<Artist>, Vec<Song>)> {
        if !self.server.native {
            return Ok(Default::default());
        }
        let q = json!({"_sort": "play_count", "_order": "DESC", "_start": 0, "_end": 10});
        let (a, s) = tokio::try_join!(
            self.nd::<Vec<Artist>>("artist", q),
            self.songs("frequent", 0, 4)
        )?;
        Ok((a.0.into_iter().filter(|a| a.play_count > 0).collect(), s.0))
    }

    pub async fn search(&self, query: &str, n: usize) -> Result<Trio> {
        let q = json!({"query": query, "artistCount": n, "albumCount": n, "songCount": n});
        self.trio("search3", q, "/searchResult3").await
    }

    pub async fn starred(&self) -> Result<Trio> {
        self.trio("getStarred2", ().into(), "/starred2").await
    }

    pub async fn genres(&self) -> Result<Vec<Genre>> {
        self.get("getGenres", (), "/genres/genre").await
    }

    pub async fn lyrics(&self, id: &str) -> Result<Vec<Lyrics>> {
        let ptr = "/lyricsList/structuredLyrics";
        self.get("getLyricsBySongId", json!({"id": id}), ptr).await
    }

    pub async fn radios(&self) -> Result<Vec<Song>> {
        let ptr = "/internetRadioStations/internetRadioStation";
        self.get("getInternetRadioStations", (), ptr).await
    }

    pub async fn song(&self, id: &str) -> Result<Value> {
        self.get("getSong", json!({"id": id}), "/song").await
    }

    pub async fn indexes(&self) -> Result<Vec<Artist>> {
        self.index("getIndexes", "/indexes/index").await
    }

    pub async fn directory(&self, id: &str) -> Result<(String, Vec<Song>)> {
        let mut d: Value = self
            .get("getMusicDirectory", json!({"id": id}), "/directory")
            .await?;
        let name = d["name"].as_str().unwrap_or_default().to_string();
        Ok((name, parse(take(&mut d, "child"))?))
    }

    pub async fn similar_songs(&self, id: &str, n: usize) -> Result<Vec<Song>> {
        let q = json!({"id": id, "count": n});
        self.get("getSimilarSongs2", q, "/similarSongs2/song").await
    }

    pub async fn scrobble(&self, id: &str, on: bool) -> Result<()> {
        let q = json!({"id": id, "submission": on});
        self.act("scrobble", q).await
    }

    pub async fn star(&self, ids: &[String], on: bool) -> Result<()> {
        let method = if on { "star" } else { "unstar" };
        self.act(method, json!({"id": ids})).await
    }

    pub async fn rate(&self, id: &str, n: u8) -> Result<()> {
        self.act("setRating", json!({"id": id, "rating": n})).await
    }

    pub async fn playlist_create(&self, name: &str, ids: &[String]) -> Result<Playlist> {
        let q = json!({"name": name, "songId": ids});
        self.get("createPlaylist", q, "/playlist").await
    }

    pub async fn playlist_update(
        &self,
        id: &str,
        name: Option<&str>,
        comment: Option<&str>,
        public: Option<bool>,
        add: &[String],
        remove: &[usize],
    ) -> Result<()> {
        let q = json!({"playlistId": id, "name": name, "comment": comment, "public": public,
            "songIdToAdd": add, "songIndexToRemove": remove});
        self.act("updatePlaylist", q).await
    }

    pub async fn playlist_delete(&self, id: &str) -> Result<()> {
        self.act("deletePlaylist", json!({"id": id})).await
    }

    pub async fn play_queue(&self) -> Result<(Vec<Song>, String, u64)> {
        let mut v: Value = self.get("getPlayQueue", (), "/playQueue").await?;
        let current = v["current"].as_str().unwrap_or_default().to_string();
        let pos = v["position"].as_u64().unwrap_or_default();
        Ok((parse(take(&mut v, "entry"))?, current, pos))
    }

    pub async fn save_play_queue(&self, ids: &[String], current: &str, pos: u64) -> Result<()> {
        let q = json!({"id": ids, "current": current, "position": pos});
        self.act("savePlayQueue", q).await
    }
}

fn take(v: &mut Value, k: &str) -> Value {
    v.get_mut(k).map(Value::take).unwrap_or_default()
}

pub fn cache_dir(kind: &str) -> PathBuf {
    crate::config::dir("cache").join("navify").join(kind)
}

pub fn cache_files(dir: &Path) -> Vec<(PathBuf, SystemTime, u64)> {
    let (mut out, mut dirs) = (Vec::new(), vec![dir.to_path_buf()]);
    while let Some(d) = dirs.pop() {
        let entries = std::fs::read_dir(d).into_iter().flatten().flatten();
        for (p, m) in entries.filter_map(|e| Some((e.path(), e.metadata().ok()?))) {
            match m.is_dir() {
                true => dirs.push(p),
                false => out.push((p, m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len())),
            }
        }
    }
    out
}

pub fn prune(dir: &Path, cap: u64) {
    let mut files = cache_files(dir);
    files.sort_by_key(|f| f.1);
    let old = SystemTime::now() - Duration::from_secs(3600);
    let mut total: u64 = files.iter().map(|f| f.2).sum();
    for (p, time, size) in files {
        let tmp = p.extension() == Some("tmp".as_ref());
        if (if tmp { time < old } else { total > cap }) && std::fs::remove_file(&p).is_ok() {
            total -= size;
        }
    }
}
