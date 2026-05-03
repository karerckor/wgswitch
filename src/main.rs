use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;

mod config;
mod notify;
mod profile;
mod status;
mod wg;

use config::Config;
use notify::Notifier;

#[derive(Parser)]
#[command(
    name = "wgswitch",
    version,
    about = "Thin wrapper around wg-quick for quick VPN switching"
)]
struct Cli {
    /// Path to wgswitch metadata config (default: /etc/wgswitch.json or ~/.wgswitch.json).
    /// Only display metadata is read from here; binary paths and conf paths cannot be overridden.
    #[arg(long, short, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Send a desktop notification on connect/off success or failure.
    #[arg(long, global = true)]
    notify: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List configured wireguard profiles discovered in /etc/wireguard/
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Bring up a profile (requires sudo, like wg-quick)
    Connect {
        /// Profile id, e.g. `work` (resolves to /etc/wireguard/work.conf)
        profile: String,
    },
    /// Bring down a profile, or all active interfaces if none given (requires sudo)
    Off { profile: Option<String> },
    /// Show current connection state
    Status {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(ValueEnum, Clone, Debug)]
enum OutputFormat {
    Text,
    Json,
}

enum ConnectOutcome {
    Connected,
    AlreadyActive,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let notifier = Notifier::new(cli.notify);
    match run(cli, &notifier) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wgswitch: {:#}", e);
            // Top-level message only (no full anyhow chain) so the banner stays terse.
            notifier.error(&format!("{}", e));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli, notifier: &Notifier) -> Result<()> {
    let cfg = Config::load(cli.config.as_deref())?;

    match cli.command {
        Commands::List { format } => cmd_list(&cfg, format),
        Commands::Connect { profile } => match cmd_connect(&profile)? {
            ConnectOutcome::Connected => {
                notifier.connected(&profile, cfg.profiles.get(&profile));
                Ok(())
            }
            ConnectOutcome::AlreadyActive => Ok(()),
        },
        Commands::Off { profile } => match profile {
            Some(name) => {
                let resolved = profile::resolve(&name)?;
                wg::down(&resolved)?;
                notifier.disconnected(&[(name.as_str(), cfg.profiles.get(&name))]);
                Ok(())
            }
            None => {
                let downed = cmd_off_all()?;
                let items: Vec<_> = downed
                    .iter()
                    .map(|n| (n.as_str(), cfg.profiles.get(n)))
                    .collect();
                notifier.disconnected(&items);
                Ok(())
            }
        },
        Commands::Status { format } => {
            let snap = status::collect(&cfg)?;
            match format {
                OutputFormat::Json => println!("{}", snap.to_json()?),
                OutputFormat::Text => println!("{}", snap.to_text()),
            }
            Ok(())
        }
    }
}

fn cmd_list(cfg: &Config, format: OutputFormat) -> Result<()> {
    let ids = profile::discover_ids();
    match format {
        OutputFormat::Text => {
            if ids.is_empty() {
                println!("(no profiles found in {})", profile::WG_DIRS.join(", "));
                return Ok(());
            }
            for id in &ids {
                let meta = cfg.profiles.get(id);
                let label = meta.and_then(|m| m.label.as_deref()).unwrap_or("");
                let emoji = meta.and_then(|m| m.emoji.as_deref()).unwrap_or("");
                let sep = if emoji.is_empty() { "" } else { " " };
                println!("{:<16} {}{}{}", id, emoji, sep, label);
            }
        }
        OutputFormat::Json => {
            let items: Vec<_> = ids
                .iter()
                .map(|id| {
                    let meta = cfg.profiles.get(id).cloned().unwrap_or_default();
                    serde_json::json!({
                        "id": id,
                        "label": meta.label,
                        "description": meta.description,
                        "emoji": meta.emoji,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string(&items)?);
        }
    }
    Ok(())
}

fn cmd_connect(target: &str) -> Result<ConnectOutcome> {
    let resolved = profile::resolve(target)?;
    let active = wg::active_config_names().unwrap_or_default();
    let target_already_active = active.iter().any(|a| a == target);

    for name in &active {
        if name == target {
            continue;
        }
        if let Err(e) = wg::down_iface(name) {
            eprintln!("wgswitch: failed to down {}: {:#}", name, e);
        }
    }

    if target_already_active {
        eprintln!("wgswitch: {} is already active", target);
        return Ok(ConnectOutcome::AlreadyActive);
    }
    wg::up(&resolved)?;
    Ok(ConnectOutcome::Connected)
}

fn cmd_off_all() -> Result<Vec<String>> {
    let active = wg::active_config_names()?;
    if active.is_empty() {
        eprintln!("wgswitch: no active wireguard interfaces");
        return Ok(Vec::new());
    }
    let mut downed = Vec::new();
    let mut last_err = None;
    for name in &active {
        match wg::down_iface(name) {
            Ok(()) => downed.push(name.clone()),
            Err(e) => {
                eprintln!("wgswitch: failed to down {}: {:#}", name, e);
                last_err = Some(e);
            }
        }
    }
    match last_err {
        Some(e) if downed.is_empty() => Err(e),
        _ => Ok(downed),
    }
}
