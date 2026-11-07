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
        let config = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
        Ok((config, true))
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
}
