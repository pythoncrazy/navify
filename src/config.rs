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
        }
    }
}

fn path() -> PathBuf {
    let dir = dirs::config_dir().expect("The system has no configuration directory.");
    dir.join("navify/config.json")
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
        let tmp = path.with_extension("tmp");
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        std::io::Write::write_all(&mut o.open(&tmp)?, &serde_json::to_vec_pretty(self)?)?;
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

pub fn forget(server_id: &str) -> Result<()> {
    match entry(server_id)?.delete_credential() {
        Err(keyring_core::Error::NoEntry) => Ok(()),
        r => r.context("Could not delete the password."),
    }
}
