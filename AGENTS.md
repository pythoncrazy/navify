# navify

Spotify-lookalike desktop client for Navidrome / any OpenSubsonic server. Rust, iced 0.14, pure-Rust audio. Targets Linux, macOS, Windows. License GPL-3.0-or-later.

## Code style (strict)

- Concise code, no filler, no comments, no doc comments unless a public API is genuinely non-obvious.
- No tiny one-use helper functions; inline them. Prefer fewer, meatier functions.
- No fallbacks that hide bugs. Use `assert!` or `expect` for internal invariants. Show real errors in the UI.
- Functional style: iterator chains, `Option`/`Result` combinators, early returns. Few enums (only where iced/protocol forces them), few types: one model struct per entity that deserializes from both Navidrome-native and Subsonic JSON via `#[serde(alias)]`.
- Use std and existing dependency features before writing code. Continuously look for LOC reductions.
- No `unwrap()` on network/IO data paths; errors bubble to a single boundary and show in the UI.

## Writing (strict)

- All writing must obey ASD-STE100 (Simplified Technical English): UI text, error messages, docs, commit messages, PR text, and code identifiers where practical.
- Use approved words with one meaning. Use short sentences: 20 words maximum for procedures, 25 for descriptions. Use active voice. Write one instruction in each sentence. Do not use slang or idioms.

## Layout

- `src/` — app crate `navify` (iced UI, API client, caches, OS integration).
- `crates/audio` — `navify-audio`: playback engine (symphonia 0.6 + `opus-decoder` + cpal + rubato + stream-download). No UI or Subsonic knowledge.

## Architecture decisions

- API: OpenSubsonic baseline (`/rest/*`, token+salt auth, `f=json`). When `ping` reports `type == "navidrome"`, log in via `POST /auth/login` and use native `/api/*` (react-admin params `_start,_end,_sort,_order`, `X-Total-Count`) for browsing/paging like Feishin; always store the refreshed JWT from the `x-nd-authorization` response header. Streaming, cover art, scrobble, star, rating, lyrics, similar/top songs always go through `/rest/*`.
- Credentials (Feishin-style): config file (0600) holds servers with url, username, subsonic salt+token, JWT. Optional "remember password" stores password in OS keyring for re-login when JWT expires; without it, fall back to Subsonic-only paths.
- Caching (Feishin-style): in-memory query cache keyed by request with a stale time; in-flight request coalescing; optimistic local updates (star/rating/playlist edits) instead of refetching. Cover art: requested only in size buckets, cached forever on disk (server serves immutable covers) and in a bounded in-memory LRU of `image::Handle`s.
- UI: never block `update`; all IO via `Task`. Virtualized long lists (render visible rows only). Bundled fonts: Figtree (text) + an icon font. No svg feature.
- Playback: gapless, crossfade, ReplayGain (gain computed by app, applied by engine), seek (range requests for raw streams, `timeOffset` for transcoded), optional transcode format/max bitrate, server play-queue sync, scrobble (`submission=false` on start, `true` after 50% or 4 min).
- OS: souvlaki (MPRIS/SMTC/macOS Now Playing). No Discord Rich Presence. Linux uses libdbus (no zbus) for both souvlaki and keyring.

## Resource budget (strict)

- Idle (paused, window open): 0% CPU. No timers, no polling, no redraws. iced 0.14 reactive rendering only.
- Playing: progress updates only while the window is visible, at 4 Hz maximum. Minimized or hidden window: no UI updates at all.
- Paused: stop the cpal stream. After 30 s paused, close the output device. Audio and network threads block on channels, never spin.
- Memory: one tokio runtime for the whole process, shared with the audio engine, with a small worker count. Bounded cover LRU (by bytes). Request covers at display size buckets only. Virtualized lists. Drop page data that is not visible after the cache stale time.
- Renderer: wgpu with tiny-skia fallback. Setting/env (`ICED_BACKEND=tiny-skia`) forces CPU rendering for the lowest RAM.
- Binary: minimal crate features, `panic = "abort"`, stripped. Measure RSS and idle CPU on each phase and report them.

## Commands

- Build: `cargo build` · Run: `cargo run` · Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` · Format: `cargo fmt --all`
- Logo: `cargo run --example logo` writes `assets/navify.svg`, `assets/navify.ico`, and `assets/icons/hicolor`. On Linux, `navify --install` writes the desktop entry and icons to the user data directory.
- Hooks: install prek with `cargo install --locked prek`. Make sure `~/.cargo/bin` is on PATH. Then run `prek install` and `prek run --all-files`.
- Server tests are `#[ignore]`. Run them with `cargo test --workspace -- --include-ignored`. Live tests read `.env` (`NAVIDROME_HOST`, `NAVIDROME_USERNAME`, `NAVIDROME_PASSWORD`). Local tests read `NAVIFY_TEST_LOCAL_URL`, `NAVIFY_TEST_LOCAL_USERNAME`, and `NAVIFY_TEST_LOCAL_PASSWORD`. The audio player test also needs `ALSA_CONFIG_PATH=crates/audio/tests/null.conf`. A missing value makes the test fail. Never print or commit credentials.
- Dependencies: only versions published ≥7 days ago.
- Tools: install all developer tools with `cargo install --locked` (not uv, pip, or system packages).
- Linux build deps: `libasound2-dev libdbus-1-dev pkg-config` (Arch: `alsa-lib dbus pkgconf`).

## Status (2026-10-14)

- `main`: the full app. CI (clippy, tests, prek) passes on Linux, macOS, and Windows. Work goes in small branches with STE pull requests to `main`.
  - `src/app.rs`, `src/pages.rs`, `src/view.rs`, `src/theme.rs`: the iced UI (Phase B done).
  - `crates/audio`: the playback engine. It uses the app runtime. It closes the device after 30 s of pause. `crates/audio/tests` test gapless, seek, crossfade, gain, and events on an ALSA null device.
  - OS: souvlaki media controls. `src/tray.rs`: tray icon (Linux StatusNotifierItem and dbusmenu over `dbus-crossroads`; `tray-icon` on Windows and macOS). The app runs as an iced `daemon`. When the tray setting is on, close hides the window: Windows hides it (SMTC needs the HWND), other systems close it and open it again.
  - Linux display: `main.rs` probes Wayland for `zxdg_decoration_manager_v1`. When the compositor draws no title bars (GNOME), Navify uses X11 to get the system title bar.
  - Rendering: the window background has the panel color (#121212). Small black quads draw the bars and the gaps, and one overlay draws the panel corners. Do not put a large quad under scrolled content: tiny-skia fills each quad that is not fully in the redraw area through the clip mask, at full size.
  - Covers: the cache keeps the compressed bytes (8 MB limit), and the renderer decodes only the visible covers. The artist banner stays decoded for the 3:1 crop.
  - Release: `[profile.release]` uses fat LTO, one codegen unit, and `opt-level = "s"`. The audio and render crates use `opt-level = 3`. Linux GNU builds use packed relative relocations (`.cargo/config.toml`), so the binary needs glibc 2.36 or later. `cargo packager --release` makes deb, AppImage, pacman, NSIS, and dmg (`[package.metadata.packager]`). `.github/workflows/release.yml` builds all targets on a `v*` tag and makes a draft release. The tag must match the Cargo version.
  - `examples/shots.rs`: renders screenshots of the routes with `iced_test`.
  - Spotify parts: keyboard shortcuts dialog (Ctrl+/), Recently played tab in the queue panel, full screen view, device menu in the player bar, album footer with a More by shelf (cached artist data only), playlist search and sort, and a compact track view. Playlist search and sort keep a separate row list; remove maps the rows back to the server order.
  - Icon font: `assets/fonts/lucide.ttf` is a subset. To add a glyph, get `lucide-static` from npm, add the code point to the `icon` module in `src/theme.rs` and to `LUCIDE-CODEPOINTS.txt`, then run `allsorts subset -t "<glyphs>" lucide-full.ttf assets/fonts/lucide.ttf` (`cargo install --locked allsorts-tools`). `tests/icons.rs` checks the glyphs.
  - Not done: the Browse icon in the search pill (an overlay adds a full window layer in tiny-skia) and home chips (the library has only music).
- Do not enable the iced `tester` feature. It stops the boot tasks of the app.
- Test the running app only on a virtual display: `env -u WAYLAND_DISPLAY dbus-run-session -- xvfb-run -a <command>`. Never open windows on the desktop of the user. wgpu fails on Xvfb (no DRI3), so use `ICED_BACKEND=tiny-skia` there. Xvfb has no window manager: use `xdotool search --name navify windowfocus` before you send keys.
- Build machine: the sandbox has little free memory. Use one cargo job, and do not run a build and a UI test at the same time. Stop all test processes after each run.
- Measurements (release, tiny-skia, Xvfb 1280x800, demo library, home page loaded): RSS 27 MB, idle CPU 0 ticks in 30 s, 6 threads (checked again after the Spotify parts). An empty iced window with the same features uses 16 MB, so RSS cannot go much lower with iced on X11. Scrolling the Songs page (2263 rows, 40 steps) uses 131 ticks; tiny-skia uses most of them. Playback: 10 ticks in 30 s (0.33% of one core). The progress updates one time each second, and four times each second on the lyrics page. Requests: cold start 12, album open 4 (2 data, 2 covers), album reopen 0 in the stale time (`tests/app.rs`). The souvlaki D-Bus thread wakes one time each second (library limit).
- Tray test (local scripts, not in git): `target/dev/tray-test.sh` with `target/dev/tray-host.py`, a fake StatusNotifierWatcher. Run it in headless mutter: `env -u WAYLAND_DISPLAY -u DISPLAY dbus-run-session -- mutter --headless --wayland --virtual-monitor 1280x800 --wayland-display navify-t -- bash target/dev/tray-test.sh`.
- An ALSA null device ends each song at once. For playback tests in real time, use an ALSA `file` plugin that pipes to a reader at 192000 bytes each second.
- Local test server for tests that change data: `tests/` read `NAVIFY_TEST_LOCAL_URL` and related variables. A Navidrome release binary with generated test tracks works well (see Phase A tests). Never change data on the `.env` server; it is the real library of the user.

## Roadmap

### Phase A fixes (do first)

1. Replace the accessor methods (`cover_id`, `gains`, `artists`, `album_artists`) and the dual fields with one normalization pass over the JSON in `rest()` and `nd()`: `participants.artist` → `artists`, `participants.albumartist` → `albumArtists`, `replayGain{..}` → flat `rg*`, `mediaFileId` → `id`, empty `coverArt` → `id`.
2. Use `std::io::Error` with `ErrorKind::PermissionDenied` for a native 401.
3. A missing JSON value gives `T::default()`. A value that does not parse gives an error.
4. Remove the in-memory cover LRU from the API layer. Use a unique temp file name for each cover download. Prune the cover disk cache to 300 MB at startup, oldest first.
5. Send independent requests at the same time (`tokio::try_join!`). Use `topSongsByArtistId` when the server advertises it.
6. Album tracks sort by disc, then track.
7. Subset the icon font to the used glyphs (target < 30 KB) with a cargo-installed tool.

### Phase B: UI (Spotify desktop look)

- Architecture: routes are strings that are also cache keys (`home`, `search:<q>`, `album:<id>`, `artist:<id>`, `playlist:<id>`, `liked`, `albums`, `artists`, `songs`, `genres`, `genre:<name>`, `folders`, `folder:<id>`, `lyrics`, `settings`). Back/forward history. Query cache `HashMap<String, (Instant, Arc<dyn Any + Send + Sync>)>`, stale time 5 min, a pending set to remove duplicate requests, one `fetch(key)` function, and one `Loaded(key, result)` message. Pages of 100 load as the user scrolls. Optimistic overlays for stars and ratings.
- Images: cover sizes 128, 192, 300, and 640 only. Use the smallest size that is not smaller than the displayed size times the zoom. Decode one cover at a time with the `image` crate in a blocking task. Keep `Handle::from_rgba` in an LRU with a 24 MB byte limit. Calculate the dominant color from the same pixels.
- Virtualized lists and grids: fixed 56 px rows, render only the visible rows ± 6. Keep the scroll offset per route.
- Player: queue, index, shuffle order, repeat (0 off, 1 all, 2 one). ReplayGain linear gain = 10^((gain_db + preamp)/20), clamped by 1/peak. Send the next track to the engine for gapless playback. Scrobble with `submission=false` at start and `submission=true` after 50% or 240 s. Queue sync: restore after login, save 5 s after a change and on pause or quit.
- Keyboard: Space, Ctrl/Cmd+Left/Right, Ctrl/Cmd+Up/Down, Ctrl/Cmd+K and L (search), Ctrl/Cmd+S (shuffle), Ctrl/Cmd+R (repeat), Alt+Left/Right (back/forward). Previous restarts the track after 3 s.
- Look: as close to Spotify as possible, classy and restrained. Collect real Spotify reference images first and compare screenshots side by side.
  - Colors: window #000000, panels #121212 with radius 8 and 8 px gaps, elevated #1f1f1f, hover white 10%, selected white 30%, text #ffffff, subdued #b3b3b3, accent #1ed760, menu #282828 (hover #3e3e3e).
  - Typography: Figtree Regular, Medium, Bold, and Black. Huge titles are Black with tight tracking. Single-line text gets an ellipsis.
  - Icons: 16/20/24 px scale. No emoji. No borders where Spotify has none. Static skeletons for loading. Short STE messages for empty and error states.
  - Login page: the Spotify login layout (gradient background, centered #121212 card, 48 px heading, labels above 48 px inputs, green pill button, "Remember password" toggle, saved servers as rows).
  - Main layout: top bar (back/forward, home, search pill, settings, avatar), left "Your Library" panel (chips, Liked Songs first, collapsible), main panel, right panel (Now Playing view or Queue), player bar (72 px).
  - Pages: Home (greeting tiles and shelves), album/playlist/liked (gradient header, action bar, track table, disc separators), artist (header, Popular, Discography, Fans also like, About), search (Browse all genre tiles, then Top result, Songs, Artists, Albums), lyrics (synced highlight, click to seek), library grids with sort pickers, folders, settings, and a context menu.
- Resource budget applies to all effects: no timers, no continuous redraws.

### Phase C: integration and release

- Finish `crates/audio` (symphonia 0.6 + `opus-decoder` + cpal + rubato + stream-download): gapless, crossfade, seek, volume, pause stops the stream. Use the shared tokio runtime of the app.
- souvlaki for MPRIS, SMTC, and macOS Now Playing (libdbus on Linux; HWND on Windows).
- Measure RSS and idle CPU with wgpu and with `ICED_BACKEND=tiny-skia`. Count network requests for cold start, album open, and album reopen (reopen must make 0 requests in the stale time).
- Check that `prek run --all-files`, clippy, and tests pass on all three platforms. Then tag a release.
