# navify

A desktop client for Navidrome and OpenSubsonic servers. It looks like Spotify.
It uses few resources. It runs on Linux, macOS, and Windows.

## Features

- Play your music with gapless playback, crossfade, and ReplayGain.
- Browse albums, artists, playlists, and folders.
- Search, lyrics, artist radio, and liked songs.
- Sync the play queue with the server.
- Control playback with media keys and the system media controls.
- Keep Navify in the system tray when you close the window (set this in the settings).
  Ctrl+W always sends the window to the tray. Ctrl+Q quits.

## Install

Download a file for your system from the
[releases page](https://github.com/pythoncrazy/navify/releases).

| System | File |
| --- | --- |
| Debian, Ubuntu | `navify_<version>_amd64.deb` or `_arm64.deb` |
| Arch Linux | `PKGBUILD-<target>` with the matching `.tar.gz` |
| Other Linux | `.AppImage`, or the portable `navify-v<version>-<target>.tar.gz` |
| Windows | `navify_<version>_x64-setup.exe` |
| macOS | `Navify_<version>_aarch64.dmg` (Apple silicon) or `_x64.dmg` (Intel) |

`SHA256SUMS` contains the checksums of all files.

### Linux

Install the `.deb` with `sudo apt install ./navify_<version>_amd64.deb`.
For Arch Linux, put the `PKGBUILD` and the `.tar.gz` in one directory and run `makepkg -si`.

The portable archive contains only the program. Extract it to a directory on your `PATH`.
Then run `navify --install` one time. This adds the menu entry and the icons
to `~/.local/share`.

The program needs ALSA and D-Bus. Navify uses Wayland or X11.
On desktops that do not draw title bars for Wayland windows, such as GNOME,
Navify uses X11 to get the system title bar. You can change this in the settings.

The tray icon needs a StatusNotifierItem host. KDE, XFCE, Cinnamon, and waybar have one.
On GNOME, install the AppIndicator extension.

### Windows

Run the setup file. Windows SmartScreen can show a warning because the file has no signature.
Click "More info", then "Run anyway".

### macOS

Open the `.dmg` and move Navify to Applications. The app has no signature.
Before the first start, run this command:

```sh
xattr -dr com.apple.quarantine /Applications/Navify.app
```

## Build

Install Rust 1.90 or newer. On Linux, install these packages first:

```sh
sudo apt-get install libasound2-dev libdbus-1-dev pkg-config
```

Then build and run:

```sh
cargo run --release
```

The release profile uses full LTO, so a release build takes some minutes.
Use `cargo run` for fast development builds.

To make packages, install cargo-packager and run it after the release build:

```sh
cargo install --locked cargo-packager
cargo packager --release -f deb,pacman -o dist
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

Some tests use a Navidrome server. They have the `#[ignore]` attribute.
Run them with `cargo test --workspace -- --include-ignored`.
The live tests read `NAVIDROME_HOST`, `NAVIDROME_USERNAME`, and `NAVIDROME_PASSWORD` from `.env`.
The tests that change data read `NAVIFY_TEST_LOCAL_URL`, `NAVIFY_TEST_LOCAL_USERNAME`,
and `NAVIFY_TEST_LOCAL_PASSWORD`. A test fails when a value is missing.

`cargo run --example logo` makes the logo files in `assets/`.

## Release

1. Set the new version in `Cargo.toml`.
2. Push a tag with the same version, for example `v0.2.0`.
3. The release workflow builds all packages and makes a draft release.
4. Examine the draft and publish it.

## License

GPL-3.0-or-later. See LICENSE.

Bundled fonts: Figtree under the SIL OFL 1.1 (assets/fonts/OFL.txt),
Lucide under the ISC license (assets/fonts/LUCIDE-LICENSE.txt),
Phosphor under the MIT license (assets/fonts/PHOSPHOR-LICENSE.txt).
