// =============================================================================
// main.rs — EasyDecoy entry point
//
// Boots EasyDecoy: reads the config file (log level, database, GUI address),
// opens the database, starts the event writer and every enabled decoy (fake
// network service), and serves the management GUI, until it is told to stop.
// Nobody legitimate has a reason to connect to a decoy, so every event it
// records is worth a look.
//
// The decoys and everything else are set in the GUI and kept in the database.
// A 0.1.0 config file's settings become the database's on its first start.
//
// The only command-line arguments are --help and --version.
// =============================================================================

mod config;
mod db;
mod decoy;
mod event;
mod settings;
mod store;
mod supervisor;
mod syslog;
mod web;

use std::io::IsTerminal;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::settings::{Settings, SettingsStore};

// Default config path; overridable via the EASYDECOY_CONFIG env var.
const DEFAULT_CONFIG: &str = "config/easydecoy.toml";

// How long events still queued at shutdown get to reach the database.
const DRAIN: Duration = Duration::from_secs(5);

// Where the packaged service puts the decoys added after 0.1.0.
const PACKAGED_FTP_BIND: &str = "0.0.0.0:21";
const PACKAGED_SSH_BIND: &str = "0.0.0.0:22";

// What --help prints.
const USAGE: &str = "\
Usage: easydecoy [--help | --version]

EasyDecoy runs decoy network services and records every connection to them.
Its decoys, events and settings are managed in a web GUI (by default
https://127.0.0.1:9443/) and kept in a SQLite database. A small TOML file,
read from the path in the EASYDECOY_CONFIG environment variable (default:
config/easydecoy.toml), says where the database is and where the GUI listens.

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
    let config_path = std::env::var("EASYDECOY_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let (config, from_file) = Config::load(&config_path)?;

    // Our own log goes to stderr: stdout is reserved for events when the
    // events file is "-". RUST_LOG still wins over the config's log_level.
    // Colour only on a terminal, so journald doesn't store escape codes.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(config.log_level.trim())))
        .init();
    tracing::info!("EasyDecoy v{} starting", env!("CARGO_PKG_VERSION"));
    if from_file {
        tracing::info!("config: {config_path}");
    } else {
        tracing::info!("config: {config_path} not found, running with defaults");
    }

    // rustls needs a process-wide crypto provider chosen explicitly (see the
    // axum-server line in Cargo.toml).
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("installing the TLS crypto provider"))?;

    let gui = config.gui_addr()?;
    // Bound first: if the port is taken, nothing else has started yet.
    let listener = web::bind(gui)?;

    let db_file = config.db_file();
    let db = db::open(&db_file).await?;
    tracing::info!("database: {db_file}");

    let (settings, empty) = SettingsStore::load(db.clone()).await?;
    first_start(&settings, &config, empty, gui, &config_path).await?;

    let removed = store::prune(&db, settings.current().recording.retention_days).await.unwrap_or_else(|e| {
        tracing::warn!("retention: {e:#}");
        0
    });
    if removed > 0 {
        tracing::info!("retention: deleted {removed} old event(s)");
    }

    let (events, writer) = event::spawn_writer(db.clone(), settings.clone()).await;
    let host_key = decoy::ssh::host_key(&db).await?;
    let ssh_fingerprint = decoy::ssh::fingerprint(&host_key);
    let (status, decoys) = supervisor::spawn(settings.clone(), events, host_key);
    let retention = tokio::spawn(store::serve_retention(db.clone(), settings.clone()));

    let pem = web::tls::load(&db, &config, gui).await?;
    let state = web::AppState::new(db.clone(), settings.clone(), status, gui, db_file, ssh_fingerprint)?;
    if let Some(warning) = web::gui_warning(gui, &settings.current()) {
        tracing::warn!("{warning}");
    }
    tracing::info!("GUI: https://{gui}/ (certificate: {})", pem.source);
    let mut gui_task = tokio::spawn(web::serve(state, listener, pem.cert, pem.key));

    let result = tokio::select! {
        finished = &mut gui_task => match finished {
            Ok(result) => result,
            Err(e) => Err(anyhow::anyhow!("the GUI task stopped: {e}")),
        },
        reason = stop_signal() => {
            tracing::info!("stopping on {reason}");
            Ok(())
        }
    };

    // Decoys first, so nothing new arrives; then the writer drains what is
    // queued (the decoys' senders are gone, so it ends) and the database closes.
    gui_task.abort();
    decoys.abort();
    retention.abort();
    let _ = decoys.await;
    if tokio::time::timeout(DRAIN, writer).await.is_err() {
        tracing::warn!("some queued events were not stored before shutdown");
    }
    db::close(&db).await;
    result
}

// ─────────────────────────────────────────────────────────────────────────────
// first_start(settings, config, empty, gui, path)
// On an empty database, stores the first settings: a 0.1.0 config file's, or
// the defaults. After that, any 0.1.0 settings left in the file are ignored,
// and the log says so once per start.
//
// As the packaged service, the decoys take the real ports. That includes a
// decoy newer than the settings being upgraded: FTP after a 0.1.0 import, SSH
// on a 0.2.0 database. Those stay off, but turning one on puts it on the port
// the docs and its card say, not the source run's.
// ─────────────────────────────────────────────────────────────────────────────
async fn first_start(
    settings: &SettingsStore,
    config: &Config,
    empty: bool,
    gui: std::net::SocketAddr,
    path: &str,
) -> Result<()> {
    // systemd gives the packaged unit a state directory.
    let packaged = std::env::var_os("STATE_DIRECTORY").is_some();
    let legacy = config.legacy();
    if !empty {
        if legacy.is_some() {
            tracing::warn!(
                "{path} still has 0.1.0 settings (telnet, http, syslog, ...); they were imported on the \
                 first start and are now ignored. Change them in the GUI, and remove them from the file."
            );
        }
        // 0.2.0 had no SSH decoy, so its database has no ssh row.
        if packaged && !settings.has_section("ssh").await? {
            let mut upgraded = (*settings.current()).clone();
            upgraded.ssh.bind = PACKAGED_SSH_BIND.to_string();
            tracing::info!("upgrade: the new SSH decoy is set up on {PACKAGED_SSH_BIND}, off until it is turned on");
            settings.save(upgraded).await.context("storing the SSH decoy's settings")?;
        }
        return Ok(());
    }
    let first = match legacy {
        Some(mut imported) => {
            match imported.check(gui) {
                Ok(()) => tracing::info!("first start: imported the decoy, syslog and limit settings from {path}"),
                // Stored anyway: the GUI is where they get fixed, and a decoy
                // that can't start says why there.
                Err(problem) => tracing::warn!("first start: imported the settings from {path}, but: {problem}"),
            }
            // 0.1.0 had neither; both stay off (see Config::legacy).
            if packaged {
                imported.ftp.bind = PACKAGED_FTP_BIND.to_string();
                imported.ssh.bind = PACKAGED_SSH_BIND.to_string();
            }
            imported
        }
        None => {
            let mut defaults = Settings::default();
            // As 0.1.0's package did. A port that is taken shows as not
            // running in the GUI.
            if packaged {
                defaults.telnet.bind = "0.0.0.0:23".to_string();
                defaults.http.bind = "0.0.0.0:80".to_string();
                defaults.ftp.bind = PACKAGED_FTP_BIND.to_string();
                // Off until it is turned on: the host's own sshd has port 22.
                defaults.ssh.bind = PACKAGED_SSH_BIND.to_string();
            }
            tracing::info!(
                "first start: default settings (Telnet on {}, HTTP on {}, FTP on {})",
                defaults.telnet.bind,
                defaults.http.bind,
                defaults.ftp.bind
            );
            defaults
        }
    };
    settings.save(first).await.context("storing the first settings")
}

/// Waits for the signal a service manager stops a process with (SIGTERM), or
/// for Ctrl-C.
async fn stop_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = tokio::signal::ctrl_c() => "Ctrl-C",
            },
            Err(e) => {
                tracing::warn!("cannot listen for SIGTERM: {e}");
                let _ = tokio::signal::ctrl_c().await;
                "Ctrl-C"
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "Ctrl-C"
    }
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
