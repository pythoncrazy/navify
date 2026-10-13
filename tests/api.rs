use navify::api::{self, Session, Song};
use serde_json::json;

fn session(vars: [&str; 3]) -> Session {
    let _ = dotenvy::dotenv();
    let [h, u, p] = vars.map(|k| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set")));
    rt().block_on(Session::login(&h, &u, &p))
        .unwrap_or_else(|e| panic!("login to {h} failed: {e}"))
}

fn rt() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().expect("runtime"))
}

fn local() -> Session {
    session([
        "NAVIFY_TEST_LOCAL_URL",
        "NAVIFY_TEST_LOCAL_USERNAME",
        "NAVIFY_TEST_LOCAL_PASSWORD",
    ])
}

async fn read_suite(s: &Session) {
    let (page, total) = s.albums("name", &[], 0, 2).await.expect("albums");
    assert!(total >= 2 && page.len() == 2);
    assert!(page[0].name.to_lowercase() <= page[1].name.to_lowercase());
    let (next, _) = s.albums("name", &[], 1, 1).await.expect("albums page 2");
    assert_eq!(next[0].id, page[1].id);
    for k in [
        "recent", "newest", "frequent", "random", "artist", "year", "rating",
    ] {
        s.albums(k, &[], 0, 5)
            .await
            .unwrap_or_else(|e| panic!("albums {k}: {e}"));
    }
    for k in [
        "name", "artist", "album", "newest", "recent", "frequent", "year", "rating",
    ] {
        s.songs(k, 0, 5)
            .await
            .unwrap_or_else(|e| panic!("songs {k}: {e}"));
    }
    let (album, songs) = s.album(&page[0].id).await.expect("album");
    assert_eq!(album.id, page[0].id);
    assert!(!songs.is_empty() && songs.iter().all(|x| x.album_id == album.id));
    assert!(
        songs
            .windows(2)
            .all(|w| (w[0].disc, w[0].track) <= (w[1].disc, w[1].track))
    );
    let (artists, n) = s.artists(0, 5).await.expect("artists");
    assert!(n >= artists.len() && !artists.is_empty());
    let id = &album.artist_id;
    let ((artist, discs), _, info) =
        tokio::try_join!(s.artist(id), s.top_songs(id), s.artist_info(id)).expect("artist");
    assert!(artist.id == *id && discs.iter().any(|a| a.id == album.id));
    assert!(info.similar.iter().all(|a| a.album_count > 0));
    let word = album
        .artist
        .split_whitespace()
        .next()
        .expect("the album has an artist");
    let (_, found, _) = s.search(word, 20).await.expect("search");
    assert!(
        found.iter().any(|a| a.artist_id == album.artist_id),
        "search for {word} misses the artist"
    );
    let idx = s.indexes().await.expect("indexes");
    assert!(!s.directory(&idx[0].id).await.expect("directory").is_empty());
    for p in s.playlists().await.expect("playlists").iter().take(1) {
        assert_eq!(s.playlist(&p.id).await.expect("playlist").0.id, p.id);
    }
    s.genres().await.expect("genres");
    s.lyrics(&songs[0].id).await.expect("lyrics");
    s.similar_songs(&songs[0].id, 3).await.expect("similar");
    let cover = s.cover(&album.id, 128).await.expect("cover");
    assert!(image::load_from_memory(&cover).is_ok());
    assert!(s.cover_file(&album.id, 128).exists());
}

#[test]
fn song_parses_from_native_and_subsonic_json() {
    let native = json!({
        "id": "row1", "mediaFileId": "n1", "title": "T", "albumId": "al1", "trackNumber": 3,
        "discNumber": 2, "starred": true, "rating": 4, "rgTrackGain": -7.5, "rgAlbumPeak": 1.2,
        "participants": {"artist": [{"id": "ar1", "name": "Ar"}]}, "hasCoverArt": true
    });
    let subsonic = json!({
        "id": "n1", "title": "T", "albumId": "al1", "track": 3, "discNumber": 2,
        "starred": "2026-01-01T00:00:00Z", "userRating": 4, "coverArt": "n1",
        "artists": [{"id": "ar1", "name": "Ar"}], "replayGain": {"trackGain": -7.5, "albumPeak": 1.2}
    });
    for v in [native, subsonic] {
        let s: Song = api::parse(v).expect("song");
        assert_eq!(
            (s.id.as_str(), s.cover.as_str(), s.album_id.as_str()),
            ("n1", "n1", "al1")
        );
        assert_eq!((s.track, s.disc, s.starred, s.user_rating), (3, 2, true, 4));
        assert_eq!((s.rg.track_gain, s.rg.album_peak), (Some(-7.5), Some(1.2)));
        assert_eq!(s.artists[0].name, "Ar");
    }
    assert!(api::parse::<Song>(json!({"title": 5})).is_err());
    assert_eq!(
        api::parse::<Vec<Song>>(serde_json::Value::Null)
            .expect("null")
            .len(),
        0
    );
}

#[test]
#[ignore = "needs the local test server"]
fn local_reads_with_native_and_subsonic_api() {
    let mut s = local();
    assert!(s.server.native && !s.jwt().is_empty());
    rt().block_on(read_suite(&s));
    s.server.native = false;
    rt().block_on(read_suite(&s));
}

#[test]
#[ignore = "needs the local test server"]
fn local_writes_round_trip() {
    let s = local();
    rt().block_on(async {
        let (songs, _) = s.songs("name", 0, 4).await.expect("songs");
        let ids: Vec<String> = songs.iter().map(|x| x.id.clone()).collect();
        s.star(&ids[..1], true).await.expect("star");
        assert!(
            s.starred()
                .await
                .expect("starred")
                .2
                .iter()
                .any(|x| x.id == ids[0])
        );
        s.star(&ids[..1], false).await.expect("unstar");
        assert!(
            !s.starred()
                .await
                .expect("starred")
                .2
                .iter()
                .any(|x| x.id == ids[0])
        );
        s.rate(&ids[0], 4).await.expect("rate");
        s.scrobble(&ids[0], false).await.expect("now playing");
        s.scrobble(&ids[0], true).await.expect("scrobble");
        let pl = s
            .playlist_create("navify test", &ids[..2])
            .await
            .expect("create");
        assert_eq!(pl.song_count, 2);
        s.playlist_update(&pl.id, Some("navify test 2"), None, &ids[2..3], &[0])
            .await
            .expect("update");
        let (pl2, tracks) = s.playlist(&pl.id).await.expect("playlist");
        assert_eq!(pl2.name, "navify test 2");
        assert_eq!(
            tracks.iter().map(|t| &t.id).collect::<Vec<_>>(),
            [&ids[1], &ids[2]]
        );
        s.playlist_delete(&pl.id).await.expect("delete");
        assert!(
            !s.playlists()
                .await
                .expect("playlists")
                .iter()
                .any(|p| p.id == pl.id)
        );
        s.save_play_queue(&ids, &ids[1], 1234)
            .await
            .expect("save queue");
        let (queue, cur, pos) = s.play_queue().await.expect("queue");
        assert_eq!(
            queue.iter().map(|x| &x.id).collect::<Vec<_>>(),
            ids.iter().collect::<Vec<_>>()
        );
        assert_eq!((cur, pos), (ids[1].clone(), 1234));
    });
}

#[test]
#[ignore = "needs the .env server"]
fn live_reads_are_read_only() {
    let s = session(["NAVIDROME_HOST", "NAVIDROME_USERNAME", "NAVIDROME_PASSWORD"]);
    rt().block_on(async {
        let (albums, total) = s.albums("frequent", &[], 0, 5).await.expect("albums");
        assert!(total > 0 && !albums.is_empty());
        let (_, songs) = s.album(&albums[0].id).await.expect("album");
        assert!(songs.iter().any(|x| x.rg.track_gain.is_some()));
    });
}
