use std::{
    collections::HashMap,
    fmt,
    hash::{BuildHasher, DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use anyhow::{Result, bail};
use bytes::Bytes;
use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::fs;

#[derive(Debug)]
pub struct Unauthorized;

impl fmt::Display for Unauthorized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The server session expired. Log in again.")
    }
}

impl std::error::Error for Unauthorized {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub url: String,
    pub user: String,
    pub salt: String,
    pub token: String,
    pub jwt: String,
    pub native: bool,
}

#[derive(Clone)]
pub struct Session {
    pub server: Server,
    jwt: Arc<Mutex<String>>,
    http: reqwest::Client,
    covers: Arc<Mutex<lru::LruCache<(String, u32), Bytes>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Rg {
    #[serde(rename = "rgTrackGain", alias = "trackGain")]
    pub track_gain: Option<f32>,
    #[serde(rename = "rgAlbumGain", alias = "albumGain")]
    pub album_gain: Option<f32>,
    #[serde(rename = "rgTrackPeak", alias = "trackPeak")]
    pub track_peak: Option<f32>,
    #[serde(rename = "rgAlbumPeak", alias = "albumPeak")]
    pub album_peak: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Genre {
    pub id: String,
    #[serde(alias = "value")]
    pub name: String,
    pub song_count: u32,
    pub album_count: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(rename = "coverArt")]
    pub cover: String,
    pub album_count: u32,
    pub song_count: u32,
    #[serde(deserialize_with = "de_starred")]
    pub starred: bool,
    pub starred_at: String,
    #[serde(alias = "rating")]
    pub user_rating: u8,
    #[serde(deserialize_with = "de_genres")]
    pub genres: Vec<Genre>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    pub size: u64,
    #[serde(alias = "createdAt")]
    pub created: String,
    #[serde(alias = "updatedAt", alias = "changed")]
    pub updated: String,
    pub played: String,
    pub play_count: u32,
    #[serde(deserialize_with = "de_starred")]
    pub starred: bool,
    pub starred_at: String,
    #[serde(alias = "rating")]
    pub user_rating: u8,
    #[serde(deserialize_with = "de_genres")]
    pub genres: Vec<Genre>,
    #[serde(rename = "coverArt")]
    pub cover: String,
    #[serde(rename = "explicitStatus", deserialize_with = "de_explicit")]
    pub explicit: bool,
    #[serde(alias = "isCompilation")]
    pub compilation: bool,
    #[serde(rename = "releaseDate", deserialize_with = "de_date")]
    pub release_date: String,
    #[serde(rename = "originalReleaseDate", alias = "originalDate", deserialize_with = "de_date")]
    pub original_date: String,
    pub version: String,
    #[serde(rename = "artists")]
    artists: Vec<Artist>,
    participants: HashMap<String, Vec<Artist>>,
    #[serde(flatten)]
    rg: Rg,
    #[serde(rename = "replayGain")]
    rg2: Rg,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    pub album: String,
    pub album_id: String,
    pub artist: String,
    pub artist_id: String,
    #[serde(alias = "displayAlbumArtist")]
    pub album_artist: String,
    pub album_artist_id: String,
    #[serde(rename = "track", alias = "trackNumber")]
    pub track: u32,
    #[serde(rename = "discNumber")]
    pub disc: u32,
    pub year: u32,
    pub duration: f32,
    pub suffix: String,
    pub bit_rate: u32,
    pub size: u64,
    pub path: String,
    pub play_count: u32,
    #[serde(alias = "rating")]
    pub user_rating: u8,
    #[serde(deserialize_with = "de_starred")]
    pub starred: bool,
    pub starred_at: String,
    #[serde(alias = "playDate")]
    pub played: String,
    #[serde(alias = "createdAt")]
    pub created: String,
    #[serde(rename = "explicitStatus", deserialize_with = "de_explicit")]
    pub explicit: bool,
    #[serde(rename = "coverArt")]
    pub cover: String,
    #[serde(deserialize_with = "de_genres")]
    pub genres: Vec<Genre>,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub song_count: u32,
    #[serde(rename = "artists")]
    artists: Vec<Artist>,
    #[serde(rename = "albumArtists")]
    album_artists: Vec<Artist>,
    participants: HashMap<String, Vec<Artist>>,
    #[serde(flatten)]
    rg: Rg,
    #[serde(rename = "replayGain")]
    rg2: Rg,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    pub sync: bool,
    #[serde(alias = "createdAt")]
    pub created: String,
    #[serde(alias = "updatedAt", alias = "changed")]
    pub updated: String,
    #[serde(rename = "coverArt")]
    pub cover: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Info {
    pub biography: String,
    #[serde(rename = "similarArtist")]
    pub similar: Vec<Artist>,
    #[serde(rename = "mediumImageUrl")]
    pub image: String,
    #[serde(rename = "lastFmUrl")]
    pub last_fm_url: String,
    #[serde(rename = "musicBrainzId")]
    pub music_brainz_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Lyrics {
    pub lang: String,
    pub synced: bool,
    pub offset: i64,
    #[serde(rename = "displayArtist")]
    pub artist: String,
    #[serde(rename = "displayTitle")]
    pub title: String,
    #[serde(rename = "line", deserialize_with = "de_lines")]
    pub lines: Vec<(Option<u64>, String)>,
}

fn de_starred<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Bool(b) => b,
        Value::String(s) => !s.is_empty(),
        _ => false,
    })
}

fn de_explicit<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    Ok(matches!(Value::deserialize(d)?.as_str(), Some("e" | "explicit")))
}

fn de_date<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        Value::Object(m) => format!(
            "{:04}-{:02}-{:02}",
            m.get("year").and_then(Value::as_u64).unwrap_or_default(),
            m.get("month").and_then(Value::as_u64).unwrap_or_default(),
            m.get("day").and_then(Value::as_u64).unwrap_or_default()
        ),
        _ => String::new(),
    })
}

fn de_genres<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<Genre>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => a
            .into_iter()
            .filter_map(|v| match v {
                Value::String(name) => Some(Genre { name, ..Genre::default() }),
                v => serde_json::from_value(v).ok(),
            })
            .collect(),
        Value::String(s) => {
            s.split(',').map(|n| Genre { name: n.trim().to_string(), ..Genre::default() }).collect()
        }
        _ => Vec::new(),
    })
}

fn de_lines<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<(Option<u64>, String)>, D::Error> {
    Ok(Vec::<Value>::deserialize(d)
        .unwrap_or_default()
        .into_iter()
        .map(|l| (l["start"].as_u64(), l["value"].as_str().unwrap_or_default().to_string()))
        .collect())
}

fn vlist<T: DeserializeOwned + Default>(v: &Value, key: &str) -> Vec<T> {
    serde_json::from_value(v[key].clone()).unwrap_or_default()
}

fn net_err(e: &reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        if e.is_timeout() {
            "The request timed out."
        } else if e.is_connect() {
            "Could not connect to the server."
        } else if e.is_redirect() {
            "The server redirected the request."
        } else {
            "The request failed."
        }
    )
}

impl Session {
    fn client() -> reqwest::Client {
        static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
        HTTP.get_or_init(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
            reqwest::Client::builder()
                .user_agent(concat!("navify/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("HTTP client")
        })
        .clone()
    }

    pub fn new(server: Server) -> Self {
        Self {
            jwt: Arc::new(Mutex::new(server.jwt.clone())),
            server,
            http: Self::client(),
            covers: Arc::new(Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(256).expect("nonzero"),
            ))),
        }
    }

    fn lock(&self) -> MutexGuard<'_, String> {
        self.jwt.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn jwt(&self) -> String {
        self.lock().clone()
    }

    pub async fn login(url: &str, user: &str, pass: &str) -> Result<Session> {
        let url = format!("https://{}", url.trim_end_matches('/'))
            .replace("https://http://", "http://")
            .replace("https://https://", "https://");
        let salt = (0..2)
            .map(|_| {
                format!(
                    "{:016x}",
                    std::collections::hash_map::RandomState::new().build_hasher().finish()
                )
            })
            .collect::<String>();
        let mut s = Session::new(Server {
            id: format!("{:016x}", {
                let mut h = DefaultHasher::new();
                (url.clone(), user).hash(&mut h);
                h.finish()
            }),
            name: reqwest::Url::parse(&url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| url.clone()),
            url,
            user: user.to_string(),
            salt: salt.clone(),
            token: md5_hex(&format!("{pass}{salt}")),
            jwt: String::new(),
            native: false,
        });
        let ping: Value = s.rest("ping", &[], "").await?;
        if ping["type"].as_str() == Some("navidrome") {
            let res = s
                .http
                .post(format!("{}/auth/login", s.server.url))
                .json(&json!({"username": user, "password": pass}))
                .send()
                .await
                .map_err(|e| net_err(&e))?;
            let status = res.status();
            let body: Value = res.json().await.unwrap_or_default();
            if !status.is_success() {
                bail!(
                    "{}",
                    body["message"].as_str().unwrap_or(
                        "The server refused the login. Check the username and password."
                    )
                );
            }
            s.server.salt = body["subsonicSalt"].as_str().unwrap_or_default().to_string();
            s.server.token = body["subsonicToken"].as_str().unwrap_or_default().to_string();
            s.server.jwt = body["token"].as_str().unwrap_or_default().to_string();
            *s.lock() = s.server.jwt.clone();
            s.server.native = true;
        }
        Ok(s)
    }

    fn auth(&self) -> [(&'static str, &str); 6] {
        [
            ("u", &self.server.user),
            ("t", &self.server.token),
            ("s", &self.server.salt),
            ("v", "1.16.1"),
            ("c", "navify"),
            ("f", "json"),
        ]
    }

    fn query<'a>(&'a self, params: &'a [(&'a str, String)]) -> Vec<(&'a str, &'a str)> {
        self.auth().into_iter().chain(params.iter().map(|(k, v)| (*k, v.as_str()))).collect()
    }

    pub async fn rest<T: DeserializeOwned + Default>(
        &self,
        method: &str,
        params: &[(&str, String)],
        pointer: &str,
    ) -> Result<T> {
        let res = self
            .http
            .get(format!("{}/rest/{method}", self.server.url))
            .query(&self.query(params))
            .send()
            .await
            .map_err(|e| net_err(&e))?;
        let status = res.status();
        let v: Value = res
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("The server returned an invalid response."))?;
        let sub = &v["subsonic-response"];
        if sub.is_null() {
            bail!("The server returned HTTP {}.", status.as_u16());
        }
        if sub["status"].as_str() == Some("failed") {
            let e = &sub["error"];
            bail!(
                "{}",
                match e["code"].as_u64().unwrap_or_default() {
                    40 => "Wrong username or password.".to_string(),
                    41 => "The server does not allow token authentication.".to_string(),
                    50 => "You do not have permission for this action.".to_string(),
                    code => format!(
                        "The server reported error {code}: {}",
                        e["message"].as_str().unwrap_or("unknown error")
                    ),
                }
            );
        }
        let data = pointer.split('/').filter(|k| !k.is_empty()).fold(sub, |v, k| &v[k]);
        Ok(serde_json::from_value(data.clone()).unwrap_or_default())
    }

    pub async fn nd<T: DeserializeOwned + Default>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<(T, usize)> {
        let req = self
            .http
            .request(method.clone(), format!("{}/api/{path}", self.server.url))
            .header("x-nd-authorization", format!("Bearer {}", self.jwt()));
        let req = if method == Method::GET {
            req.query(&query.iter().map(|(k, v)| (*k, v.as_str())).collect::<Vec<_>>())
        } else {
            req.json(
                &query
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), Value::String(v.clone())))
                    .collect::<serde_json::Map<String, Value>>(),
            )
        };
        let res = req.send().await.map_err(|e| net_err(&e))?;
        if let Some(t) = res
            .headers()
            .get("x-nd-authorization")
            .and_then(|v| v.to_str().ok())
            .filter(|t| !t.is_empty())
        {
            *self.lock() = t.to_string();
        }
        let status = res.status();
        if status.as_u16() == 401 {
            return Err(Unauthorized.into());
        }
        if !status.is_success() {
            bail!("The server returned HTTP {}.", status.as_u16());
        }
        let total = res
            .headers()
            .get("x-total-count")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or_default();
        Ok((res.json().await.unwrap_or_default(), total))
    }

    pub fn url(&self, method: &str, params: &[(&str, String)]) -> String {
        self.http
            .get(format!("{}/rest/{method}", self.server.url))
            .query(&self.query(params))
            .build()
            .map(|r| r.url().to_string())
            .unwrap_or_default()
    }

    pub fn stream_url(
        &self,
        song: &Song,
        format: Option<&str>,
        max_bitrate: u32,
    ) -> (String, bool) {
        let mut p = vec![("id", song.id.clone())];
        if let Some(f) = format {
            p.push(("format", f.to_string()));
        }
        if max_bitrate > 0 {
            p.push(("maxBitRate", max_bitrate.to_string()));
        }
        let transcode = max_bitrate > 0 || (format.is_some() && format != Some("raw"));
        (self.url("stream", &p), transcode)
    }

    pub async fn cover(&self, id: &str, size: u32) -> Result<Bytes> {
        let key = (id.to_string(), size);
        if let Some(b) = self.covers.lock().expect("covers").get(&key) {
            return Ok(b.clone());
        }
        let file = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("navify/covers")
            .join(&self.server.id)
            .join(format!("{id}-{size}"));
        if let Ok(b) = fs::read(&file).await {
            let b = Bytes::from(b);
            self.covers.lock().expect("covers").put(key, b.clone());
            return Ok(b);
        }
        let res = self
            .http
            .get(self.url("getCoverArt", &[("id", id.to_string()), ("size", size.to_string())]))
            .send()
            .await
            .map_err(|e| net_err(&e))?;
        if !res.status().is_success() {
            bail!("The server returned HTTP {} for cover art.", res.status().as_u16());
        }
        let b = res.bytes().await.map_err(|e| net_err(&e))?;
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir).await;
            let tmp = dir.join(format!("{}.tmp", std::process::id()));
            if fs::write(&tmp, &b).await.is_ok() {
                let _ = fs::rename(&tmp, &file).await;
            }
        }
        self.covers.lock().expect("covers").put(key, b.clone());
        Ok(b)
    }

    pub async fn albums(
        &self,
        sort: &str,
        filter: &[(&str, String)],
        offset: usize,
        size: usize,
    ) -> Result<(Vec<Album>, usize)> {
        if self.server.native {
            let (field, order) = sort_key(sort, ALBUM_SORTS, "importedAt");
            let mut q = vec![
                ("_start", offset.to_string()),
                ("_end", (offset + size).to_string()),
                ("_sort", field.to_string()),
                ("_order", order.to_string()),
            ];
            q.extend_from_slice(filter);
            if sort == "starred" {
                q.push(("starred", "true".to_string()));
            }
            self.nd(Method::GET, "album", &q).await
        } else {
            let t = ALBUM_SORTS.iter().find(|(k, ..)| *k == sort).map_or("newest", |(_, t, _)| *t);
            let mut q = vec![
                ("type", t.to_string()),
                ("size", size.to_string()),
                ("offset", offset.to_string()),
            ];
            for (k, v) in filter {
                if *k == "year" {
                    q.push(("fromYear", v.clone()));
                    q.push(("toYear", v.clone()));
                } else {
                    q.push((*k, v.clone()));
                }
            }
            let list: Vec<Album> = self.rest("getAlbumList2", &q, "/albumList2/album").await?;
            let total = offset + list.len() + usize::from(list.len() == size);
            Ok((list, total))
        }
    }

    pub async fn album(&self, id: &str) -> Result<(Album, Vec<Song>)> {
        if self.server.native {
            let id = id.to_string();
            let (album, _) = self.nd::<Album>(Method::GET, &format!("album/{id}"), &[]).await?;
            let (songs, _) = self
                .nd::<Vec<Song>>(
                    Method::GET,
                    "song",
                    &[
                        ("album_id", id),
                        ("_sort", "trackNumber".to_string()),
                        ("_order", "ASC".to_string()),
                    ],
                )
                .await?;
            Ok((album, songs))
        } else {
            let v: Value = self.rest("getAlbum", &[("id", id.to_string())], "/album").await?;
            Ok((serde_json::from_value(v.clone()).unwrap_or_default(), vlist(&v, "song")))
        }
    }

    pub async fn artists(&self, offset: usize, size: usize) -> Result<(Vec<Artist>, usize)> {
        if self.server.native {
            self.nd(
                Method::GET,
                "artist",
                &[
                    ("_start", offset.to_string()),
                    ("_end", (offset + size).to_string()),
                    ("_sort", "name".to_string()),
                    ("_order", "ASC".to_string()),
                ],
            )
            .await
        } else {
            let v: Value = self.rest("getArtists", &[], "/artists/index").await?;
            let all: Vec<Artist> = v
                .as_array()
                .map(|a| a.iter().flat_map(|i| vlist(i, "artist")).collect())
                .unwrap_or_default();
            let total = all.len();
            Ok((all.into_iter().skip(offset).take(size).collect(), total))
        }
    }

    pub async fn artist(&self, id: &str) -> Result<(Artist, Vec<Album>, Vec<Song>, Info)> {
        let (artist, albums) = if self.server.native {
            let (a, _) = self.nd::<Artist>(Method::GET, &format!("artist/{id}"), &[]).await?;
            let (albums, _) = self
                .nd::<Vec<Album>>(Method::GET, "album", &[("role_albumartist_id", id.to_string())])
                .await?;
            (a, albums)
        } else {
            let v: Value = self.rest("getArtist", &[("id", id.to_string())], "/artist").await?;
            (serde_json::from_value(v.clone()).unwrap_or_default(), vlist(&v, "album"))
        };
        let top: Vec<Song> = self
            .rest(
                "getTopSongs",
                &[("artist", artist.name.clone()), ("count", "20".to_string())],
                "/topSongs/song",
            )
            .await?;
        let info: Info = self
            .rest(
                "getArtistInfo2",
                &[
                    ("id", id.to_string()),
                    ("count", "20".to_string()),
                    ("includeNotPresent", "true".to_string()),
                ],
                "/artistInfo2",
            )
            .await?;
        Ok((artist, albums, top, info))
    }

    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        if self.server.native {
            Ok(self
                .nd::<Vec<Playlist>>(
                    Method::GET,
                    "playlist",
                    &[("_sort", "name".to_string()), ("_order", "ASC".to_string())],
                )
                .await?
                .0)
        } else {
            self.rest("getPlaylists", &[], "/playlists/playlist").await
        }
    }

    pub async fn playlist(&self, id: &str) -> Result<(Playlist, Vec<Song>)> {
        if self.server.native {
            let (pl, _) = self.nd::<Playlist>(Method::GET, &format!("playlist/{id}"), &[]).await?;
            let (mut tracks, _) =
                self.nd::<Vec<Value>>(Method::GET, &format!("playlist/{id}/tracks"), &[]).await?;
            for t in &mut tracks {
                if let Some(id) = t["mediaFileId"].as_str() {
                    t["id"] = Value::String(id.to_string());
                }
            }
            Ok((pl, serde_json::from_value(Value::Array(tracks)).unwrap_or_default()))
        } else {
            let v: Value = self.rest("getPlaylist", &[("id", id.to_string())], "/playlist").await?;
            Ok((serde_json::from_value(v.clone()).unwrap_or_default(), vlist(&v, "entry")))
        }
    }

    pub async fn songs(
        &self,
        sort: &str,
        filter: &[(&str, String)],
        offset: usize,
        size: usize,
    ) -> Result<(Vec<Song>, usize)> {
        if self.server.native {
            let (field, order) = sort_key(sort, SONG_SORTS, "title");
            let mut q = vec![
                ("_start", offset.to_string()),
                ("_end", (offset + size).to_string()),
                ("_sort", field.to_string()),
                ("_order", order.to_string()),
            ];
            q.extend_from_slice(filter);
            if sort == "starred" {
                q.push(("starred", "true".to_string()));
            }
            self.nd(Method::GET, "song", &q).await
        } else if sort == "starred" {
            let (.., songs) = self.starred().await?;
            let total = songs.len();
            Ok((songs, total))
        } else {
            let v: Value = self
                .rest(
                    "search3",
                    &[
                        ("query", String::new()),
                        ("songCount", size.to_string()),
                        ("songOffset", offset.to_string()),
                        ("artistCount", "0".to_string()),
                        ("albumCount", "0".to_string()),
                    ],
                    "/searchResult3",
                )
                .await?;
            let list: Vec<Song> = vlist(&v, "song");
            let total = offset + list.len() + usize::from(list.len() == size);
            Ok((list, total))
        }
    }

    pub async fn search(
        &self,
        query: &str,
        count: usize,
    ) -> Result<(Vec<Artist>, Vec<Album>, Vec<Song>)> {
        let c = count.to_string();
        let v: Value = self
            .rest(
                "search3",
                &[
                    ("query", query.to_string()),
                    ("artistCount", c.clone()),
                    ("albumCount", c.clone()),
                    ("songCount", c),
                ],
                "/searchResult3",
            )
            .await?;
        Ok((vlist(&v, "artist"), vlist(&v, "album"), vlist(&v, "song")))
    }

    pub async fn starred(&self) -> Result<(Vec<Artist>, Vec<Album>, Vec<Song>)> {
        let v: Value = self.rest("getStarred2", &[], "/starred2").await?;
        Ok((vlist(&v, "artist"), vlist(&v, "album"), vlist(&v, "song")))
    }

    pub async fn genres(&self) -> Result<Vec<Genre>> {
        self.rest("getGenres", &[], "/genres/genre").await
    }

    pub async fn lyrics(&self, song_id: &str) -> Result<Vec<Lyrics>> {
        self.rest(
            "getLyricsBySongId",
            &[("id", song_id.to_string())],
            "/lyricsList/structuredLyrics",
        )
        .await
    }

    pub async fn indexes(&self) -> Result<Vec<Artist>> {
        let v: Value = self.rest("getIndexes", &[], "/indexes/index").await?;
        Ok(v.as_array()
            .map(|a| a.iter().flat_map(|i| vlist(i, "artist")).collect())
            .unwrap_or_default())
    }

    pub async fn directory(&self, id: &str) -> Result<Vec<Song>> {
        self.rest("getMusicDirectory", &[("id", id.to_string())], "/directory/child").await
    }

    pub async fn random_songs(&self, count: usize) -> Result<Vec<Song>> {
        self.rest("getRandomSongs", &[("size", count.to_string())], "/randomSongs/song").await
    }

    pub async fn similar_songs(&self, song_id: &str, count: usize) -> Result<Vec<Song>> {
        self.rest(
            "getSimilarSongs2",
            &[("id", song_id.to_string()), ("count", count.to_string())],
            "/similarSongs2/song",
        )
        .await
    }

    pub async fn scrobble(&self, song_id: &str, submission: bool) -> Result<()> {
        self.rest::<Value>(
            "scrobble",
            &[("id", song_id.to_string()), ("submission", submission.to_string())],
            "",
        )
        .await?;
        Ok(())
    }

    pub async fn star(&self, ids: &[String], on: bool) -> Result<()> {
        let p: Vec<(&str, String)> = ids.iter().map(|i| ("id", i.clone())).collect();
        self.rest::<Value>(if on { "star" } else { "unstar" }, &p, "").await?;
        Ok(())
    }

    pub async fn rate(&self, id: &str, rating: u8) -> Result<()> {
        self.rest::<Value>(
            "setRating",
            &[("id", id.to_string()), ("rating", rating.to_string())],
            "",
        )
        .await?;
        Ok(())
    }

    pub async fn playlist_create(&self, name: &str, song_ids: &[String]) -> Result<Playlist> {
        let mut p = vec![("name", name.to_string())];
        p.extend(song_ids.iter().map(|i| ("songId", i.clone())));
        self.rest("createPlaylist", &p, "/playlist").await
    }

    pub async fn playlist_update(
        &self,
        id: &str,
        name: Option<&str>,
        public: Option<bool>,
        add: &[String],
        remove: &[usize],
    ) -> Result<()> {
        let mut p = vec![("playlistId", id.to_string())];
        if let Some(n) = name {
            p.push(("name", n.to_string()));
        }
        if let Some(b) = public {
            p.push(("public", b.to_string()));
        }
        p.extend(add.iter().map(|i| ("songIdToAdd", i.clone())));
        p.extend(remove.iter().map(|i| ("songIndexToRemove", i.to_string())));
        self.rest::<Value>("updatePlaylist", &p, "").await?;
        Ok(())
    }

    pub async fn playlist_delete(&self, id: &str) -> Result<()> {
        self.rest::<Value>("deletePlaylist", &[("id", id.to_string())], "").await?;
        Ok(())
    }

    pub async fn play_queue(&self) -> Result<(Vec<Song>, String, u64)> {
        let v: Value = self.rest("getPlayQueue", &[], "/playQueue").await?;
        Ok((
            vlist(&v, "entry"),
            v["current"].as_str().unwrap_or_default().to_string(),
            v["position"].as_u64().unwrap_or_default(),
        ))
    }

    pub async fn save_play_queue(
        &self,
        ids: &[String],
        current: &str,
        position_ms: u64,
    ) -> Result<()> {
        let mut p: Vec<(&str, String)> = ids.iter().map(|i| ("id", i.clone())).collect();
        p.push(("current", current.to_string()));
        p.push(("position", position_ms.to_string()));
        self.rest::<Value>("savePlayQueue", &p, "").await?;
        Ok(())
    }
}

impl Album {
    pub fn cover_id(&self) -> &str {
        if self.cover.is_empty() { &self.id } else { &self.cover }
    }

    pub fn artists(&self) -> &[Artist] {
        self.participants
            .get("albumartist")
            .filter(|v| !v.is_empty())
            .map_or(&self.artists, Vec::as_slice)
    }

    pub fn gains(&self) -> Rg {
        if self.rg == Rg::default() { self.rg2 } else { self.rg }
    }
}

impl Song {
    pub fn cover_id(&self) -> &str {
        if self.cover.is_empty() { &self.id } else { &self.cover }
    }

    pub fn artists(&self) -> &[Artist] {
        self.participants
            .get("artist")
            .filter(|v| !v.is_empty())
            .map_or(&self.artists, Vec::as_slice)
    }

    pub fn album_artists(&self) -> &[Artist] {
        self.participants
            .get("albumartist")
            .filter(|v| !v.is_empty())
            .map_or(&self.album_artists, Vec::as_slice)
    }

    pub fn gains(&self) -> Rg {
        if self.rg == Rg::default() { self.rg2 } else { self.rg }
    }
}

impl Artist {
    pub fn cover_id(&self) -> &str {
        if self.cover.is_empty() { &self.id } else { &self.cover }
    }
}

impl Playlist {
    pub fn cover_id(&self) -> &str {
        if self.cover.is_empty() { &self.id } else { &self.cover }
    }
}

fn md5_hex(s: &str) -> String {
    use md5::Digest;
    md5::Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn sort_key<'a>(
    sort: &str,
    map: &'a [(&'a str, &'a str, &'a str)],
    default: &'a str,
) -> (&'a str, &'a str) {
    let field = map.iter().find(|(k, ..)| *k == sort).map_or(default, |(.., f)| *f);
    let order =
        if matches!(sort, "name" | "artist" | "album" | "genre" | "year") { "ASC" } else { "DESC" };
    (field, order)
}

const ALBUM_SORTS: &[(&str, &str, &str)] = &[
    ("recent", "recent", "play_date"),
    ("newest", "newest", "recently_added"),
    ("frequent", "frequent", "play_count"),
    ("random", "random", "random"),
    ("name", "alphabeticalByName", "name"),
    ("artist", "alphabeticalByArtist", "album_artist"),
    ("starred", "starred", "starred_at"),
    ("year", "byYear", "max_year"),
    ("genre", "byGenre", "genre"),
    ("rating", "highest", "rating"),
];

const SONG_SORTS: &[(&str, &str, &str)] = &[
    ("recent", "recent", "play_date"),
    ("newest", "newest", "created_at"),
    ("frequent", "frequent", "play_count"),
    ("random", "random", "random"),
    ("name", "alphabeticalByName", "title"),
    ("artist", "alphabeticalByArtist", "artist"),
    ("album", "alphabeticalByName", "album"),
    ("starred", "starred", "starred_at"),
    ("year", "byYear", "year"),
    ("genre", "byGenre", "genre"),
    ("rating", "highest", "rating"),
];
