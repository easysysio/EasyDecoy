// =============================================================================
// main.rs — EasyDecoy entry point
//
// Boots EasyDecoy: loads config, starts the event writer, then runs every
// enabled decoy (fake network service) until one of them fails or the process
// is interrupted. Nobody legitimate has a reason to connect to a decoy, so
// every event it records is worth a look.
//
// The only command-line arguments are --help and --version; settings come from
// the config file named by EASYDECOY_CONFIG.
// =============================================================================

mod config;
mod decoy;
mod event;
mod syslog;

use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::decoy::Shared;
use crate::decoy::http::Http;
use crate::decoy::telnet::Telnet;

// Default config path; overridable via the EASYDECOY_CONFIG env var.
const DEFAULT_CONFIG: &str = "config/easydecoy.toml";

// What --help prints. EasyDecoy takes no other arguments: everything is in the
// config file.
const USAGE: &str = "\
Usage: easydecoy [--help | --version]

EasyDecoy runs decoy network services and records every connection to them as
a JSON event. It takes no other arguments: settings live in a TOML config file,
read from the path in the EASYDECOY_CONFIG environment variable (default:
config/easydecoy.toml).

  -h, --help       print this help and exit
  -V, --version    print the version and exit";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Cli {
    Run,
    Help,
    Version,
    Unknown(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// parse_args(args)
// Reads the command line (without the program name). No argument runs the
// decoys; anything unrecognized is reported rather than ignored, so a mistyped
// flag can't quietly start a second instance.
// ─────────────────────────────────────────────────────────────────────────────
fn parse_args(mut args: impl Iterator<Item = String>) -> Cli {
    match args.next().as_deref() {
        None => Cli::Run,
        Some("-h" | "--help") => Cli::Help,
        Some("-V" | "--version") => Cli::Version,
        Some(other) => Cli::Unknown(other.to_string()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// main()
// Process entry point: read config, install logging, start the event writer
// and every enabled decoy, then wait for a failure or Ctrl-C.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<()> {
    match parse_args(std::env::args().skip(1)) {
        Cli::Run => {}
        Cli::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        Cli::Version => {
            println!("easydecoy {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Cli::Unknown(arg) => {
            eprintln!("easydecoy: unknown argument '{arg}'\n\n{USAGE}");
            std::process::exit(2);
        }
    }

    // The config sets the log level, so it is read before the subscriber is
    // installed; anything that goes wrong here is reported by the returned error.
    let config_path =
        std::env::var("EASYDECOY_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let (config, from_file) = Config::load(&config_path)?;

    // Our own log goes to stderr: stdout is reserved for events when no events
    // file is configured. RUST_LOG still wins over the config's log_level.
    // Colour only on a terminal, so journald doesn't store escape codes.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(config.log_level.trim())),
        )
        .init();
    tracing::info!("EasyDecoy v{} starting", env!("CARGO_PKG_VERSION"));
    if from_file {
        tracing::info!("config: {config_path}");
    } else {
        tracing::info!("config: {config_path} not found, running with defaults");
    }

    let syslog = config.syslog.enabled.then(|| syslog::spawn(&config.syslog));
    let events = event::spawn_writer(&config.events_file, syslog).await?;
    tracing::info!(
        "events: {}",
        if config.events_file.trim().is_empty() { "stdout" } else { config.events_file.trim() }
    );
    if config.syslog.enabled {
        let s = &config.syslog;
        tracing::info!(
            "syslog: {} to {} ({}, {})",
            format!("{:?}", s.protocol).to_lowercase(),
            s.server,
            format!("{:?}", s.format).to_lowercase(),
            format!("{:?}", s.facility).to_lowercase(),
        );
    }

    let shared = Shared {
        events,
        slots: Arc::new(Semaphore::new(config.max_connections)),
        idle: Duration::from_secs(config.idle_timeout_secs),
        session: Duration::from_secs(config.session_limit_secs),
    };

    let mut decoys = JoinSet::new();
    if config.telnet.enabled {
        let telnet = Arc::new(Telnet::new(&config.telnet));
        decoys.spawn(decoy::serve(telnet, config.telnet.bind.clone(), shared.clone()));
    }
    if config.http.enabled {
        let http = Arc::new(Http::new(&config.http));
        decoys.spawn(decoy::serve(http, config.http.bind.clone(), shared.clone()));
    }
    if decoys.is_empty() {
        bail!("no decoys are enabled in {config_path}");
    }

    // A decoy only returns if it could not start (usually a port already in
    // use); that stops EasyDecoy rather than leaving it half running.
    tokio::select! {
        Some(finished) = decoys.join_next() => {
            finished.context("a decoy task panicked")??;
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("interrupted, shutting down");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn parses_the_command_line() {
        assert_eq!(parse_args(args(&[])), Cli::Run);
        assert_eq!(parse_args(args(&["--help"])), Cli::Help);
        assert_eq!(parse_args(args(&["-V"])), Cli::Version);
        assert_eq!(parse_args(args(&["--port"])), Cli::Unknown("--port".into()));
    }
}
