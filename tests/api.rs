use navify::api::{self, Session, Song};

fn creds() -> Option<(String, String, String)> {
    let _ = dotenvy::dotenv();
    match (
        std::env::var("NAVIDROME_HOST"),
        std::env::var("NAVIDROME_USERNAME"),
        std::env::var("NAVIDROME_PASSWORD"),
    ) {
        (Ok(h), Ok(u), Ok(p)) if !h.is_empty() => Some((h, u, p)),
        _ => None,
    }
}

fn rt() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().expect("runtime"))
}

static SESSION: tokio::sync::OnceCell<Option<Session>> = tokio::sync::OnceCell::const_new();

async fn session() -> Option<Session> {
    SESSION
        .get_or_init(|| async {
            let (h, u, p) = creds()?;
            Session::login(&h, &u, &p).await.ok()
        })
        .await
        .clone()
}

macro_rules! live {
    () => {
        match session().await {
            Some(s) => s,
            None => {
                eprintln!("skip: no server credentials in .env");
                return;
            }
        }
    };
}

static LOCAL: tokio::sync::OnceCell<Option<Session>> = tokio::sync::OnceCell::const_new();

async fn local() -> Option<Session> {
    LOCAL
        .get_or_init(|| async {
            match (
                std::env::var("NAVIFY_TEST_LOCAL_URL"),
                std::env::var("NAVIFY_TEST_LOCAL_USERNAME"),
                std::env::var("NAVIFY_TEST_LOCAL_PASSWORD"),
            ) {
                (Ok(h), Ok(u), Ok(p)) => match Session::login(&h, &u, &p).await {
                    Ok(s) => Some(s),
                    Err(e) => panic!("local login failed: {e}"),
                },
                _ => None,
            }
        })
        .await
        .clone()
}

macro_rules! local {
    () => {
        match local().await {
            Some(s) => s,
            None => {
                eprintln!("skip: no NAVIFY_TEST_LOCAL_* vars");
                return;
            }
        }
    };
}

#[test]
fn login_detects_navidrome() {
    rt().block_on(async {
        let s = live!();
        assert!(s.server.native);
        assert!(!s.server.token.is_empty());
        assert!(!s.jwt().is_empty());
    });
}

#[test]
fn albums_page_and_total() {
    rt().block_on(async {
        let s = live!();
        let (page, total) = s.albums("name", &[], 0, 25).await.expect("albums");
        assert_eq!(page.len(), 25);
        assert!(total > 100);
        assert!(page.windows(2).all(|w| w[0].name <= w[1].name));
        let (page2, _) = s.albums("name", &[], 25, 25).await.expect("albums page 2");
        assert!(page2[0].name >= page[24].name);
    });
}

#[test]
fn album_detail_has_songs_with_gain() {
    rt().block_on(async {
        let s = live!();
        let (albums, _) = s.albums("frequent", &[], 0, 5).await.expect("albums");
        let (album, songs) = s.album(&albums[0].id).await.expect("album");
        assert_eq!(album.id, albums[0].id);
        assert!(!songs.is_empty());
        assert!(songs.windows(2).all(|w| w[0].track <= w[1].track));
        assert!(songs.iter().any(|s| s.rg.track_gain.is_some()));
        assert!(songs.iter().all(|s| !s.album_id.is_empty()));
    });
}

#[test]
fn artist_page() {
    rt().block_on(async {
        let s = live!();
        let (artists, total) = s.artists(0, 5).await.expect("artists");
        assert!(total > 10);
        let (artist, albums, top, info) = s.artist(&artists[0].id).await.expect("artist");
        assert_eq!(artist.id, artists[0].id);
        assert!(!artist.name.is_empty());
        let _ = albums;
        let _ = top;
        let _ = info;
    });
}

#[test]
fn playlists_and_tracks() {
    rt().block_on(async {
        let s = live!();
        let playlists = s.playlists().await.expect("playlists");
        assert!(!playlists.is_empty());
        let (pl, songs) = s.playlist(&playlists[0].id).await.expect("playlist");
        assert_eq!(pl.id, playlists[0].id);
        if !songs.is_empty() {
            assert!(songs.iter().all(|s| s.id.len() > 4));
        }
    });
}

#[test]
fn search_returns_results() {
    rt().block_on(async {
        let s = live!();
        let (albums, _) = s.albums("name", &[], 0, 1).await.expect("albums");
        let word: String = albums[0].name.chars().filter(|c| c.is_alphabetic()).take(4).collect();
        let (artists, albums, songs) = s.search(&word, 5).await.expect("search");
        assert!(!(artists.is_empty() && albums.is_empty() && songs.is_empty()));
    });
}

#[test]
fn lyrics_call_succeeds() {
    rt().block_on(async {
        let s = live!();
        let (songs, _) = s.songs("frequent", &[], 0, 10).await.expect("songs");
        for song in &songs {
            if s.lyrics(&song.id).await.is_ok_and(|l| !l.is_empty()) {
                return;
            }
        }
        let _ = s.lyrics(&songs[0].id).await.expect("lyrics call");
    });
}

#[test]
fn cover_caches_on_disk() {
    rt().block_on(async {
        let s = live!();
        let (albums, _) = s.albums("name", &[], 0, 1).await.expect("albums");
        let id = albums[0].cover.clone();
        let first = s.cover(&id, 128).await.expect("cover fetch");
        assert!(!first.is_empty());
        let start = std::time::Instant::now();
        let second = s.cover(&id, 128).await.expect("cover cache");
        assert_eq!(first, second);
        assert!(start.elapsed() < std::time::Duration::from_millis(500));
    });
}

#[test]
fn subsonic_fallback_lists_albums() {
    rt().block_on(async {
        let mut s = live!();
        s.server.native = false;
        let (albums, _) = s.albums("name", &[], 0, 10).await.expect("subsonic albums");
        assert_eq!(albums.len(), 10);
        assert!(albums.iter().all(|a| !a.name.is_empty() && !a.cover.is_empty()));
        let songs = s.songs("starred", &[], 0, 10).await.expect("starred songs");
        assert!(songs.0.iter().all(|s| s.starred));
    });
}

#[test]
fn song_deserializes_native_and_subsonic() {
    let native = serde_json::json!({
        "id": "n1",
        "title": "T",
        "album": "A",
        "albumId": "al1",
        "artist": "Ar",
        "artistId": "ar1",
        "albumArtist": "AA",
        "albumArtistId": "aa1",
        "trackNumber": 3,
        "discNumber": 1,
        "year": 2020,
        "duration": 123.4,
        "suffix": "flac",
        "bitRate": 900,
        "playCount": 5,
        "starred": true,
        "rating": 4,
        "explicitStatus": "e",
        "genres": [{"id": "g1", "name": "Rock"}],
        "rgTrackGain": -7.5,
        "rgAlbumGain": -8.0,
        "rgTrackPeak": 1.1,
        "rgAlbumPeak": 1.2,
        "participants": {"artist": [{"id": "ar1", "name": "Ar"}], "albumartist": [{"id": "aa1", "name": "AA"}]},
        "hasCoverArt": true
    });
    let subsonic = serde_json::json!({
        "id": "s1",
        "title": "T",
        "album": "A",
        "albumId": "al1",
        "artist": "Ar",
        "artistId": "ar1",
        "track": 3,
        "discNumber": 1,
        "year": 2020,
        "duration": 123,
        "suffix": "mp3",
        "bitRate": 320,
        "playCount": 5,
        "starred": "2026-01-01T00:00:00Z",
        "userRating": 4,
        "explicitStatus": "explicit",
        "coverArt": "mf-s1_x",
        "genres": [{"name": "Rock"}],
        "artists": [{"id": "ar1", "name": "Ar"}],
        "albumArtists": [{"id": "aa1", "name": "AA"}],
        "replayGain": {"trackGain": -7.5, "albumGain": -8.0, "trackPeak": 1.1, "albumPeak": 1.2}
    });
    let n: Song = api::parse(native).expect("native song");
    let s: Song = api::parse(subsonic).expect("subsonic song");
    for song in [&n, &s] {
        assert_eq!(song.title, "T");
        assert_eq!(song.album_id, "al1");
        assert_eq!(song.track, 3);
        assert!(song.starred);
        assert_eq!(song.user_rating, 4);
        assert!(song.explicit);
        assert_eq!(song.genres[0].name, "Rock");
        assert_eq!(song.rg.track_gain, Some(-7.5));
        assert_eq!(song.rg.album_peak, Some(1.2));
    }
    assert_eq!(n.id, "n1");
    assert_eq!(n.artists[0].name, "Ar");
    assert_eq!(n.album_artists[0].name, "AA");
    assert_eq!(s.id, "s1");
    assert_eq!(s.cover, "mf-s1_x");
    assert_eq!(s.artists[0].id, "ar1");
    assert_eq!(s.album_artists[0].name, "AA");
    assert_eq!(n.cover, "n1");
    assert!((s.duration - 123.0).abs() < f32::EPSILON);
}

#[test]
fn local_star_rate_scrobble() {
    rt().block_on(async {
        let s = local!();
        let (songs, _) = s.songs("name", &[], 0, 5).await.expect("songs");
        assert!(!songs.is_empty());
        let id = songs[0].id.clone();
        s.star(std::slice::from_ref(&id), true).await.expect("star");
        let (.., starred) = s.starred().await.expect("starred");
        assert!(starred.iter().any(|x| x.id == id));
        s.star(std::slice::from_ref(&id), false).await.expect("unstar");
        let (.., starred) = s.starred().await.expect("starred 2");
        assert!(!starred.iter().any(|x| x.id == id));
        s.rate(&id, 4).await.expect("rate");
        s.scrobble(&id, true).await.expect("scrobble");
        s.scrobble(&id, false).await.expect("now playing");
    });
}

#[test]
fn local_playlist_roundtrip() {
    rt().block_on(async {
        let s = local!();
        let (songs, _) = s.songs("name", &[], 0, 5).await.expect("songs");
        let ids: Vec<String> = songs.iter().take(3).map(|x| x.id.clone()).collect();
        let pl = s.playlist_create("navify test", &ids[..2]).await.expect("create");
        assert_eq!(pl.song_count, 2);
        let (_pl2, tracks) = s.playlist(&pl.id).await.expect("playlist");
        assert_eq!(tracks.len(), 2);
        s.playlist_update(&pl.id, Some("navify test 2"), None, &ids[2..3], &[0])
            .await
            .expect("update");
        let (pl3, tracks) = s.playlist(&pl.id).await.expect("playlist 2");
        assert_eq!(pl3.name, "navify test 2");
        assert_eq!(tracks.len(), 2);
        assert!(tracks.iter().any(|t| t.id == ids[2]));
        s.playlist_delete(&pl.id).await.expect("delete");
        assert!(!s.playlists().await.expect("playlists").iter().any(|p| p.id == pl.id));
    });
}

#[test]
fn local_play_queue_roundtrip() {
    rt().block_on(async {
        let s = local!();
        let (songs, _) = s.songs("name", &[], 0, 4).await.expect("songs");
        let ids: Vec<String> = songs.iter().map(|x| x.id.clone()).collect();
        s.save_play_queue(&ids, &ids[1], 1234).await.expect("save queue");
        let (queue, current, position) = s.play_queue().await.expect("queue");
        assert_eq!(queue.len(), ids.len());
        assert_eq!(current, ids[1]);
        assert_eq!(position, 1234);
    });
}

#[test]
fn local_subsonic_read_suite() {
    rt().block_on(async {
        let mut s = local!();
        s.server.native = false;
        let (albums, _) = s.albums("name", &[], 0, 10).await.expect("albums");
        assert!(!albums.is_empty());
        let (album, songs) = s.album(&albums[0].id).await.expect("album");
        assert_eq!(album.id, albums[0].id);
        assert!(!songs.is_empty());
        let (artists, _) = s.artists(0, 10).await.expect("artists");
        let (artist, albums, top, info) = s.artist(&artists[0].id).await.expect("artist");
        let _ = (albums, top, info, artist);
        let playlists = s.playlists().await.expect("playlists");
        if let Some(pl) = playlists.first() {
            let _ = s.playlist(&pl.id).await.expect("playlist");
        }
        let (a, b, c) = s.search("a", 5).await.expect("search");
        let _ = (a, b, c);
        let _ = s.genres().await.expect("genres");
        let idx = s.indexes().await.expect("indexes");
        assert!(!idx.is_empty());
        let dir = s.directory(&idx[0].id).await.expect("directory");
        assert!(!dir.is_empty());
        let _ = s.random_songs(3).await.expect("random");
        let _ = s.similar_songs(&songs[0].id, 3).await.expect("similar");
        let _ = s.starred().await.expect("starred");
        let _ = s.lyrics(&songs[0].id).await.expect("lyrics");
    });
}
