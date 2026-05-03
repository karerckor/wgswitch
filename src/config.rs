use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ProfileMeta {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub profiles: HashMap<String, ProfileMeta>,
}

const SYSTEM_DEFAULTS: &[&str] = &["/etc/wgswitch.json", "/usr/local/etc/wgswitch.json"];

impl Config {
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = match explicit {
            Some(p) => Some(p.to_path_buf()),
            None => default_path(),
        };
        let Some(path) = path else {
            return Ok(Config::default());
        };
        if !path.exists() {
            if explicit.is_some() {
                bail!("config file does not exist: {}", path.display());
            }
            return Ok(Config::default());
        }
        verify_perms(&path)?;
        let raw =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let cfg: Config =
            serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        Ok(cfg)
    }
}

fn default_path() -> Option<PathBuf> {
    for c in SYSTEM_DEFAULTS {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(b) = directories::BaseDirs::new() {
        let p = b.home_dir().join(".wgswitch.json");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Mirrors wg-quick's hardening: reject world-writable files and files owned
/// by an unexpected user. Under sudoers wgswitch typically runs as euid=0 — in
/// that case only root or the original (calling) uid is acceptable.
fn verify_perms(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let mode = meta.mode();
    if mode & 0o002 != 0 {
        bail!(
            "config {} is world-writable (mode {:o}); refusing to load",
            path.display(),
            mode & 0o7777
        );
    }
    let owner = meta.uid();
    let euid = unsafe { libc::geteuid() };
    let allowed_uid = match std::env::var("SUDO_UID")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        Some(u) => u,
        None => euid,
    };
    if owner != 0 && owner != euid && owner != allowed_uid {
        bail!(
            "config {} is owned by uid {} (expected root, {} or {})",
            path.display(),
            owner,
            euid,
            allowed_uid
        );
    }
    Ok(())
}
