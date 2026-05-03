use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::profile::discover_ids;
use crate::wg::{active_config_names, name_map, show_dump};

mod state;

#[derive(Serialize, Debug, Clone, Default)]
pub struct InterfaceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_port: Option<u16>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct PeerInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_ips: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_handshake: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_handshake_ago_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_handshake_ago_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_bytes: Option<u64>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Totals {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_human: String,
    pub tx_human: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_per_sec: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_per_sec: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_per_sec_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_per_sec_human: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ProfileStatus {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<InterfaceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totals: Option<Totals>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<PeerInfo>,
}

#[derive(Serialize, Debug)]
pub struct Snapshot {
    pub current: Option<String>,
    pub active: Vec<String>,
    pub profiles: Vec<ProfileStatus>,
}

impl Snapshot {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn to_text(&self) -> String {
        if self.profiles.is_empty() {
            return "(no profiles)".to_string();
        }
        let mut out = String::new();
        for p in &self.profiles {
            let dot = if p.active { "●" } else { "○" };
            let emoji = p.emoji.as_deref().unwrap_or("");
            let label = p.label.as_deref().unwrap_or(&p.id);
            let sep = if emoji.is_empty() { "" } else { " " };
            out.push_str(&format!("{} {}{}{}  ({})\n", dot, emoji, sep, label, p.id));

            if !p.active {
                continue;
            }
            if let Some(t) = &p.totals {
                out.push_str(&format!(
                    "    total:    ↓ {} / ↑ {}\n",
                    t.rx_human, t.tx_human
                ));
                if let (Some(rxh), Some(txh)) = (&t.rx_per_sec_human, &t.tx_per_sec_human) {
                    out.push_str(&format!("    speed:    ↓ {} / ↑ {}\n", rxh, txh));
                }
            }
            for peer in &p.peers {
                let endpoint = peer.endpoint.as_deref().unwrap_or("(none)");
                let hs = peer.last_handshake_ago_human.as_deref().unwrap_or("never");
                out.push_str(&format!(
                    "    peer:     {}  •  handshake: {}\n",
                    endpoint, hs
                ));
            }
        }
        out.trim_end().to_string()
    }
}

pub fn collect(cfg: &Config) -> Result<Snapshot> {
    let active = active_config_names().unwrap_or_default();
    let kernel_to_cfg = name_map();

    let mut ids: Vec<String> = discover_ids();
    for a in &active {
        if !ids.contains(a) {
            ids.push(a.clone());
        }
    }
    ids.sort();
    ids.dedup();

    let dump = show_dump().unwrap_or_default();
    let parsed = parse_dump(&dump);
    let parsed = translate_keys(parsed, &kernel_to_cfg);

    let now_ms = unix_ms_now();
    let now_secs = now_ms / 1000;
    let prev = state::read();
    let mut current_samples: HashMap<String, state::Sample> = HashMap::new();

    let mut profiles = Vec::with_capacity(ids.len());
    for id in &ids {
        let meta = cfg.profiles.get(id).cloned().unwrap_or_default();
        let is_active = active.contains(id);

        let (interface, peers_raw) = parsed.get(id).cloned().unwrap_or_default();
        let interface = if is_active && interface.is_some() {
            interface
        } else {
            None
        };

        let (peers, totals) = if is_active {
            let totals_raw: (u64, u64) = peers_raw.iter().fold((0u64, 0u64), |(rx, tx), p| {
                (rx + p.rx_bytes.unwrap_or(0), tx + p.tx_bytes.unwrap_or(0))
            });
            current_samples.insert(
                id.clone(),
                state::Sample {
                    rx: totals_raw.0,
                    tx: totals_raw.1,
                },
            );
            let totals = build_totals(id, totals_raw, now_ms, prev.as_ref());
            let peers = peers_raw
                .into_iter()
                .map(|p| enrich_peer(p, now_secs))
                .collect();
            (peers, Some(totals))
        } else {
            (Vec::new(), None)
        };

        profiles.push(ProfileStatus {
            id: id.clone(),
            label: meta.label,
            description: meta.description,
            emoji: meta.emoji,
            active: is_active,
            interface,
            totals,
            peers,
        });
    }

    state::write(&state::State {
        timestamp_ms: now_ms,
        samples: current_samples,
    });

    let current = match active.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    };

    Ok(Snapshot {
        current,
        active,
        profiles,
    })
}

fn enrich_peer(mut p: PeerInfo, now_secs: u64) -> PeerInfo {
    if let Some(hs) = p.last_handshake {
        if hs > 0 && now_secs >= hs {
            let ago = now_secs - hs;
            p.last_handshake_ago_seconds = Some(ago);
            p.last_handshake_ago_human = Some(relative_seconds(ago));
        }
    }
    p
}

fn build_totals(
    id: &str,
    totals_raw: (u64, u64),
    now_ms: u64,
    prev: Option<&state::State>,
) -> Totals {
    let (rx, tx) = totals_raw;
    let mut totals = Totals {
        rx_bytes: rx,
        tx_bytes: tx,
        rx_human: bytes_human(rx),
        tx_human: bytes_human(tx),
        ..Default::default()
    };
    if let Some(prev) = prev {
        if let Some(sample) = prev.samples.get(id) {
            let delta_ms = now_ms.saturating_sub(prev.timestamp_ms);
            // Useful window: 50ms..=10min. Outside that we treat the sample as
            // either too noisy or too stale to derive a meaningful rate.
            if (50..=600_000).contains(&delta_ms) && rx >= sample.rx && tx >= sample.tx {
                let secs = delta_ms as f64 / 1000.0;
                let rxr = ((rx - sample.rx) as f64 / secs).round() as u64;
                let txr = ((tx - sample.tx) as f64 / secs).round() as u64;
                totals.rx_per_sec = Some(rxr);
                totals.tx_per_sec = Some(txr);
                totals.rx_per_sec_human = Some(format!("{}/s", bytes_human(rxr)));
                totals.tx_per_sec_human = Some(format!("{}/s", bytes_human(txr)));
            }
        }
    }
    totals
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn bytes_human(n: u64) -> String {
    const K: f64 = 1024.0;
    let n = n as f64;
    if n < K {
        return format!("{} B", n as u64);
    }
    if n < K * K {
        return format!("{:.2} KiB", n / K);
    }
    if n < K * K * K {
        return format!("{:.2} MiB", n / K / K);
    }
    if n < K * K * K * K {
        return format!("{:.2} GiB", n / K / K / K);
    }
    format!("{:.2} TiB", n / K / K / K / K)
}

pub fn relative_seconds(secs: u64) -> String {
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

type ParsedIface = (Option<InterfaceInfo>, Vec<PeerInfo>);

fn translate_keys(
    raw: HashMap<String, ParsedIface>,
    kernel_to_cfg: &HashMap<String, String>,
) -> HashMap<String, ParsedIface> {
    let mut out: HashMap<String, ParsedIface> = HashMap::new();
    for (iface, payload) in raw {
        let cfg_name = kernel_to_cfg.get(&iface).cloned().unwrap_or(iface);
        let entry = out.entry(cfg_name).or_default();
        if entry.0.is_none() {
            entry.0 = payload.0;
        }
        entry.1.extend(payload.1);
    }
    out
}

/// Parses `wg show all dump`. Per-line format:
///   interface (5 cols): iface  privkey  pubkey  listen-port  fwmark
///   peer (9 cols):      iface  pubkey   psk     endpoint     allowed-ips  handshake  rx  tx  keepalive
fn parse_dump(text: &str) -> HashMap<String, ParsedIface> {
    let mut out: HashMap<String, ParsedIface> = HashMap::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols.len() {
            5 => {
                let iface = cols[0].to_string();
                let pubkey = (cols[2] != "(none)").then(|| cols[2].to_string());
                let port = cols[3].parse::<u16>().ok().filter(|&p| p != 0);
                let entry = out.entry(iface).or_default();
                entry.0 = Some(InterfaceInfo {
                    public_key: pubkey,
                    listen_port: port,
                });
            }
            9 => {
                let iface = cols[0].to_string();
                let peer_pub = (cols[1] != "(none)").then(|| cols[1].to_string());
                let endpoint = (cols[3] != "(none)").then(|| cols[3].to_string());
                let allowed_ips = (cols[4] != "(none)").then(|| cols[4].to_string());
                let handshake = cols[5].parse::<u64>().ok().filter(|&v| v != 0);
                let rx = cols[6].parse::<u64>().ok();
                let tx = cols[7].parse::<u64>().ok();
                let entry = out.entry(iface).or_default();
                entry.1.push(PeerInfo {
                    public_key: peer_pub,
                    endpoint,
                    allowed_ips,
                    last_handshake: handshake,
                    last_handshake_ago_seconds: None,
                    last_handshake_ago_human: None,
                    rx_bytes: rx,
                    tx_bytes: tx,
                });
            }
            _ => continue,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_interface_and_peer_lines() {
        let dump = "\
work\tprivkey\tpubkey\t51820\toff
work\tpeerpub\t(none)\t1.2.3.4:51820\t10.0.0.0/24\t1714750000\t12345\t6789\toff
home\tprivkey2\tpubkey2\t51821\toff
home\tpeerpub2\t(none)\t(none)\t10.1.0.0/24\t0\t0\t0\toff";
        let map = parse_dump(dump);
        let work = &map["work"];
        let work_iface = work.0.as_ref().unwrap();
        assert_eq!(work_iface.public_key.as_deref(), Some("pubkey"));
        assert_eq!(work_iface.listen_port, Some(51820));
        assert_eq!(work.1.len(), 1);
        assert_eq!(work.1[0].public_key.as_deref(), Some("peerpub"));
        assert_eq!(work.1[0].endpoint.as_deref(), Some("1.2.3.4:51820"));
        assert_eq!(work.1[0].last_handshake, Some(1714750000));

        let home = &map["home"];
        assert!(home.1[0].endpoint.is_none(), "(none) endpoint -> null");
        assert!(home.1[0].last_handshake.is_none(), "0 handshake -> null");
    }

    #[test]
    fn bytes_human_formatting() {
        assert_eq!(bytes_human(0), "0 B");
        assert_eq!(bytes_human(512), "512 B");
        assert_eq!(bytes_human(1536), "1.50 KiB");
        assert_eq!(bytes_human(1024 * 1024 * 3), "3.00 MiB");
    }

    #[test]
    fn relative_seconds_ranges() {
        assert_eq!(relative_seconds(5), "5s ago");
        assert_eq!(relative_seconds(120), "2m ago");
        assert_eq!(relative_seconds(7200), "2h ago");
        assert_eq!(relative_seconds(2 * 86400 + 5), "2d ago");
    }

    #[test]
    fn json_drops_inactive_extras() {
        let snap = Snapshot {
            current: Some("work".into()),
            active: vec!["work".into()],
            profiles: vec![ProfileStatus {
                id: "home".into(),
                label: Some("Home".into()),
                description: None,
                emoji: Some("🏠".into()),
                active: false,
                interface: None,
                totals: None,
                peers: Vec::new(),
            }],
        };
        let s = snap.to_json().unwrap();
        assert!(!s.contains("\"peers\""));
        assert!(!s.contains("\"interface\""));
        assert!(!s.contains("\"totals\""));
        assert!(s.contains("\"current\":\"work\""));
    }
}
