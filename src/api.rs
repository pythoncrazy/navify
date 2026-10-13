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
    pub cover: String,
    pub album_count: u32,
    pub song_count: u32,
    pub starred: bool,
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
    #[serde(alias = "trackNumber")]
    pub track: u32,
    #[serde(rename = "discNumber")]
    pub disc: u32,
    pub duration: f32,
    pub play_count: u32,
    #[serde(alias = "rating")]
    pub user_rating: u8,
    pub starred: bool,
    #[serde(alias = "createdAt")]
    pub created: String,
    pub cover: String,
    pub is_dir: bool,
    pub artists: Vec<Artist>,
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

type Trio = (Vec<Artist>, Vec<Album>, Vec<Song>);
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
            if let Some(Value::Object(d)) = m.get("releaseDate") {
                let n = |k| d.get(k).and_then(Value::as_u64).unwrap_or_default();
                let date = format!("{:04}-{:02}-{:02}", n("year"), n("month"), n("day"));
                m.insert("releaseDate".into(), date.into());
            }
            if let Some(s) = m.get_mut("starred").filter(|s| !s.is_boolean()) {
                *s = s.as_str().is_some_and(|s| !s.is_empty()).into();
            }
            if let Some(Value::Array(lines)) = m.get_mut("line") {
                for l in lines.iter_mut().filter(|l| l.is_object()) {
                    *l = json!([l["start"], l["value"].as_str().unwrap_or_default()]);
                }
            }
            if let Some(id) = m.remove("mediaFileId") {
                m.insert("id".into(), id);
            }
            let has = m.get("hasCoverArt") != Some(&false.into());
            let id = m.get("id").filter(|id| has && id.is_string()).cloned();
            if let Some(c) = m.remove("coverArt").filter(|c| c != "").or(id) {
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
    let field = sort_row(sort)[col];
    let starred = (sort == "starred").then_some(true);
    json!({"_sort": field, "_order": order, "_start": off, "_end": off + n, "starred": starred})
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
            let builder = reqwest::Client::builder();
            let agent = concat!("navify/", env!("CARGO_PKG_VERSION"));
            builder.user_agent(agent).build().expect("HTTP client")
        })
        .clone()
    }

    pub fn new(server: Server) -> Self {
        let (jwt, http) = (Arc::new(Mutex::new(server.jwt.clone())), Self::client());
        Self { server, jwt, http }
    }

    pub fn jwt(&self) -> String {
        self.jwt.lock().expect("JWT lock").clone()
    }

    pub async fn login(url: &str, user: &str, pass: &str) -> Result<Session> {
        let url = url.trim().trim_end_matches('/');
        let url = format!("{}{url}", if url.contains("://") { "" } else { "https://" });
        let parsed = reqwest::Url::parse(&url).ok();
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
            token: digest.iter().map(|b| format!("{b:02x}")).collect(),
            url,
            salt,
            ..Server::default()
        });
        let ping: Value = s.get("ping", (), "").await?;
        if ping["openSubsonic"] == true {
            let ptr = "/openSubsonicExtensions";
            let ext: Vec<Value> = s.get("getOpenSubsonicExtensions", (), ptr).await?;
            let names = ext.iter().filter_map(|e| e["name"].as_str());
            s.server.ext = names.map(str::to_string).collect();
        }
        if ping["type"] != "navidrome" {
            return Ok(s);
        }
        let body = json!({"username": user, "password": pass});
        let req = s.http.post(format!("{}/auth/login", s.server.url));
        let res = req.json(&body).send().await.map_err(net_err)?;
        let ok = res.status().is_success();
        let body: Value = res.json().await.unwrap_or_default();
        let refused = "The server refused the login. Check the username and password.";
        ensure!(ok, "{}", body["message"].as_str().unwrap_or(refused));
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
        let req = self.http.get(url).query(&auth).query(&pairs(q)).build();
        req.expect("the server URL is valid").url().to_string()
    }

    pub async fn rest<T: Data>(&self, method: &str, q: &Q<'_>, ptr: &str) -> Result<T> {
        self.get(method, Value::from_iter(q.iter().copied()), ptr)
            .await
    }

    async fn get<T: Data>(&self, method: &str, q: impl Into<Value>, ptr: &str) -> Result<T> {
        let url = self.url(method, &q.into());
        let res = self.http.get(url).send().await.map_err(net_err)?;
        let status = res.status().as_u16();
        let mut v: Value = res.json().await.map_err(|_| anyhow!(INVALID))?;
        let mut v = v["subsonic-response"].take();
        ensure!(!v.is_null(), "The server returned HTTP {status}.");
        let e = v["error"].take();
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
        let listed = usize::from(matches!(path, "album" | "song" | "artist"));
        let url = format!("{}/api/{path}", self.server.url);
        let req = self.http.get(url).query(&pairs(&q.into()));
        let req = req.query(&[("missing", false)][..listed]);
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
        list: &str,
        q: impl Into<Value>,
    ) -> Result<(T, Vec<U>)> {
        if self.server.native {
            let item = format!("{kind}/{id}");
            let (a, b) = tokio::try_join!(self.nd(&item, ()), self.nd(list, q))?;
            return Ok((a.0, b.0));
        }
        let method = format!("get{}{}", kind[..1].to_uppercase(), &kind[1..]);
        let ptr = format!("/{kind}");
        let mut v: Value = self.get(&method, json!({"id": id}), &ptr).await?;
        let list = parse(v[key].take())?;
        Ok((parse(v)?, list))
    }

    async fn index(&self, method: &str, ptr: &str) -> Result<Vec<Artist>> {
        let v: Vec<Value> = self.get(method, (), ptr).await?;
        let lists = v.into_iter().map(|mut i| parse(i["artist"].take()));
        Ok(lists.collect::<Result<Vec<Vec<_>>>>()?.concat())
    }

    async fn trio(&self, method: &str, q: Value, ptr: &str) -> Result<Trio> {
        let mut v: Value = self.get(method, q, ptr).await?;
        let (ar, al, so) = (v["artist"].take(), v["album"].take(), v["song"].take());
        Ok((parse(ar)?, parse(al)?, parse(so)?))
    }

    pub fn stream_url(&self, song: &Song, format: Option<&str>, br: u32) -> (String, bool) {
        let q = json!({"id": song.id, "format": format, "maxBitRate": (br > 0).then_some(br)});
        let transcoded = br > 0 || format.is_some_and(|f| f != "raw");
        (self.url("stream", &q), transcoded)
    }

    pub fn cover_file(&self, id: &str, size: u32) -> PathBuf {
        let name = format!("{}-{size}", id.replace(['/', '\\'], "_"));
        cache_dir("covers").join(&self.server.id).join(name)
    }

    pub async fn cover(&self, id: &str, size: u32) -> Result<Bytes> {
        let file = self.cover_file(id, size);
        if let Ok(b) = fs::read(&file).await {
            return Ok(b.into());
        }
        let url = self.url("getCoverArt", &json!({"id": id, "size": size}));
        let res = self.http.get(url).send().await.map_err(net_err)?;
        let status = res.status().as_u16();
        let http = format!("The server returned HTTP {status} for cover art.");
        ensure!(status < 300, http);
        let b = res.bytes().await.map_err(net_err)?;
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
        let q = json!({"album_id": id});
        let (album, mut songs) = self.detail("album", id, "song", "song", q).await?;
        songs.sort_by_key(|s: &Song| (s.disc, s.track));
        Ok((album, songs))
    }

    pub async fn artists(&self, off: usize, n: usize) -> Paged<Artist> {
        if self.server.native {
            return self.nd("artist", sorted("name", 2, off, n)).await;
        }
        let all = self.index("getArtists", "/artists/index").await?;
        Ok((all.iter().skip(off).take(n).cloned().collect(), all.len()))
    }

    pub async fn artist(&self, id: &str) -> Result<(Artist, Vec<Album>)> {
        let q = json!({"role_albumartist_id": id});
        self.detail("artist", id, "album", "album", q).await
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

    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        if self.server.native {
            let q = json!({"_sort": "name", "_order": "ASC"});
            return Ok(self.nd("playlist", q).await?.0);
        }
        self.get("getPlaylists", (), "/playlists/playlist").await
    }

    pub async fn playlist(&self, id: &str) -> Result<(Playlist, Vec<Song>)> {
        let tracks = format!("playlist/{id}/tracks");
        self.detail("playlist", id, "entry", &tracks, ()).await
    }

    pub async fn songs(&self, sort: &str, off: usize, n: usize) -> Paged<Song> {
        if self.server.native {
            return self.nd("song", sorted(sort, 3, off, n)).await;
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

    pub async fn indexes(&self) -> Result<Vec<Artist>> {
        self.index("getIndexes", "/indexes/index").await
    }

    pub async fn directory(&self, id: &str) -> Result<Vec<Song>> {
        let ptr = "/directory/child";
        self.get("getMusicDirectory", json!({"id": id}), ptr).await
    }

    pub async fn similar_songs(&self, id: &str, n: usize) -> Result<Vec<Song>> {
        let q = json!({"id": id, "count": n});
        self.get("getSimilarSongs2", q, "/similarSongs2/song").await
    }

    pub async fn scrobble(&self, id: &str, on: bool) -> Result<()> {
        self.act("scrobble", json!({"id": id, "submission": on}))
            .await
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
        add: &[String],
        remove: &[usize],
    ) -> Result<()> {
        let q = json!({"playlistId": id, "name": name, "comment": comment,
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
        Ok((parse(v["entry"].take())?, current, pos))
    }

    pub async fn save_play_queue(&self, ids: &[String], current: &str, pos: u64) -> Result<()> {
        let q = json!({"id": ids, "current": current, "position": pos});
        self.act("savePlayQueue", q).await
    }
}

pub fn cache_dir(kind: &str) -> PathBuf {
    let dir = dirs::cache_dir().expect("The system has no cache directory.");
    dir.join("navify").join(kind)
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
