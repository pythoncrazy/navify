use anyhow::{Context, Error, Result};
use navify::api::{self, Album, Server, Session, Song};
use serde_json::json;

async fn session(prefix: &str, keys: [&str; 3]) -> Session {
    let _ = dotenvy::dotenv();
    let [h, u, p] = keys.map(|k| std::env::var(format!("{prefix}{k}")).expect(k));
    Session::login(&h, &u, &p).await.expect("login")
}

async fn read_suite(s: &Session) -> Result<()> {
    let (page, total) = s.albums("name", &[], 0, 2).await?;
    assert!(total >= 2 && page.len() == 2);
    assert!(page[0].name.to_lowercase() <= page[1].name.to_lowercase());
    assert_eq!(s.albums("name", &[], 1, 1).await?.0[0].id, page[1].id);
    for k in "recent newest frequent random artist album year rating".split(' ') {
        s.albums(k, &[], 0, 5).await.context(k)?;
        s.songs(k, 0, 5).await.context(k)?;
    }
    let genre = s.albums("newest", &[("genre", "No Genre")], 0, 5).await?;
    assert!(genre.0.is_empty());
    for a in &page {
        let (album, songs) = s.album(&a.id).await?;
        assert!(album.id == a.id && songs.iter().all(|x| x.album_id == a.id));
        assert!(songs.is_sorted_by_key(|x| (x.disc, x.track)) && !songs.is_empty());
    }
    let (album, songs) = s.album(&page[0].id).await?;
    let (artists, n) = s.artists(0, 5).await?;
    assert!(n >= artists.len() && !artists.is_empty());
    let id = &album.artist_id;
    let ((artist, discs), top, info) =
        tokio::try_join!(s.artist(id), s.top_songs(id), s.artist_info(id))?;
    assert!(artist.id == *id && discs.iter().any(|a| a.id == album.id));
    assert!(top.is_empty() != s.server.native);
    assert!(info.similar.iter().all(|a| a.album_count > 0));
    let word = album.artist.split(' ').next().unwrap_or_default();
    let found = s.search(word, 20).await?.1;
    assert!(found.iter().any(|a| a.artist_id == album.artist_id));
    let (name, kids) = s.directory(&s.indexes().await?[0].id).await?;
    assert!(!name.is_empty() && !kids.is_empty());
    s.genres().await?;
    s.lyrics(&songs[0].id).await?;
    s.similar_songs(&songs[0].id, 3).await?;
    let cover = s.cover(&album.id, 128).await?;
    assert!(image::load_from_memory(&cover).is_ok() && s.cover_file(&album.id, 128).exists());
    Ok(())
}

#[test]
fn song_and_album_parse_from_native_and_subsonic_json() -> Result<()> {
    let native = json!({
        "id": "n1", "title": "T", "albumId": "al1", "trackNumber": 3,
        "discNumber": 2, "starred": true, "rating": 4, "rgTrackGain": -7.5, "rgAlbumPeak": 1.2,
        "participants": {"artist": [{"id": "ar1", "name": "Ar"}]}, "hasCoverArt": true
    });
    let subsonic = json!({
        "id": "n1", "title": "T", "albumId": "al1", "track": 3, "discNumber": 2,
        "starred": "2026-01-01T00:00:00Z", "userRating": 4, "coverArt": "n1", "year": null,
        "artists": [{"id": "ar1", "name": "Ar"}],
        "replayGain": {"trackGain": -7.5, "albumPeak": 1.2}
    });
    for v in [native, subsonic] {
        let s: Song = api::parse(v)?;
        let names = (&*s.id, &*s.cover, &*s.album_id, &*s.artists[0].name);
        assert_eq!(names, ("n1", "n1", "al1", "Ar"));
        assert_eq!(s.artists[0].cover, "o:ar1");
        assert_eq!((s.track, s.disc, s.starred, s.user_rating), (3, 2, true, 4));
        assert_eq!((s.rg.track_gain, s.rg.album_peak), (Some(-7.5), Some(1.2)));
    }
    let bare = json!({"id": "s", "albumId": "al1", "hasCoverArt": false, "imageHash": "h"});
    assert_eq!(api::parse::<Song>(bare)?.cover, "al1#h");
    let sub = api::parse::<serde_json::Value>(json!({"id": "s", "coverArt": "al-al1_h"}))?;
    assert_eq!(api::parse::<Song>(sub)?.cover, "al1#h");
    assert!(api::parse::<Song>(json!({"title": 5})).is_err());
    assert!(api::parse::<Vec<Song>>(serde_json::Value::Null)?.is_empty());
    let native = json!({"id": "al1", "maxYear": 2015, "tags": {"recordlabel": ["L"], "releasetype": ["ep"]}});
    let subsonic = json!({
        "id": "al1", "year": 2015, "recordLabels": [{"name": "L"}], "releaseTypes": ["ep"],
        "releaseDate": {"year": 2015, "month": 3, "day": 9}
    });
    for v in [native, subsonic] {
        let a: Album = api::parse(v)?;
        assert_eq!(
            (a.year, a.labels, a.release_types),
            (2015, vec!["L".into()], vec!["ep".into()])
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "needs the local test server"]
async fn local_reads_and_writes_with_both_apis() -> Result<()> {
    let mut s = session("NAVIFY_TEST_LOCAL_", ["URL", "USERNAME", "PASSWORD"]).await;
    assert!(s.server.native && !s.jwt().is_empty());
    assert!(s.server.ext.iter().any(|e| e == "topSongsByArtistId"));
    let bad = Session::new(Server {
        jwt: "x".into(),
        ..s.server.clone()
    });
    let e = bad
        .albums("newest", &[], 0, 1)
        .await
        .expect_err("an old token fails");
    let kind = e.downcast_ref::<std::io::Error>().map(std::io::Error::kind);
    assert_eq!(kind, Some(std::io::ErrorKind::PermissionDenied));
    read_suite(&s).await?;
    s.server.native = false;
    read_suite(&s).await?;
    let songs = s.songs("name", 0, 4).await?.0;
    let ids: Vec<String> = songs.into_iter().map(|x| x.id).collect();
    let starred = async || Ok::<_, Error>(s.starred().await?.2.iter().any(|x| x.id == ids[0]));
    s.star(&ids[..1], true).await?;
    assert!(starred().await?);
    s.star(&ids[..1], false).await?;
    assert!(!starred().await?);
    s.rate(&ids[0], 4).await?;
    s.scrobble(&ids[0], false).await?;
    s.scrobble(&ids[0], true).await?;
    let pl = s.playlist_create("navify test", &ids[..2]).await?;
    assert_eq!(pl.song_count, 2);
    let name = Some("navify test 2");
    s.playlist_update(&pl.id, name, None, &ids[2..3], &[0])
        .await?;
    let (pl2, tracks) = s.playlist(&pl.id).await?;
    assert_eq!(Some(pl2.name.as_str()), name);
    assert!(tracks.iter().map(|t| &t.id).eq([&ids[1], &ids[2]]));
    s.playlist_delete(&pl.id).await?;
    assert!(!s.playlists().await?.iter().any(|p| p.id == pl.id));
    s.save_play_queue(&ids, &ids[1], 1234).await?;
    let (queue, cur, pos) = s.play_queue().await?;
    assert!(queue.iter().map(|x| &x.id).eq(&ids) && (cur, pos) == (ids[1].clone(), 1234));
    Ok(())
}

#[tokio::test]
#[ignore = "needs the .env server"]
async fn live_reads_are_read_only() -> Result<()> {
    let s = session("NAVIDROME_", ["HOST", "USERNAME", "PASSWORD"]).await;
    let (albums, total) = s.albums("frequent", &[], 0, 5).await?;
    assert!(total > 0 && !albums.is_empty());
    let songs = s.album(&albums[0].id).await?.1;
    assert!(songs.iter().any(|x| x.rg.track_gain.is_some()));
    Ok(())
}
