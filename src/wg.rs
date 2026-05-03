use anyhow::{Context, Result, anyhow, bail};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::profile::{ResolvedProfile, validate_name};

const RUN_DIR: &str = "/var/run/wireguard";

// wg-quick on macOS is a bash script that shells out to `wg`, `wireguard-go`,
// `bash`, `route`, `networksetup`, etc. via $PATH. Under sudo with `env_reset`
// (the macOS default) and no `env_keep += "PATH"`, the subprocess inherits the
// caller's PATH — minimal under launchd/SwiftBar/cron, missing /opt/homebrew/bin.
// Inject a known-good PATH so wg-quick can find its peers regardless of caller.
fn child_path(bin: &Path) -> OsString {
    let mut parts: Vec<PathBuf> = Vec::new();
    if let Some(parent) = bin.parent() {
        parts.push(parent.to_path_buf());
    }
    for p in [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        parts.push(PathBuf::from(p));
    }
    if let Some(existing) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&existing) {
            parts.push(p);
        }
    }
    let mut seen: HashSet<PathBuf> = HashSet::new();
    parts.retain(|p| seen.insert(p.clone()));
    std::env::join_paths(parts).unwrap_or_default()
}

const WG_QUICK_PATHS: &[&str] = &[
    "/usr/bin/wg-quick",
    "/usr/local/bin/wg-quick",
    "/opt/homebrew/bin/wg-quick",
    "/sbin/wg-quick",
    "/usr/sbin/wg-quick",
];

const WG_PATHS: &[&str] = &[
    "/usr/bin/wg",
    "/usr/local/bin/wg",
    "/opt/homebrew/bin/wg",
    "/sbin/wg",
    "/usr/sbin/wg",
];

fn find_bin(candidates: &[&str], name: &str) -> Result<PathBuf> {
    for c in candidates {
        let p = Path::new(c);
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
    }
    Err(anyhow!(
        "could not locate `{}` in trusted paths ({})",
        name,
        candidates.join(", ")
    ))
}

pub fn wg_quick() -> Result<PathBuf> {
    find_bin(WG_QUICK_PATHS, "wg-quick")
}

pub fn wg() -> Result<PathBuf> {
    find_bin(WG_PATHS, "wg")
}

pub fn up(p: &ResolvedProfile) -> Result<()> {
    run_quick("up", &p.conf_path.display().to_string())
}

pub fn down(p: &ResolvedProfile) -> Result<()> {
    run_quick("down", &p.conf_path.display().to_string())
}

pub fn down_iface(name: &str) -> Result<()> {
    crate::profile::validate_name(name)?;
    run_quick("down", name)
}

fn run_quick(action: &str, target: &str) -> Result<()> {
    let bin = wg_quick()?;
    let status = Command::new(&bin)
        .env("PATH", child_path(&bin))
        .arg(action)
        .arg(target)
        .status()
        .with_context(|| format!("spawn {}", bin.display()))?;
    if !status.success() {
        bail!("wg-quick {} {} failed (exit {})", action, target, status);
    }
    Ok(())
}

/// Reads `/var/run/wireguard/<cfg>.name` files (created by wg-quick on macOS).
/// Returns kernel_iface (e.g. "utun6") -> config_name (e.g. "ua").
/// On Linux this directory typically does not exist; map will be empty.
pub fn name_map() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(rd) = std::fs::read_dir(RUN_DIR) else {
        return out;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.extension().and_then(|s| s.to_str()) != Some("name") {
            continue;
        }
        let Some(cfg_name) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if validate_name(cfg_name).is_err() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&p) else {
            continue;
        };
        let kernel = content.trim();
        if !kernel.is_empty() {
            out.insert(kernel.to_string(), cfg_name.to_string());
        }
    }
    out
}

/// Returns active config names (the names you can pass to `wg-quick down`).
/// On macOS we resolve via `*.name` files in /var/run/wireguard/. On Linux the
/// kernel iface name equals the config name, so `wg show interfaces` is fine.
pub fn active_config_names() -> Result<Vec<String>> {
    let map = name_map();
    if !map.is_empty() {
        let mut names: Vec<String> = map.into_values().collect();
        names.sort();
        names.dedup();
        return Ok(names);
    }
    active_kernel_interfaces()
}

pub fn active_kernel_interfaces() -> Result<Vec<String>> {
    let bin = wg()?;
    let output = Command::new(&bin)
        .env("PATH", child_path(&bin))
        .arg("show")
        .arg("interfaces")
        .output()
        .with_context(|| format!("spawn {}", bin.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let lower = stderr.to_lowercase();
        if lower.contains("permission") || lower.contains("operation not permitted") {
            return Ok(Vec::new());
        }
        return Err(anyhow!("wg show interfaces failed: {}", stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(|s| s.to_string())
        .collect())
}

pub fn show_dump() -> Result<String> {
    let bin = wg()?;
    let output = Command::new(&bin)
        .env("PATH", child_path(&bin))
        .arg("show")
        .arg("all")
        .arg("dump")
        .output()
        .with_context(|| format!("spawn {}", bin.display()))?;
    if !output.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
