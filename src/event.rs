// =============================================================================
// event.rs — what EasyDecoy records
//
// Every decoy reports what happens to it as an Event: a connection, a request,
// a login attempt. Events are written one JSON object per line, to the file in
// `events_file` or, when that is empty, to stdout. EasyDecoy's own log goes to
// stderr, so stdout carries nothing but events and can be piped into jq.
//
// Decoys hand events to a single writer task over a bounded channel. One
// writer means lines from concurrent sessions never interleave; the bound
// means a flood of connections slows the decoys down rather than filling
// memory with unwritten events.
//
// With [syslog] enabled, the writer also passes each line on to the syslog
// sender, after the local copy is written and without waiting for it (see
// src/syslog.rs).
// =============================================================================

use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::syslog::SyslogTx;

// Events waiting to be written before decoys start waiting for the writer.
const QUEUE: usize = 1024;

/// One thing that happened to a decoy. `details` holds the fields specific to
/// the kind of event and is flattened into the top level of the JSON line.
#[derive(Debug, Serialize)]
pub struct Event {
    pub time: DateTime<Utc>,
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
            time: Utc::now(),
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
// spawn_writer(path, syslog)
// Opens the events file (appending; empty path means stdout) and starts the
// task that writes every event sent to the returned EventTx, copying each to
// `syslog` when there is one. Opening happens here, before any decoy starts,
// so a bad path stops EasyDecoy at startup instead of losing events later.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn spawn_writer(path: &str, syslog: Option<SyslogTx>) -> Result<EventTx> {
    let path = path.trim();
    let mut file = if path.is_empty() {
        None
    } else {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .with_context(|| format!("opening events file {path}"))?;
        Some(file)
    };

    let (tx, mut rx) = mpsc::channel::<Event>(QUEUE);
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(event) = rx.recv().await {
            let mut line = match serde_json::to_string(&event) {
                Ok(line) => line,
                Err(e) => {
                    tracing::error!("event could not be serialised: {e}");
                    continue;
                }
            };
            line.push('\n');
            // Flushed per event: events are rare, and one still sitting in a
            // buffer when the process dies is one nobody sees.
            let written = match file.as_mut() {
                Some(file) => {
                    // The events go to a file, so the log says what happened.
                    tracing::info!("{} {} from {}", event.decoy, event.kind, event.src_ip);
                    write_line(file, &line).await
                }
                None => write_line(&mut stdout, &line).await,
            };
            if let Err(e) = written {
                tracing::error!("event could not be written: {e}");
            }
            if let Some(syslog) = &syslog {
                syslog.send(event.time, event.kind, line.trim_end());
            }
        }
    });
    Ok(EventTx(tx))
}

async fn write_line(out: &mut (impl AsyncWriteExt + Unpin), line: &str) -> std::io::Result<()> {
    out.write_all(line.as_bytes()).await?;
    out.flush().await
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
    }

    #[test]
    fn clean_strips_control_characters_and_truncates() {
        assert_eq!(clean("root\r\n\x1b[31m", 64), "root...[31m");
        assert_eq!(clean("abcdef", 3), "abc");
    }
}
