use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::api::Server;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub servers: Vec<Server>,
    pub active: Option<String>,
    pub settings: Settings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub replaygain: String,
    pub preamp_db: f32,
    pub crossfade_secs: f32,
    pub volume: f32,
    pub transcode_format: String,
    pub transcode_max_bitrate: u32,
    pub queue_sync: bool,
    pub scrobble: bool,
    pub fallback_db: f32,
    pub device: String,
    pub zoom: f32,
    pub window: (f32, f32),
    pub remaining: bool,
    pub disc_grid: bool,
    pub system_frame: bool,
    pub tray: bool,
    pub panels: (f32, f32),
    pub lib_collapsed: bool,
    pub banners: bool,
    pub lib_sort: u8,
    pub scrobble_pct: f32,
    pub seek_secs: f32,
    pub rg_clip: bool,
    pub lyrics_offset: f32,
    pub lyrics_size: f32,
    pub compact: bool,
    pub scrobble_by_time: bool,
    pub scrobble_secs: f32,
    pub lyrics_follow: bool,
    pub play_mode: u8,
    pub ratings: bool,
    pub start_min: bool,
    pub min_tray: bool,
    pub queue_skip: String,
    pub confirm_queue: bool,
    pub keys: std::collections::BTreeMap<String, String>,
    pub notify: bool,
    pub awake: bool,
    pub vol_step: f32,
    pub lyrics_align: u8,
    pub smooth: bool,
}

impl Settings {
    pub fn check(&self) -> Result<(), &'static str> {
        let within = |v: f32, lo: f32, hi: f32| (lo..=hi).contains(&v);
        let ok = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5].contains(&self.zoom)
            && ["off", "track", "album"].contains(&self.replaygain.as_str())
            && self.lib_sort < 3
            && self.play_mode < 4
            && self.lyrics_align < 3
            && within(self.volume, 0.0, 1.0)
            && within(self.crossfade_secs, 0.0, 12.0)
            && within(self.preamp_db, -15.0, 15.0)
            && within(self.fallback_db, -15.0, 15.0)
            && within(self.seek_secs, 1.0, 30.0)
            && within(self.vol_step, 1.0, 25.0)
            && within(self.lyrics_offset, -5.0, 5.0)
            && within(self.lyrics_size, 20.0, 48.0)
            && within(self.scrobble_pct, 10.0, 100.0)
            && within(self.scrobble_secs, 10.0, 600.0)
            && within(self.panels.0, 280.0, 480.0)
            && within(self.panels.1, 280.0, 420.0);
        ok.then_some(())
            .ok_or("A value in the settings is out of range.")
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            replaygain: "track".to_string(),
            preamp_db: 0.0,
            crossfade_secs: 0.0,
            volume: 0.8,
            transcode_format: String::new(),
            transcode_max_bitrate: 0,
            queue_sync: true,
            scrobble: true,
            fallback_db: 0.0,
            device: String::new(),
            zoom: 1.0,
            window: (1280.0, 800.0),
            remaining: false,
            disc_grid: false,
            system_frame: true,
            tray: false,
            panels: (300.0, 320.0),
            lib_collapsed: false,
            banners: false,
            lib_sort: 0,
            scrobble_pct: 50.0,
            seek_secs: 5.0,
            rg_clip: true,
            lyrics_offset: 0.0,
            lyrics_size: 32.0,
            compact: false,
            scrobble_by_time: false,
            scrobble_secs: 240.0,
            lyrics_follow: true,
            smooth: true,
            play_mode: 0,
            ratings: false,
            start_min: false,
            min_tray: false,
            queue_skip: String::new(),
            confirm_queue: false,
            keys: Default::default(),
            notify: false,
            awake: false,
            vol_step: 10.0,
            lyrics_align: 0,
        }
    }
}

pub fn dir(kind: &str) -> PathBuf {
    let env = |k: &str| {
        std::env::var_os(k)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let home = || std::env::home_dir().expect("The system has no home directory.");
    let (xdg, unix, mac, win) = match kind {
        "config" => (
            "XDG_CONFIG_HOME",
            ".config",
            "Library/Application Support",
            "APPDATA",
        ),
        "cache" => ("XDG_CACHE_HOME", ".cache", "Library/Caches", "LOCALAPPDATA"),
        _ => (
            "XDG_DATA_HOME",
            ".local/share",
            "Library/Application Support",
            "APPDATA",
        ),
    };
    match () {
        _ if cfg!(windows) => env(win).expect("The system has no application data directory."),
        _ if cfg!(target_os = "macos") => home().join(mac),
        _ => env(xdg).unwrap_or_else(|| home().join(unix)),
    }
}

fn path() -> PathBuf {
    dir("config").join("navify/config.json")
}

impl Config {
    pub fn load() -> Self {
        let p = path();
        let s = match std::fs::read_to_string(&p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            r => r.map_err(anyhow::Error::from),
        };
        s.and_then(|s| Ok(serde_json::from_str(&s)?))
            .unwrap_or_else(|e| panic!("Cannot read the configuration file {}: {e}", p.display()))
    }

    pub fn save(&self) -> Result<()> {
        let (path, mut o) = (path(), std::fs::OpenOptions::new());
        std::fs::create_dir_all(path.parent().expect("the path has a directory"))?;
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        std::io::Write::write_all(&mut f, &serde_json::to_vec_pretty(self)?)?;
        f.sync_all()?;
        Ok(std::fs::rename(&tmp, &path)?)
    }
}

fn entry(server_id: &str) -> Result<keyring_core::Entry> {
    #[cfg(target_os = "macos")]
    let s = apple_native_keyring_store::keychain::Store::new();
    #[cfg(target_os = "windows")]
    let s = windows_native_keyring_store::Store::new();
    #[cfg(all(unix, not(target_os = "macos")))]
    let s = dbus_secret_service_keyring_store::Store::new();
    keyring_core::set_default_store(s.context("The credential store is not available.")?);
    keyring_core::Entry::new("navify", server_id).context("Could not access the credential store.")
}

pub fn remember(server_id: &str, password: &str) -> Result<()> {
    entry(server_id)?
        .set_password(password)
        .context("Could not store the password.")
}

pub fn recall(server_id: &str) -> Result<Option<String>> {
    match entry(server_id)?.get_password() {
        Err(keyring_core::Error::NoEntry) => Ok(None),
        r => r.map(Some).context("Could not read the password."),
    }
}

pub fn forget(server_id: &str) -> Result<()> {
    match entry(server_id)?.delete_credential() {
        Err(keyring_core::Error::NoEntry) => Ok(()),
        r => r.context("Could not delete the password."),
    }
}
