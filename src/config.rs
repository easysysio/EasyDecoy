// =============================================================================
// config.rs — what EasyDecoy needs before the GUI exists
//
// The config file holds only what has to be known before the database is open
// and the GUI is listening: the log level, where the database is, and where
// the GUI listens. Everything else is set in the GUI and kept in the database
// (see src/settings.rs).
//
// 0.1.0 kept every setting in this file. Those keys are still read, once: on
// the first start with an empty database they become its settings (`legacy`),
// and after that they are reported as ignored. Unknown keys are an error, so a
// typo can't silently leave something at its default.
// =============================================================================

use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::settings::{HttpSettings, Settings, SyslogSettings, TelnetSettings};

/// The GUI's default address: this machine only, reached over an SSH tunnel.
/// A decoy host's other addresses are for intruders to find; the GUI must not
/// be one of the things they find.
pub const DEFAULT_GUI_BIND: &str = "127.0.0.1:9443";

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Verbosity when RUST_LOG isn't set: error, warn, info, debug or trace.
    pub log_level: String,
    /// The SQLite database. Empty puts it in systemd's state directory
    /// (/var/lib/easydecoy) when run as the service, or easydecoy.db here.
    pub db_path: String,
    /// Where the management GUI listens, as ip:port. Always HTTPS.
    pub gui_bind: String,
    /// A certificate and key (PEM files) for the GUI. Both empty uses a
    /// self-signed certificate made on the first start.
    pub gui_cert: String,
    pub gui_key: String,

    // ── 0.1.0 settings: imported into an empty database, then ignored ──────
    pub events_file: Option<String>,
    pub max_connections: Option<usize>,
    pub idle_timeout_secs: Option<u64>,
    pub session_limit_secs: Option<u64>,
    pub telnet: Option<TelnetSettings>,
    pub http: Option<HttpSettings>,
    pub syslog: Option<SyslogSettings>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            db_path: String::new(),
            gui_bind: DEFAULT_GUI_BIND.to_string(),
            gui_cert: String::new(),
            gui_key: String::new(),
            events_file: None,
            max_connections: None,
            idle_timeout_secs: None,
            session_limit_secs: None,
            telnet: None,
            http: None,
            syslog: None,
        }
    }
}

impl Config {
    // ─────────────────────────────────────────────────────────────────────────
    // load(path)
    // Reads and parses the TOML file at `path`. A missing file yields the
    // defaults (the bool says which happened); an unreadable or invalid one is
    // an error, because running with settings other than the ones written
    // would be a surprise.
    // ─────────────────────────────────────────────────────────────────────────
    pub fn load(path: &str) -> Result<(Self, bool)> {
        if !Path::new(path).exists() {
            return Ok((Self::default(), false));
        }
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let config: Self = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
        config.gui_addr().with_context(|| format!("checking {path}"))?;
        if config.gui_cert.trim().is_empty() != config.gui_key.trim().is_empty() {
            anyhow::bail!("{path}: gui_cert and gui_key go together; set both or neither");
        }
        Ok((config, true))
    }

    pub fn gui_addr(&self) -> Result<SocketAddr> {
        crate::settings::bind_address("gui_bind", &self.gui_bind).map_err(anyhow::Error::msg)
    }

    /// Where the database is: `db_path`, or the default for how EasyDecoy runs.
    pub fn db_file(&self) -> String {
        if !self.db_path.trim().is_empty() {
            return self.db_path.trim().to_string();
        }
        // systemd sets STATE_DIRECTORY for a unit with StateDirectory=.
        match std::env::var("STATE_DIRECTORY") {
            Ok(dir) if !dir.is_empty() => format!("{dir}/easydecoy.db"),
            _ => "easydecoy.db".to_string(),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // legacy()
    // The 0.1.0 settings this file still carries, as Settings, or None when
    // it has none. Keys the file leaves out keep the current defaults.
    // ─────────────────────────────────────────────────────────────────────────
    pub fn legacy(&self) -> Option<Settings> {
        let any = self.events_file.is_some()
            || self.max_connections.is_some()
            || self.idle_timeout_secs.is_some()
            || self.session_limit_secs.is_some()
            || self.telnet.is_some()
            || self.http.is_some()
            || self.syslog.is_some();
        if !any {
            return None;
        }
        let mut s = Settings::default();
        // 0.1.0 had no FTP decoy: an upgrade doesn't open a port it didn't
        // have. It can be turned on in the GUI.
        s.ftp.enabled = false;
        if let Some(v) = &self.events_file {
            // 0.1.0 wrote to stdout when events_file was empty; 0.2.0 spells
            // that "-", and empty means the database alone.
            s.recording.events_file = match v.trim() {
                "" => "-".to_string(),
                path => path.to_string(),
            };
        }
        if let Some(v) = self.max_connections {
            s.limits.max_connections = v;
        }
        if let Some(v) = self.idle_timeout_secs {
            s.limits.idle_timeout_secs = v;
        }
        if let Some(v) = self.session_limit_secs {
            s.limits.session_limit_secs = v;
        }
        if let Some(v) = &self.telnet {
            s.telnet = v.clone();
        }
        if let Some(v) = &self.http {
            s.http = v.clone();
        }
        if let Some(v) = &self.syslog {
            s.syslog = v.clone();
        }
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SyslogProtocol;

    #[test]
    fn a_misspelled_key_is_an_error() {
        assert!(toml::from_str::<Config>("gui_bnd = \"127.0.0.1:9443\"\n").is_err());
    }

    #[test]
    fn the_shipped_configs_are_valid_and_carry_no_old_settings() {
        for text in [include_str!("../config/easydecoy.toml"), include_str!("../packaging/easydecoy.toml")] {
            let config: Config = toml::from_str(text).unwrap();
            config.gui_addr().unwrap();
            assert!(config.legacy().is_none());
        }
    }

    #[test]
    fn a_0_1_0_config_becomes_the_first_settings() {
        // The config 0.1.0's package installed, as an administrator edited it.
        let text = r#"
log_level = "info"
events_file = "/var/log/easydecoy/events.jsonl"
max_connections = 64

[telnet]
enabled  = true
bind     = "0.0.0.0:23"
hostname = "fs-ams-03"

[http]
enabled = false

[syslog]
enabled  = true
server   = "10.0.0.5:514"
protocol = "tcp"
"#;
        let config: Config = toml::from_str(text).unwrap();
        let s = config.legacy().unwrap();
        assert_eq!(s.recording.events_file, "/var/log/easydecoy/events.jsonl");
        assert_eq!(s.limits.max_connections, 64);
        assert_eq!(s.limits.idle_timeout_secs, 30);
        assert_eq!(s.telnet.bind, "0.0.0.0:23");
        assert_eq!(s.telnet.hostname, "fs-ams-03");
        assert_eq!(s.telnet.banner, "Ubuntu 22.04.4 LTS");
        assert!(!s.http.enabled);
        assert!(!s.ftp.enabled, "0.1.0 had no FTP decoy");
        assert!(!s.ssh.enabled, "nor an SSH one");
        assert_eq!(s.syslog.protocol, SyslogProtocol::Tcp);
        // An empty events_file was stdout in 0.1.0, and stays stdout.
        let stdout: Config = toml::from_str("events_file = \"\"\n").unwrap();
        assert_eq!(stdout.legacy().unwrap().recording.events_file, "-");
        // GUI settings were not in 0.1.0, so they take their defaults.
        assert_eq!(config.gui_bind, DEFAULT_GUI_BIND);
    }

    #[test]
    fn the_database_goes_where_systemd_says_state_belongs() {
        let config = Config { db_path: "/data/decoy.db".into(), ..Config::default() };
        assert_eq!(config.db_file(), "/data/decoy.db");
    }
}
