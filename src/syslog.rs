// =============================================================================
// syslog.rs — events sent to a syslog collector
//
// When [syslog] is enabled, every event is also sent to a collector such as
// EasyLog, over UDP or TCP, as one syslog message whose text is the event's
// JSON line. Login attempts go at severity warning, everything else at notice.
// In RFC 5424 the event's kind is also the message ID, so a collector can
// filter on it without parsing the JSON.
//
// Syslog is a copy, never the record: the events file (or stdout) is written
// first, and messages are handed to this module's task with try_send. A
// collector that is slow, unreachable or gone costs dropped messages, counted
// and reported every few minutes, never a stalled decoy or a missing line in
// the local file.
//
// TCP messages are newline-framed, which EasyLog and rsyslog both read. A lost
// connection is made again on a later message, at most once every RETRY. Plain
// TCP syslog has no acknowledgements: a message written just before the
// collector went away can be lost without an error.
// =============================================================================

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket, lookup_host};
use tokio::sync::mpsc;

use crate::config::{SyslogConfig, SyslogFormat, SyslogProtocol};

// Messages waiting to be sent before new ones are dropped.
const QUEUE: usize = 1024;
// How long a TCP connect or write may take before the collector counts as down.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);
// Least time between attempts to reach a collector that is down.
const RETRY: Duration = Duration::from_secs(10);
// How often undelivered messages are reported.
const REPORT: Duration = Duration::from_secs(300);
// APP-NAME / TAG in every message.
const APP: &str = "easydecoy";

/// Syslog severities EasyDecoy uses (RFC 5424 §6.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning = 4,
    Notice = 5,
}

impl Severity {
    /// Someone trying to log in is more than someone connecting.
    pub fn of(kind: &str) -> Self {
        if kind == "login_attempt" { Self::Warning } else { Self::Notice }
    }
}

/// One event, waiting to be sent.
struct Message {
    time: DateTime<Utc>,
    kind: &'static str,
    text: String,
}

/// Where the event writer hands events for syslog. Never blocks.
pub struct SyslogTx {
    tx: mpsc::Sender<Message>,
    lost: Arc<AtomicU64>,
}

impl SyslogTx {
    pub fn send(&self, time: DateTime<Utc>, kind: &'static str, text: &str) {
        let message = Message { time, kind, text: text.to_string() };
        if self.tx.try_send(message).is_err() {
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// spawn(config)
// Starts the task that sends to the collector and returns its handle. Nothing
// is connected yet: a collector that is down at startup is reported when the
// first event cannot reach it, and EasyDecoy runs regardless.
// ─────────────────────────────────────────────────────────────────────────────
pub fn spawn(config: &SyslogConfig) -> SyslogTx {
    let (tx, rx) = mpsc::channel(QUEUE);
    let lost = Arc::new(AtomicU64::new(0));
    let hostname = match config.hostname.trim() {
        "" => system_hostname(),
        name => name.to_string(),
    };
    let sender = Sender {
        config: config.clone(),
        hostname: header_field(&hostname),
        udp: None,
        tcp: None,
        next_connect: None,
        failing: false,
    };
    tokio::spawn(sender.run(rx, lost.clone()));
    SyslogTx { tx, lost }
}

struct Sender {
    config: SyslogConfig,
    hostname: String,
    udp: Option<UdpSocket>,
    tcp: Option<TcpStream>,
    // While the collector is down: the earliest time to try it again.
    next_connect: Option<Instant>,
    // So an outage is reported once when it starts and once when it ends,
    // rather than once per event.
    failing: bool,
}

impl Sender {
    async fn run(mut self, mut rx: mpsc::Receiver<Message>, lost: Arc<AtomicU64>) {
        let mut report = tokio::time::interval(REPORT);
        report.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        report.tick().await; // the first tick is immediate
        let mut reported = 0;

        loop {
            tokio::select! {
                message = rx.recv() => {
                    let Some(message) = message else { break };
                    let frame = self.frame(&message);
                    match self.deliver(&frame).await {
                        Ok(()) if self.failing => {
                            tracing::info!("syslog: delivering to {} again", self.config.server);
                            self.failing = false;
                        }
                        Ok(()) => {}
                        Err(e) => {
                            lost.fetch_add(1, Ordering::Relaxed);
                            if !self.failing {
                                tracing::warn!(
                                    "syslog: cannot deliver to {}: {e} (events are still recorded locally)",
                                    self.config.server
                                );
                                self.failing = true;
                            }
                        }
                    }
                }
                _ = report.tick() => {
                    let total = lost.load(Ordering::Relaxed);
                    if total > reported {
                        tracing::warn!(
                            "syslog: {} event(s) not delivered in the last {} minutes ({total} in all)",
                            total - reported,
                            REPORT.as_secs() / 60
                        );
                        reported = total;
                    }
                }
            }
        }
    }

    /// The message as it goes on the wire.
    fn frame(&self, message: &Message) -> Vec<u8> {
        let pri = self.config.facility.code() * 8 + Severity::of(message.kind) as u8;
        let host = &self.hostname;
        let text = &message.text;
        let mut line = match self.config.format {
            SyslogFormat::Rfc5424 => {
                let time = message.time.to_rfc3339_opts(SecondsFormat::Millis, true);
                format!("<{pri}>1 {time} {host} {APP} - {} - {text}", message.kind)
            }
            SyslogFormat::Rfc3164 => {
                // RFC 3164 has no time zone; this is UTC, like EasyWAF's.
                let time = message.time.format("%b %e %H:%M:%S");
                format!("<{pri}>{time} {host} {APP}: {text}")
            }
        };
        if self.config.protocol == SyslogProtocol::Tcp {
            line.push('\n');
        }
        line.into_bytes()
    }

    async fn deliver(&mut self, frame: &[u8]) -> io::Result<()> {
        match self.config.protocol {
            SyslogProtocol::Udp => {
                // Resolved per message, so a collector whose address changes is
                // followed. Events are rare enough for this to cost nothing.
                let addr = resolve(&self.config.server).await?;
                let socket = self.udp_socket(addr).await?;
                socket.send_to(frame, addr).await?;
                Ok(())
            }
            SyslogProtocol::Tcp => {
                if let Some(stream) = self.tcp.as_mut() {
                    match tokio::time::timeout(NETWORK_TIMEOUT, stream.write_all(frame)).await {
                        Ok(Ok(())) => return Ok(()),
                        // The connection went away: make a new one below and
                        // send this message on it.
                        _ => self.tcp = None,
                    }
                }
                if self.next_connect.is_some_and(|next| Instant::now() < next) {
                    return Err(io::Error::other("collector is down, waiting to retry"));
                }
                self.next_connect = Some(Instant::now() + RETRY);

                let addr = resolve(&self.config.server).await?;
                let mut stream = tokio::time::timeout(NETWORK_TIMEOUT, TcpStream::connect(addr))
                    .await
                    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timed out"))??;
                tokio::time::timeout(NETWORK_TIMEOUT, stream.write_all(frame))
                    .await
                    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "write timed out"))??;
                self.tcp = Some(stream);
                self.next_connect = None;
                Ok(())
            }
        }
    }

    /// A UDP socket of the same family as `addr`, opened on first use.
    async fn udp_socket(&mut self, addr: SocketAddr) -> io::Result<&UdpSocket> {
        let matches = self
            .udp
            .as_ref()
            .and_then(|s| s.local_addr().ok())
            .is_some_and(|local| local.is_ipv4() == addr.is_ipv4());
        if !matches {
            let any = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
            self.udp = Some(UdpSocket::bind(any).await?);
        }
        Ok(self.udp.as_ref().expect("opened above"))
    }
}

async fn resolve(server: &str) -> io::Result<SocketAddr> {
    lookup_host(server)
        .await?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the name has no address"))
}

/// This machine's name, as the collector should see it: the sensor that saw
/// the event, not the name a decoy pretends to have.
fn system_hostname() -> String {
    ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| APP.to_string())
}

/// A header field may not contain spaces or control characters (RFC 5424
/// §6: PRINTUSASCII); anything else would shift every field after it.
fn header_field(text: &str) -> String {
    text.chars().take(255).map(|c| if c.is_ascii_graphic() { c } else { '-' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Facility;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::TcpListener;

    fn sender(config: SyslogConfig) -> Sender {
        Sender { config, hostname: "sensor1".into(), udp: None, tcp: None, next_connect: None, failing: false }
    }

    fn message() -> Message {
        Message {
            time: "2026-11-07T11:20:40.409920Z".parse().unwrap(),
            kind: "login_attempt",
            text: r#"{"decoy":"telnet"}"#.into(),
        }
    }

    #[test]
    fn frames_rfc5424_with_the_kind_as_message_id() {
        let frame = sender(SyslogConfig::default()).frame(&message());
        // local0 (16) * 8 + warning (4) = 132
        assert_eq!(
            String::from_utf8(frame).unwrap(),
            r#"<132>1 2026-11-07T11:20:40.409Z sensor1 easydecoy - login_attempt - {"decoy":"telnet"}"#
        );
    }

    #[test]
    fn frames_rfc3164_newline_terminated_over_tcp() {
        let config = SyslogConfig {
            format: SyslogFormat::Rfc3164,
            protocol: SyslogProtocol::Tcp,
            facility: Facility::Auth,
            ..SyslogConfig::default()
        };
        let mut message = message();
        message.kind = "connect";
        // auth (4) * 8 + notice (5) = 37
        assert_eq!(
            String::from_utf8(sender(config).frame(&message)).unwrap(),
            "<37>Nov  7 11:20:40 sensor1 easydecoy: {\"decoy\":\"telnet\"}\n"
        );
    }

    #[test]
    fn a_hostname_cannot_break_the_header() {
        assert_eq!(header_field("my host\n"), "my-host-");
    }

    #[tokio::test]
    async fn delivers_over_udp() {
        let collector = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let config = SyslogConfig { server: collector.local_addr().unwrap().to_string(), ..SyslogConfig::default() };
        let tx = spawn(&config);
        tx.send(Utc::now(), "connect", "{}");

        let mut buf = [0u8; 512];
        let n = tokio::time::timeout(Duration::from_secs(2), collector.recv(&mut buf)).await.unwrap().unwrap();
        let got = String::from_utf8_lossy(&buf[..n]);
        assert!(got.starts_with("<133>1 "), "{got}");
        assert!(got.ends_with(" easydecoy - connect - {}"), "{got}");
    }

    #[tokio::test]
    async fn delivers_over_tcp_one_line_per_event() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = SyslogConfig {
            server: listener.local_addr().unwrap().to_string(),
            protocol: SyslogProtocol::Tcp,
            ..SyslogConfig::default()
        };
        let tx = spawn(&config);
        tx.send(Utc::now(), "connect", "{\"n\":1}");
        tx.send(Utc::now(), "login_attempt", "{\"n\":2}");

        let (stream, _) = listener.accept().await.unwrap();
        let mut lines = BufReader::new(stream).lines();
        let first = lines.next_line().await.unwrap().unwrap();
        let second = lines.next_line().await.unwrap().unwrap();
        assert!(first.ends_with(r#"connect - {"n":1}"#), "{first}");
        assert!(second.starts_with("<132>1 ") && second.ends_with(r#"{"n":2}"#), "{second}");
    }
}
