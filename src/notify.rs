use std::path::Path;
use std::process::Command;

use crate::config::ProfileMeta;

pub struct Notifier {
    enabled: bool,
}

impl Notifier {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub fn connected(&self, name: &str, meta: Option<&ProfileMeta>) {
        if !self.enabled {
            return;
        }
        send("✅ Connected", &format_profile(name, meta));
    }

    pub fn disconnected(&self, items: &[(&str, Option<&ProfileMeta>)]) {
        if !self.enabled || items.is_empty() {
            return;
        }
        let body = items
            .iter()
            .map(|(n, m)| format_profile(n, *m))
            .collect::<Vec<_>>()
            .join("\n");
        send("⭕ Disconnected", &body);
    }

    pub fn error(&self, body: &str) {
        if !self.enabled {
            return;
        }
        send("🆘 Error", body);
    }
}

fn format_profile(name: &str, meta: Option<&ProfileMeta>) -> String {
    let emoji = meta.and_then(|m| m.emoji.as_deref());
    let label = meta.and_then(|m| m.label.as_deref()).unwrap_or(name);
    let description = meta
        .and_then(|m| m.description.as_deref())
        .filter(|s| !s.is_empty());

    let head = match emoji {
        Some(e) => format!("{} {}", e, label),
        None => label.to_string(),
    };
    match description {
        Some(d) => format!("{}\n{}", head, d),
        None => head,
    }
}

fn send(title: &str, body: &str) {
    let _ = if cfg!(target_os = "macos") {
        send_macos(title, body)
    } else {
        send_linux(title, body)
    };
}

const OSASCRIPT_PATHS: &[&str] = &["/usr/bin/osascript"];
const SUDO_PATHS: &[&str] = &["/usr/bin/sudo"];
const NOTIFY_SEND_PATHS: &[&str] = &[
    "/usr/bin/notify-send",
    "/usr/local/bin/notify-send",
    "/opt/homebrew/bin/notify-send",
];

fn first_existing(candidates: &[&'static str]) -> Option<&'static str> {
    candidates
        .iter()
        .find(|c| Path::new(c).is_file())
        .copied()
}

/// AppleScript treats `\` and `"` specially inside string literals; everything
/// else (including emoji and unicode) is fine.
fn applescript_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn send_macos(title: &str, body: &str) -> Option<()> {
    let osascript = first_existing(OSASCRIPT_PATHS)?;
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        applescript_quote(body),
        applescript_quote(title)
    );

    // When running under sudo, NotificationCenter will not surface a banner
    // for root — re-enter the calling user's UI session.
    let sudo_user = std::env::var("SUDO_USER").ok().filter(|s| !s.is_empty());
    let mut cmd = if let Some(user) = sudo_user {
        let sudo = first_existing(SUDO_PATHS)?;
        let mut c = Command::new(sudo);
        c.arg("-u").arg(user).arg(osascript).arg("-e").arg(script);
        c
    } else {
        let mut c = Command::new(osascript);
        c.arg("-e").arg(script);
        c
    };
    let _ = cmd.status();
    Some(())
}

fn send_linux(title: &str, body: &str) -> Option<()> {
    let bin = first_existing(NOTIFY_SEND_PATHS)?;
    let sudo_user = std::env::var("SUDO_USER").ok().filter(|s| !s.is_empty());
    let mut cmd = if let Some(user) = sudo_user {
        let sudo = first_existing(SUDO_PATHS)?;
        let mut c = Command::new(sudo);
        c.arg("-u").arg(user).arg(bin).arg(title).arg(body);
        c
    } else {
        let mut c = Command::new(bin);
        c.arg(title).arg(body);
        c
    };
    let _ = cmd.status();
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_uses_emoji_and_description() {
        let meta = ProfileMeta {
            label: Some("Work VPN".into()),
            description: Some("Office network".into()),
            emoji: Some("🏢".into()),
        };
        assert_eq!(
            format_profile("work", Some(&meta)),
            "🏢 Work VPN\nOffice network"
        );
    }

    #[test]
    fn body_falls_back_to_id_when_no_meta() {
        assert_eq!(format_profile("ua", None), "ua");
    }

    #[test]
    fn empty_description_is_ignored() {
        let meta = ProfileMeta {
            label: Some("Home".into()),
            description: Some("".into()),
            emoji: Some("🏠".into()),
        };
        assert_eq!(format_profile("home", Some(&meta)), "🏠 Home");
    }
}
