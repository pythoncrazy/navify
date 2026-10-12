# navify

A desktop client for Navidrome and OpenSubsonic servers. It looks like Spotify.
It uses few resources. It runs on Linux, macOS, and Windows.

## Features

- Play your music with gapless playback, crossfade, and ReplayGain.
- Browse albums, artists, playlists, and folders.
- Search, lyrics, artist radio, and liked songs.
- Sync the play queue with the server.
- Control playback with media keys and the system media controls.

## Build

Install Rust 1.90 or newer. Then build:

```sh
cargo build --release
```

On Linux, install these packages first:

```sh
sudo apt-get install libasound2-dev libdbus-1-dev pkg-config
```

## Development

Install prek to run the git hooks:

```sh
cargo install --locked prek
prek install
```

Run the checks before you commit:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
prek run --all-files
```

Some tests use a live Navidrome server. Put `NAVIDROME_HOST`,
`NAVIDROME_USERNAME`, and `NAVIDROME_PASSWORD` in `.env` to run them.
The tests skip when these values are absent.

## License

GPL-3.0-or-later. See LICENSE.

Bundled fonts: Figtree under the SIL OFL 1.1 (assets/fonts/OFL.txt),
Lucide under the ISC license (assets/fonts/LUCIDE-LICENSE.txt).
