// =============================================================================
// event.rs — what EasyDecoy records
//
// Every decoy reports what happens to it as an Event: a connection, a request,
// a login attempt. Each event becomes one JSON line, and the writer task sends
// that line to up to three places:
//   - the database, always: it is what the GUI shows and exports;
//   - the events file, when one is set ("-" is stdout), for tools that tail it;
//   - a syslog collector, when syslog is enabled (see src/syslog.rs).
//
// Decoys hand events to the single writer over a bounded channel. One writer
// means lines from concurrent sessions never interleave; the bound means a
// flood of connections slows the decoys down rather than filling memory. The
// writer takes whatever has queued up as one batch, so the database sees one
// transaction per batch, not one per event.
//
// The writer watches the settings: a new events file or syslog collector saved
// in the GUI takes effect from the next event.
// =============================================================================

use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result};
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::settings::{Settings, SettingsStore, SyslogSettings};
use crate::syslog::{self, SyslogTx};

// Events waiting to be written before decoys start waiting for the writer.
const QUEUE: usize = 1024;
// The most events written as one batch.
const BATCH: usize = 256;

/// One thing that happened to a decoy. `details` holds the fields specific to
/// the kind of event and is flattened into the top level of the JSON line.
#[derive(Debug, Serialize)]
pub struct Event {
    #[serde(serialize_with = "serialize_time")]
    pub time: OffsetDateTime,
    pub decoy: &'static str,
    pub kind: &'static str,
    pub src_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    #[serde(flatten)]
    pub details: Map<String, Value>,
}

impl Event {
    pub fn new(decoy: &'static str, kind: &'static str, peer: SocketAddr, local: SocketAddr) -> Self {
        Self {
            time: OffsetDateTime::now_utc(),
            decoy,
            kind,
            // A dual-stack listener reports IPv4 clients as ::ffff:a.b.c.d.
            src_ip: peer.ip().to_canonical(),
            src_port: peer.port(),
            dst_port: local.port(),
            details: Map::new(),
        }
    }

    /// Adds a detail field.
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.details.insert(key.to_string(), value.into());
        self
    }
}

/// A time as EasyDecoy writes it everywhere: UTC, microseconds, always the
/// same width, so stored times sort and compare as text.
pub fn stamp(time: OffsetDateTime) -> String {
    let format = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6]Z");
    // Formatting a UTC time with this description fails only for a year past
    // 9999, which no clock reports.
    time.to_offset(UtcOffset::UTC).format(format).unwrap_or_default()
}

fn serialize_time<S: Serializer>(time: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&stamp(*time))
}

/// Where decoys send events. Cheap to clone; every decoy holds one.
#[derive(Clone)]
pub struct EventTx(mpsc::Sender<Event>);

impl EventTx {
    pub async fn send(&self, event: Event) {
        // Fails only once the writer has stopped, at shutdown.
        let _ = self.0.send(event).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// spawn_writer(db, settings)
// Starts the writer task. It runs until every EventTx is dropped, then writes
// what is still queued and ends, so awaiting the handle at shutdown loses
// nothing.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn spawn_writer(db: SqlitePool, settings: SettingsStore) -> (EventTx, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<Event>(QUEUE);
    let mut watch = settings.subscribe();
    let current = watch.borrow_and_update().clone();
    let mut outputs = Outputs::default();
    outputs.apply(&current).await;
    let handle = tokio::spawn(async move {
        let mut rx = rx;
        let mut batch = Vec::with_capacity(BATCH);
        loop {
            tokio::select! {
                // Events first: a settings change waits for the batch in hand.
                biased;
                n = rx.recv_many(&mut batch, BATCH) => {
                    if n == 0 {
                        break;
                    }
                    outputs.write(&db, &mut batch).await;
                }
                changed = watch.changed() => {
                    if changed.is_err() {
                        // The settings store is gone; keep what we have.
                        continue;
                    }
                    let new = watch.borrow_and_update().clone();
                    outputs.apply(&new).await;
                }
            }
        }
    });
    (EventTx(tx), handle)
}

/// Where events go besides the database, as the settings last said.
#[derive(Default)]
struct Outputs {
    file_path: String,
    file: Option<Sink>,
    syslog_settings: Option<SyslogSettings>,
    syslog: Option<SyslogTx>,
}

enum Sink {
    File(tokio::fs::File),
    Stdout(tokio::io::Stdout),
}

impl Outputs {
    /// Brings the events file and syslog in line with `settings`, touching
    /// only what changed.
    async fn apply(&mut self, settings: &Settings) {
        let path = settings.recording.events_file.trim();
        if path != self.file_path {
            self.file_path = path.to_string();
            self.file = match open_sink(path).await {
                Ok(sink) => {
                    match path {
                        "" => tracing::info!("events: database only"),
                        "-" => tracing::info!("events: database and stdout"),
                        p => tracing::info!("events: database and {p}"),
                    }
                    sink
                }
                Err(e) => {
                    // The GUI checks a path before saving it, so this is a file
                    // that went away or a 0.1.0 import; the database still has
                    // every event.
                    tracing::error!("{e:#}; events go to the database only");
                    None
                }
            };
        }

        let wanted = settings.syslog.enabled.then(|| settings.syslog.clone());
        if wanted != self.syslog_settings {
            // Dropping the old sender lets it finish its queue and stop.
            self.syslog = wanted.as_ref().map(syslog::spawn);
            match &wanted {
                Some(s) => tracing::info!(
                    "syslog: {} to {} ({}, {})",
                    format!("{:?}", s.protocol).to_lowercase(),
                    s.server,
                    format!("{:?}", s.format).to_lowercase(),
                    format!("{:?}", s.facility).to_lowercase(),
                ),
                None if self.syslog_settings.is_some() => tracing::info!("syslog: off"),
                None => {}
            }
            self.syslog_settings = wanted;
        }
    }

    /// Writes a batch everywhere it goes, and empties it.
    async fn write(&mut self, db: &SqlitePool, batch: &mut Vec<Event>) {
        let mut stored = Vec::with_capacity(batch.len());
        let mut text = String::new();
        for event in batch.drain(..) {
            let line = match serde_json::to_string(&event) {
                Ok(line) => line,
                Err(e) => {
                    tracing::error!("event could not be serialised: {e}");
                    continue;
                }
            };
            if !matches!(self.file, Some(Sink::Stdout(_))) {
                // Stdout carries nothing but events, so it isn't also logged.
                tracing::info!("{} {} from {}", event.decoy, event.kind, event.src_ip);
            }
            if let Some(syslog) = &self.syslog {
                syslog.send(event.time, event.kind, &line);
            }
            if self.file.is_some() {
                text.push_str(&line);
                text.push('\n');
            }
            stored.push((event, line));
        }

        // Flushed per batch: one still sitting in a buffer when the process
        // dies is one nobody sees.
        if let Some(sink) = &mut self.file {
            let written = match sink {
                Sink::File(f) => write_all(f, &text).await,
                Sink::Stdout(s) => write_all(s, &text).await,
            };
            if let Err(e) = written {
                tracing::error!("events file {}: {e}", self.file_path);
            }
        }
        if let Err(e) = crate::store::insert(db, &stored).await {
            tracing::error!("{} event(s) could not be stored: {e:#}", stored.len());
        }
    }
}

async fn write_all(out: &mut (impl AsyncWriteExt + Unpin), text: &str) -> std::io::Result<()> {
    out.write_all(text.as_bytes()).await?;
    out.flush().await
}

/// Opens the events file for appending: None for "", stdout for "-".
async fn open_sink(path: &str) -> Result<Option<Sink>> {
    match path {
        "" => Ok(None),
        "-" => Ok(Some(Sink::Stdout(tokio::io::stdout()))),
        path => {
            let file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await
                .with_context(|| format!("opening events file {path}"))?;
            Ok(Some(Sink::File(file)))
        }
    }
}

/// Checks that EasyDecoy can append to `path`, for the GUI to call before
/// saving it.
pub async fn check_events_file(path: &str) -> Result<(), String> {
    open_sink(path.trim()).await.map(|_| ()).map_err(|e| format!("{e:#}"))
}

/// Makes text from a client safe to record: at most `max` characters, with
/// control characters (newlines, escapes) replaced so they can't forge or mangle
/// log lines.
pub fn clean(text: &str, max: usize) -> String {
    text.chars().take(max).map(|c| if c.is_control() { '.' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_are_flattened_into_the_line() {
        let peer: SocketAddr = "[::ffff:10.0.0.5]:51000".parse().unwrap();
        let local: SocketAddr = "10.0.0.1:23".parse().unwrap();
        let event = Event::new("telnet", "login_attempt", peer, local).with("username", "root");
        let json: Value = serde_json::to_value(&event).unwrap();
        assert_eq!(json["src_ip"], "10.0.0.5");
        assert_eq!(json["dst_port"], 23);
        assert_eq!(json["username"], "root");
        assert_eq!(json["time"].as_str().unwrap().len(), "2026-01-01T00:00:00.000000Z".len());
    }

    #[test]
    fn clean_strips_control_characters_and_truncates() {
        assert_eq!(clean("root\r\n\x1b[31m", 64), "root...[31m");
        assert_eq!(clean("abcdef", 3), "abc");
    }

    #[tokio::test]
    async fn the_writer_stores_every_event_and_drains_at_shutdown() {
        let db = crate::db::memory().await;
        let (settings, _) = SettingsStore::load(db.clone()).await.unwrap();
        let (tx, handle) = spawn_writer(db.clone(), settings).await;
        let peer: SocketAddr = "10.0.0.5:50000".parse().unwrap();
        let local: SocketAddr = "10.0.0.1:23".parse().unwrap();
        for _ in 0..300 {
            tx.send(Event::new("telnet", "connect", peer, local)).await;
        }
        drop(tx);
        handle.await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events").fetch_one(&db).await.unwrap();
        assert_eq!(count, 300);
    }
}
