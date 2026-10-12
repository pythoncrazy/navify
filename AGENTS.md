# navify

Spotify-lookalike desktop client for Navidrome / any OpenSubsonic server. Rust, iced 0.14, pure-Rust audio. Targets Linux, macOS, Windows. License GPL-3.0-or-later.

## Code style (strict)

- Concise code, no filler, no comments, no doc comments unless a public API is genuinely non-obvious.
- No tiny one-use helper functions; inline them. Prefer fewer, meatier functions.
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
- Playback: gapless, crossfade, ReplayGain (gain computed by app, applied by engine), speed, seek (range requests for raw streams, `timeOffset` for transcoded), optional transcode format/max bitrate, server play-queue sync, scrobble (`submission=false` on start, `true` after 50% or 4 min).
- OS: souvlaki (MPRIS/SMTC/macOS Now Playing), optional Discord Rich Presence. Linux uses libdbus (no zbus) for both souvlaki and keyring.

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
- Hooks: install prek with `cargo install --locked prek`. Make sure `~/.cargo/bin` is on PATH. Then run `prek install` and `prek run --all-files`.
- Live API tests read `.env` (`NAVIDROME_HOST`, `NAVIDROME_USERNAME`, `NAVIDROME_PASSWORD`) and are skipped when absent. Never print or commit credentials.
- Dependencies: only versions published ≥7 days ago.
- Tools: install all developer tools with `cargo install --locked` (not uv, pip, or system packages).
- Linux build deps: `libasound2-dev libdbus-1-dev pkg-config` (Arch: `alsa-lib dbus pkgconf`).
