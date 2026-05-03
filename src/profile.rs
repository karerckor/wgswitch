use anyhow::{Result, anyhow, bail};
use std::path::{Path, PathBuf};

pub const WG_DIRS: &[&str] = &[
    "/etc/wireguard",
    "/usr/local/etc/wireguard",
    "/opt/homebrew/etc/wireguard",
];

#[derive(Debug, Clone)]
pub struct ResolvedProfile {
    pub conf_path: PathBuf,
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("profile name is empty");
    }
    if name.len() > 15 {
        bail!("profile name '{}' is too long (max 15 chars)", name);
    }
    if name == "." || name == ".." {
        bail!("invalid profile name '{}'", name);
    }
    for c in name.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '_' | '=' | '+' | '.' | '-');
        if !ok {
            bail!("profile name '{}' contains invalid character {:?}", name, c);
        }
    }
    Ok(())
}

pub fn resolve(name: &str) -> Result<ResolvedProfile> {
    validate_name(name)?;
    for dir in WG_DIRS {
        let p = Path::new(dir).join(format!("{}.conf", name));
        if p.is_file() {
            return Ok(ResolvedProfile { conf_path: p });
        }
    }
    Err(anyhow!(
        "profile '{}' not found in any wireguard directory ({})",
        name,
        WG_DIRS.join(", ")
    ))
}

pub fn discover_ids() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dir in WG_DIRS {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if path.extension().and_then(|s| s.to_str()) != Some("conf") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if validate_name(stem).is_err() {
                continue;
            }
            out.push(stem.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical_names() {
        for n in ["work", "home", "vpn-eu", "test_1", "a.b", "x+y=z"] {
            assert!(validate_name(n).is_ok(), "expected ok: {}", n);
        }
    }

    #[test]
    fn rejects_path_traversal() {
        for n in [
            "",
            "..",
            ".",
            "../etc/passwd",
            "a/b",
            "a\\b",
            "a b",
            "a:b",
            "name_that_is_way_too_long",
        ] {
            assert!(validate_name(n).is_err(), "expected err: {:?}", n);
        }
    }

    #[test]
    fn rejects_shell_metacharacters() {
        for n in ["a;b", "a$b", "a`b", "a|b", "a&b", "a*b"] {
            assert!(validate_name(n).is_err(), "expected err: {:?}", n);
        }
    }
}
