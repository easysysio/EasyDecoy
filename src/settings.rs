// =============================================================================
// settings.rs — what the GUI configures
//
// Everything an administrator changes from the GUI: the decoys, the limits
// they run under, where events are recorded, and syslog. Settings live in the
// database, one JSON row per section, and every part of EasyDecoy that uses
// them watches a single channel, so a change saved in the GUI reaches the
// decoys, the event writer and the syslog sender without a restart.
//
// A section the database has no row for takes its defaults, and a field a
// stored section does not have does too, so a release that adds a setting
// reads an older database without a migration.
// =============================================================================

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, watch};

/// Every setting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub limits: Limits,
    pub telnet: TelnetSettings,
    pub http: HttpSettings,
    pub recording: Recording,
    pub syslog: SyslogSettings,
}

/// What every decoy runs under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Concurrent client sessions across all decoys; more are closed at once.
    pub max_connections: usize,
    /// A client that sends nothing for this long is disconnected.
    pub idle_timeout_secs: u64,
    /// No session lasts longer than this, however active the client.
    pub session_limit_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_connections: 256, idle_timeout_secs: 30, session_limit_secs: 120 }
    }
}

/// The Telnet decoy: a login prompt that never accepts a login.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelnetSettings {
    pub enabled: bool,
    /// Address and port to listen on, as ip:port.
    pub bind: String,
    /// Printed when a client connects, as a real host prints /etc/issue.net.
    pub banner: String,
    /// Shown in the prompt: "<hostname> login:".
    pub hostname: String,
}

impl Default for TelnetSettings {
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpSettings {
    pub enabled: bool,
    /// Address and port to listen on, as ip:port.
    pub bind: String,
    /// Sent as the Server response header. Empty leaves the header out.
    pub server_header: String,
    /// Title and heading of the sign-in page.
    pub title: String,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "0.0.0.0:8080".to_string(),
            server_header: "nginx/1.24.0".to_string(),
            title: "Admin Console".to_string(),
        }
    }
}

/// Where events are kept, and for how long.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recording {
    /// Events older than this many days are deleted from the database. 0 keeps
    /// them forever.
    pub retention_days: u32,
    /// A file every event is also appended to, as a JSON line. Empty writes no
    /// file; "-" writes to stdout.
    pub events_file: String,
}

impl Default for Recording {
    fn default() -> Self {
        Self { retention_days: 90, events_file: String::new() }
    }
}

/// Sending every event to a syslog collector as well (see src/syslog.rs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyslogSettings {
    pub enabled: bool,
    /// The collector, as host:port. A name is resolved again for each message.
    pub server: String,
    pub protocol: SyslogProtocol,
    pub format: SyslogFormat,
    pub facility: Facility,
    /// The HOSTNAME field. Empty uses this machine's name.
    pub hostname: String,
}

impl Default for SyslogSettings {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyslogProtocol {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyslogFormat {
    /// Full UTC timestamp with milliseconds, and the event kind as MSGID.
    Rfc5424,
    /// The older BSD format, for collectors that read nothing else.
    Rfc3164,
}

/// The syslog facilities it makes sense to file decoy events under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl Settings {
    // ─────────────────────────────────────────────────────────────────────────
    // check(gui)
    // What makes a set of settings unusable, in words for the person who typed
    // it. `gui` is where the management GUI listens: a decoy may not take its
    // port.
    // ─────────────────────────────────────────────────────────────────────────
    pub fn check(&self, gui: SocketAddr) -> Result<(), String> {
        let l = &self.limits;
        within("Concurrent sessions", l.max_connections as u64, 1, 100_000)?;
        within("Idle timeout", l.idle_timeout_secs, 1, 3_600)?;
        within("Session limit", l.session_limit_secs, 1, 86_400)?;
        within("Retention", self.recording.retention_days as u64, 0, 3_650)?;

        let telnet = bind_address("Telnet", &self.telnet.bind)?;
        let http = bind_address("HTTP", &self.http.bind)?;
        text("Telnet banner", &self.telnet.banner, 0, 200)?;
        text("Telnet hostname", &self.telnet.hostname, 1, 64)?;
        text("HTTP Server header", &self.http.server_header, 0, 128)?;
        text("HTTP page title", &self.http.title, 1, 128)?;

        let mut taken = vec![("the management GUI", gui)];
        if self.telnet.enabled {
            taken.push(("the Telnet decoy", telnet));
        }
        if self.http.enabled {
            taken.push(("the HTTP decoy", http));
        }
        for (i, (a, x)) in taken.iter().enumerate() {
            for (b, y) in &taken[i + 1..] {
                if overlap(*x, *y) {
                    return Err(format!("{b} would listen on {y}, which {a} already uses"));
                }
            }
        }

        if self.syslog.enabled {
            let port = self.syslog.server.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok());
            if port.is_none_or(|p| p == 0) {
                return Err(format!(
                    "The syslog server {:?} must be host:port, e.g. 10.0.0.5:514",
                    self.syslog.server
                ));
            }
            text("Syslog hostname", &self.syslog.hostname, 0, 255)?;
        }
        text("Events file", &self.recording.events_file, 0, 4096)?;
        Ok(())
    }
}

fn within(what: &str, value: u64, min: u64, max: u64) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("{what} must be between {min} and {max}"))
    }
}

fn text(what: &str, value: &str, min: usize, max: usize) -> Result<(), String> {
    let n = value.chars().count();
    if n < min {
        return Err(format!("{what} cannot be empty"));
    }
    if n > max {
        return Err(format!("{what} can be at most {max} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{what} cannot contain line breaks or control characters"));
    }
    Ok(())
}

/// A decoy listens on an address, not a name: ip:port, e.g. 0.0.0.0:23.
pub fn bind_address(what: &str, bind: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = bind
        .trim()
        .parse()
        .map_err(|_| format!("{what} must listen on ip:port, e.g. 0.0.0.0:23 or [::]:23"))?;
    if addr.port() == 0 {
        return Err(format!("{what} needs a port"));
    }
    Ok(addr)
}

/// Whether two listeners would collide: the same port on the same address, or
/// on any address when one of them listens on all of them.
fn overlap(a: SocketAddr, b: SocketAddr) -> bool {
    a.port() == b.port() && (a.ip() == b.ip() || a.ip().is_unspecified() || b.ip().is_unspecified())
}

// ═════════════════════════════════════════════════════════════════════════════
// The store: settings in the database, and the channel that announces them
// ═════════════════════════════════════════════════════════════════════════════

/// Settings as stored, and the channel every part of EasyDecoy watches.
#[derive(Clone)]
pub struct SettingsStore {
    db: SqlitePool,
    tx: Arc<watch::Sender<Arc<Settings>>>,
    // Saves one at a time, so two administrators saving at once cannot leave a
    // mix of both behind.
    saving: Arc<Mutex<()>>,
}

impl SettingsStore {
    // ─────────────────────────────────────────────────────────────────────────
    // load(db)
    // Reads every section from the database. The bool says whether the
    // database had no settings at all, which is a first start.
    // ─────────────────────────────────────────────────────────────────────────
    pub async fn load(db: SqlitePool) -> Result<(Self, bool)> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT section, value FROM settings")
            .fetch_all(&db)
            .await
            .context("reading settings")?;
        let empty = rows.is_empty();

        let mut sections = serde_json::Map::new();
        for (section, value) in rows {
            let value: serde_json::Value = serde_json::from_str(&value)
                .with_context(|| format!("the stored {section} settings are not valid JSON"))?;
            sections.insert(section, value);
        }
        let settings: Settings = serde_json::from_value(serde_json::Value::Object(sections))
            .context("the stored settings cannot be read")?;

        let (tx, _) = watch::channel(Arc::new(settings));
        Ok((Self { db, tx: Arc::new(tx), saving: Arc::new(Mutex::new(())) }, empty))
    }

    pub fn current(&self) -> Arc<Settings> {
        self.tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Settings>> {
        self.tx.subscribe()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // save(new)
    // Writes every section in one transaction, then announces the new settings.
    // The caller checks them first (Settings::check).
    // ─────────────────────────────────────────────────────────────────────────
    pub async fn save(&self, new: Settings) -> Result<()> {
        let _one_at_a_time = self.saving.lock().await;
        let serde_json::Value::Object(sections) = serde_json::to_value(&new)? else {
            unreachable!("Settings serialises to an object");
        };
        let mut tx = self.db.begin().await?;
        for (section, value) in sections {
            sqlx::query(
                "INSERT INTO settings (section, value) VALUES (?, ?) \
                 ON CONFLICT (section) DO UPDATE SET value = excluded.value",
            )
            .bind(section)
            .bind(value.to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await.context("saving settings")?;
        self.tx.send_replace(Arc::new(new));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui() -> SocketAddr {
        "127.0.0.1:9443".parse().unwrap()
    }

    #[test]
    fn the_defaults_are_valid() {
        Settings::default().check(gui()).unwrap();
    }

    #[test]
    fn a_decoy_cannot_take_a_port_already_in_use() {
        let mut s = Settings::default();
        s.http.bind = "0.0.0.0:2323".into();
        assert!(s.check(gui()).unwrap_err().contains("Telnet decoy already uses"));

        // On all addresses, port 9443 would include the GUI's 127.0.0.1:9443.
        let mut s = Settings::default();
        s.telnet.bind = "0.0.0.0:9443".into();
        assert!(s.check(gui()).unwrap_err().contains("management GUI"));

        // A disabled decoy holds no port.
        let mut s = Settings::default();
        s.http.bind = "0.0.0.0:2323".into();
        s.telnet.enabled = false;
        s.check(gui()).unwrap();

        // Same port on two different addresses is fine.
        let mut s = Settings::default();
        s.telnet.bind = "10.0.0.9:9443".into();
        s.check(gui()).unwrap();
    }

    #[test]
    fn rejects_what_a_decoy_cannot_use() {
        let check = |f: fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            s.check(gui())
        };
        assert!(check(|s| s.telnet.bind = "fileserver:23".into()).is_err());
        assert!(check(|s| s.telnet.bind = "0.0.0.0:0".into()).is_err());
        assert!(check(|s| s.telnet.hostname = String::new()).is_err());
        assert!(check(|s| s.http.server_header = "nginx\r\nX-Evil: 1".into()).is_err());
        assert!(check(|s| s.limits.max_connections = 0).is_err());
        assert!(check(|s| {
            s.syslog.enabled = true;
            s.syslog.server = "10.0.0.5".into();
        })
        .is_err());
        assert!(check(|s| {
            s.syslog.enabled = true;
            s.syslog.server = "[fd00::5]:514".into();
        })
        .is_ok());
    }

    #[tokio::test]
    async fn settings_survive_a_round_trip_through_the_database() {
        let db = crate::db::memory().await;
        let (store, empty) = SettingsStore::load(db.clone()).await.unwrap();
        assert!(empty);
        assert_eq!(*store.current(), Settings::default());

        let mut changed = Settings::default();
        changed.telnet.hostname = "fs-ams-03".into();
        changed.syslog.protocol = SyslogProtocol::Tcp;
        let mut watcher = store.subscribe();
        store.save(changed.clone()).await.unwrap();
        assert!(watcher.has_changed().unwrap());
        assert_eq!(**watcher.borrow_and_update(), changed);

        let (reloaded, empty) = SettingsStore::load(db).await.unwrap();
        assert!(!empty);
        assert_eq!(*reloaded.current(), changed);
    }

    #[tokio::test]
    async fn a_section_missing_from_the_database_takes_its_defaults() {
        let db = crate::db::memory().await;
        sqlx::query("INSERT INTO settings VALUES ('telnet', '{\"hostname\":\"fs1\"}')")
            .execute(&db)
            .await
            .unwrap();
        let (store, _) = SettingsStore::load(db).await.unwrap();
        let s = store.current();
        assert_eq!(s.telnet.hostname, "fs1");
        assert_eq!(s.telnet.bind, TelnetSettings::default().bind);
        assert_eq!(s.http, HttpSettings::default());
    }
}
