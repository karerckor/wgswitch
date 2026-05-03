use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const STATE_DIR: &str = "/var/run/wgswitch";
const STATE_FILE: &str = "state.json";

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Sample {
    pub rx: u64,
    pub tx: u64,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct State {
    pub timestamp_ms: u64,
    pub samples: HashMap<String, Sample>,
}

fn state_path() -> Option<PathBuf> {
    let dir = Path::new(STATE_DIR);
    if !dir.is_dir() {
        // Best effort: create with restrictive perms. If we cannot (non-root),
        // skip rate computation entirely.
        if std::fs::create_dir_all(dir).is_err() {
            return None;
        }
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Some(dir.join(STATE_FILE))
}

pub fn read() -> Option<State> {
    let path = state_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn write(state: &State) {
    let Some(path) = state_path() else {
        return;
    };
    let Ok(json) = serde_json::to_string(state) else {
        return;
    };
    // Atomic-ish write: temp file then rename within the same dir.
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_err() {
        return;
    }
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    let _ = std::fs::rename(&tmp, &path);
}
