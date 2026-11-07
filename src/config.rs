// =============================================================================
// config.rs — EasyDecoy runtime configuration
//
// Loads settings from a TOML file (default: config/easydecoy.toml): where
// events go, the limits every decoy runs under, and one section per decoy.
// Every field has a default, so a partial or missing config still yields a
// running honeypot. Unknown keys are an error, so a typo can't silently leave
// a decoy at its defaults.
// =============================================================================

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// Top-level configuration loaded from TOML.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Verbosity when RUST_LOG isn't set: error, warn, info, debug or trace.
    pub log_level: String,
    /// File events are appended to as JSON lines. Empty writes them to stdout.
    pub events_file: String,
    /// Concurrent client sessions across all decoys; more are closed at once.
    pub max_connections: usize,
    /// A client that sends nothing for this long is disconnected.
    pub idle_timeout_secs: u64,
    /// No session lasts longer than this, however active the client.
    pub session_limit_secs: u64,
    pub telnet: TelnetConfig,
    pub http: HttpConfig,
    pub syslog: SyslogConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            events_file: String::new(),
            max_connections: 256,
            idle_timeout_secs: 30,
            session_limit_secs: 120,
            telnet: TelnetConfig::default(),
            http: HttpConfig::default(),
            syslog: SyslogConfig::default(),
        }
    }
}

/// The Telnet decoy: a login prompt that never accepts a login.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelnetConfig {
    pub enabled: bool,
    /// Address and port to listen on.
    pub bind: String,
    /// Printed when a client connects, as a real host prints /etc/issue.net.
    pub banner: String,
    /// Shown in the prompt: "<hostname> login:".
    pub hostname: String,
}

impl Default for TelnetConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "0.0.0.0:2323".to_string(),
            banner: "Ubuntu 22.04.4 LTS".to_string(),
            hostname: "fileserver01".to_string(),
        }
    }
}

/// The HTTP decoy: a web administration sign-in page.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub enabled: bool,
    /// Address and port to listen on.
    pub bind: String,
    /// Sent as the Server response header. Empty leaves the header out.
    pub server_header: String,
    /// Title and heading of the sign-in page.
    pub title: String,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "0.0.0.0:8080".to_string(),
            server_header: "nginx/1.24.0".to_string(),
            title: "Admin Console".to_string(),
        }
    }
}

/// Sending every event to a syslog collector as well (see src/syslog.rs).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyslogConfig {
    pub enabled: bool,
    /// The collector, as host:port. A name is resolved again for each message.
    pub server: String,
    pub protocol: SyslogProtocol,
    pub format: SyslogFormat,
    pub facility: Facility,
    /// The HOSTNAME field. Empty uses this machine's name.
    pub hostname: String,
}

impl Default for SyslogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: "127.0.0.1:514".to_string(),
            protocol: SyslogProtocol::Udp,
            format: SyslogFormat::Rfc5424,
            facility: Facility::Local0,
            hostname: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyslogProtocol {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyslogFormat {
    /// Full UTC timestamp with milliseconds, and the event kind as MSGID.
    Rfc5424,
    /// The older BSD format, for collectors that read nothing else.
    Rfc3164,
}

/// The syslog facilities it makes sense to file decoy events under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Facility {
    User,
    Daemon,
    Auth,
    Authpriv,
    Local0,
    Local1,
    Local2,
    Local3,
    Local4,
    Local5,
    Local6,
    Local7,
}

impl Facility {
    /// The facility's number (RFC 5424 §6.2.1).
    pub fn code(self) -> u8 {
        match self {
            Self::User => 1,
            Self::Daemon => 3,
            Self::Auth => 4,
            Self::Authpriv => 10,
            Self::Local0 => 16,
            Self::Local1 => 17,
            Self::Local2 => 18,
            Self::Local3 => 19,
            Self::Local4 => 20,
            Self::Local5 => 21,
            Self::Local6 => 22,
            Self::Local7 => 23,
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
        config.check().with_context(|| format!("checking {path}"))?;
        Ok((config, true))
    }

    /// What parsing alone cannot catch.
    fn check(&self) -> Result<()> {
        if self.syslog.enabled {
            // A missing port would otherwise surface only as a failed send,
            // when the first event arrives.
            let port = self.syslog.server.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok());
            if port.is_none_or(|p| p == 0) {
                anyhow::bail!("syslog server {:?} must be host:port, e.g. 10.0.0.5:514", self.syslog.server);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_config_keeps_the_other_defaults() {
        let config: Config = toml::from_str("[http]\nenabled = false\n").unwrap();
        assert!(!config.http.enabled);
        assert!(config.telnet.enabled);
        assert_eq!(config.http.bind, "0.0.0.0:8080");
    }

    #[test]
    fn a_misspelled_key_is_an_error() {
        assert!(toml::from_str::<Config>("[telnet]\nenabeld = false\n").is_err());
    }

    #[test]
    fn reads_the_syslog_choices() {
        let config: Config =
            toml::from_str("[syslog]\nenabled = true\nprotocol = \"tcp\"\nfacility = \"auth\"\n").unwrap();
        assert_eq!(config.syslog.protocol, SyslogProtocol::Tcp);
        assert_eq!(config.syslog.facility.code(), 4);
        assert_eq!(config.syslog.format, SyslogFormat::Rfc5424);
        assert!(toml::from_str::<Config>("[syslog]\nprotocol = \"tls\"\n").is_err());
        assert!(toml::from_str::<Config>("[syslog]\nfacility = \"kern\"\n").is_err());
    }

    #[test]
    fn the_shipped_configs_are_valid() {
        for text in [include_str!("../config/easydecoy.toml"), include_str!("../packaging/easydecoy.toml")] {
            let config: Config = toml::from_str(text).unwrap();
            config.check().unwrap();
        }
    }

    #[test]
    fn a_syslog_server_needs_a_port() {
        let check = |server: &str| {
            let mut config = Config::default();
            config.syslog.enabled = true;
            config.syslog.server = server.to_string();
            config.check()
        };
        assert!(check("10.0.0.5:514").is_ok());
        assert!(check("[fd00::5]:514").is_ok());
        assert!(check("logs.example.com:6514").is_ok());
        assert!(check("10.0.0.5").is_err());
        assert!(check("10.0.0.5:0").is_err());
    }
}
