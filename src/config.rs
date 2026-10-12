use std::{path::PathBuf, sync::OnceLock};

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
    pub speed: f32,
    pub volume: f32,
    pub transcode_format: String,
    pub transcode_max_bitrate: u32,
    pub queue_sync: bool,
    pub scrobble: bool,
    pub fallback_db: f32,
    pub device: String,
    pub zoom: f32,
    pub window: (f32, f32),
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            replaygain: "track".to_string(),
            preamp_db: 0.0,
            crossfade_secs: 0.0,
            speed: 1.0,
            volume: 0.8,
            transcode_format: String::new(),
            transcode_max_bitrate: 0,
            queue_sync: true,
            scrobble: true,
            fallback_db: 0.0,
            device: String::new(),
            zoom: 1.0,
            window: (1280.0, 800.0),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .expect("The system has no configuration directory.")
            .join("navify/config.json")
    }

    pub fn load() -> Self {
        let p = Self::path();
        match std::fs::read_to_string(&p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            r => r
                .map_err(anyhow::Error::from)
                .and_then(|s| Ok(serde_json::from_str(&s)?))
                .unwrap_or_else(|e| {
                    panic!("Cannot read the configuration file {}: {e}", p.display())
                }),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            let mut f = o.open(&tmp)?;
            std::io::Write::write_all(&mut f, serde_json::to_string_pretty(self)?.as_bytes())?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

fn store() -> Result<()> {
    static STORE: OnceLock<Result<(), String>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            (|| -> Result<()> {
                #[cfg(target_os = "macos")]
                let s = apple_native_keyring_store::keychain::Store::new();
                #[cfg(target_os = "windows")]
                let s = windows_native_keyring_store::Store::new();
                #[cfg(all(unix, not(target_os = "macos")))]
                let s = dbus_secret_service_keyring_store::Store::new();
                keyring_core::set_default_store(
                    s.context("No credential store on this platform.")?,
                );
                Ok(())
            })()
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("The credential store is not available: {e}"))
        .copied()
}

fn entry(server_id: &str) -> Result<keyring_core::Entry> {
    store()?;
    keyring_core::Entry::new("navify", server_id).context("Could not access the credential store.")
}

pub fn password(server_id: &str) -> Result<String> {
    entry(server_id)?.get_password().context("No password is stored for this server.")
}

pub fn remember(server_id: &str, password: &str) -> Result<()> {
    entry(server_id)?.set_password(password).context("Could not store the password.")
}

pub fn forget(server_id: &str) -> Result<()> {
    match entry(server_id)?.delete_credential() {
        Err(keyring_core::Error::NoEntry) => Ok(()),
        r => r.context("Could not delete the password."),
    }
}
