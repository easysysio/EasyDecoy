// =============================================================================
// decoy/ssh.rs — a fake SSH server
//
// SSH is encrypted from the first packet after the version line, so unlike
// the other decoys this one can't just print prompts: it runs a real key
// exchange (russh) with a host key of its own, then refuses every login.
//
// It offers what a stock Ubuntu sshd offers, publickey and password, and
// records each try as a login_attempt:
//   - a password: the username and the password's length, never the password,
//     refused after the two seconds a real server's PAM takes to say no;
//   - a public key: the username, the key's type and its SHA-256 fingerprint,
//     refused at once, as OpenSSH refuses a key it doesn't know.
// After six refusals the client is disconnected, as OpenSSH's MaxAuthTries
// does. No login succeeds, so no channel, shell or forwarding is ever opened.
//
// The version line the client sends (its software, "SSH-2.0-OpenSSH_9.6") is
// recorded as a `handshake` event as soon as it arrives, so a scanner that
// reads the banner and leaves is seen too, and it is added to every login
// attempt.
//
// The host key is an Ed25519 key made on the first start and kept in the
// database, so the fingerprint a client remembers stays the same across
// restarts and upgrades. It is the decoy's own, never the host's.
// =============================================================================

use std::io;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use anyhow::{Context, Result};
use rand::RngCore;
use russh::keys::ssh_key::LineEnding;
use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::{HashAlg, PrivateKey};
use russh::server::{self, Auth, PublicKeyOffer};
use russh::{MethodKind, MethodSet, SshId};
use sqlx::SqlitePool;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use super::{Conn, Decoy, Recorder};
use crate::event::{Event, clean};
use crate::settings::SshSettings;

// OpenSSH's MaxAuthTries.
const MAX_ATTEMPTS: usize = 6;
// How long a wrong password takes to be refused: pam_unix's delay on Ubuntu.
const FAIL_DELAY: Duration = Duration::from_secs(2);
// Longest username and client version recorded. RFC 4253 caps the version
// line at 255 characters.
const MAX_USERNAME: usize = 128;
const MAX_VERSION: usize = 255;

const META_HOST_KEY: &str = "ssh_host_key";

pub struct Ssh {
    config: Arc<server::Config>,
}

impl Ssh {
    pub fn new(settings: &SshSettings, host_key: PrivateKey, idle: Duration) -> Self {
        let config = server::Config {
            // clean() keeps a line break in the version from ending the line
            // early (the GUI refuses one, but the setting could be older).
            server_id: SshId::Standard(format!("SSH-2.0-{}", clean(&settings.version, 200)).into()),
            methods: methods(),
            // The handler takes its own time to refuse a password; a key or
            // the "none" probe OpenSSH starts with is refused at once.
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            keys: vec![host_key],
            max_auth_attempts: MAX_ATTEMPTS,
            // Silence for this long ends the session, as on the other decoys.
            inactivity_timeout: Some(idle),
            nodelay: true,
            ..Default::default()
        };
        Self { config: Arc::new(config) }
    }
}

impl Decoy for Ssh {
    const NAME: &'static str = "ssh";

    async fn handle(&self, conn: Conn) -> Result<()> {
        let (stream, recorder) = conn.into_parts();
        // russh runs the session in a task of its own, which would outlive
        // this one when the session limit stops it. A second handle on the
        // socket lets dropping this future close the connection, and with it
        // that task.
        let stream = stream.into_std()?;
        let _closer = Closer(stream.try_clone()?);
        let stream = TcpStream::from_std(stream)?;

        let version = Arc::new(OnceLock::new());
        let tap = Tap { inner: stream, line: Vec::new(), version: version.clone() };
        let client = Client { recorder: recorder.clone(), version: version.clone() };
        // Returns once the client's version line has been read (or it never
        // came, or wasn't SSH).
        let running = server::run_stream(self.config.clone(), tap, client).await;
        if let Some(v) = version.get() {
            recorder.record(recorder.event("handshake").with("client", v.clone())).await;
        }
        running?.await?;
        Ok(())
    }
}

/// What the server offers: what a stock Ubuntu sshd does.
fn methods() -> MethodSet {
    MethodSet::from(&[MethodKind::PublicKey, MethodKind::Password][..])
}

/// A refusal that leaves both methods on offer, as OpenSSH's does. (russh
/// otherwise stops offering a password after the first wrong one, and a
/// client gives up rather than ask again.)
fn refuse() -> Auth {
    Auth::Reject { proceed_with_methods: Some(methods()), partial_success: false }
}

/// One client's session, as russh sees it.
struct Client {
    recorder: Recorder,
    version: Arc<OnceLock<String>>,
}

impl Client {
    fn attempt(&self, user: &str, method: &'static str) -> Event {
        let mut event =
            self.recorder.event("login_attempt").with("username", clean(user, MAX_USERNAME)).with("method", method);
        if let Some(v) = self.version.get() {
            event = event.with("client", v.clone());
        }
        event
    }
}

impl server::Handler for Client {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        let event = self.attempt(user, "password").with("password_length", password.chars().count());
        self.recorder.record(event).await;
        tokio::time::sleep(FAIL_DELAY).await;
        Ok(refuse())
    }

    async fn auth_publickey_offered(&mut self, user: &str, offer: PublicKeyOffer<'_>) -> Result<Auth, Self::Error> {
        let key = offer.public_key;
        let event = self
            .attempt(user, "publickey")
            .with("key_type", key.algorithm().as_str())
            .with("key_fingerprint", key.fingerprint(HashAlg::Sha256).to_string());
        self.recorder.record(event).await;
        Ok(refuse())
    }
}

/// Shuts the connection down when dropped.
struct Closer(std::net::TcpStream);

impl Drop for Closer {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tap
// The client's socket, as given to russh, keeping a copy of the first line
// the client sends: its version, "SSH-2.0-<software> <comments>". russh reads
// the line but doesn't hand it to the handler before the login.
// ─────────────────────────────────────────────────────────────────────────────
struct Tap {
    inner: TcpStream,
    line: Vec<u8>,
    version: Arc<OnceLock<String>>,
}

impl Tap {
    fn saw(&mut self, data: &[u8]) {
        if self.version.get().is_some() {
            return;
        }
        // Room for the longest line and its "\r\n"; past that it isn't SSH.
        let room = (MAX_VERSION + 2).saturating_sub(self.line.len());
        let end = data.iter().position(|b| *b == b'\n');
        let take = end.map_or(data.len(), |i| i + 1).min(room);
        self.line.extend_from_slice(&data[..take]);
        if end.is_some() || self.line.len() >= MAX_VERSION + 2 {
            let _ = self.version.set(version_text(&self.line));
            self.line = Vec::new();
        }
    }
}

/// The version line as recorded: without its line ending, made safe to show.
fn version_text(line: &[u8]) -> String {
    let text = String::from_utf8_lossy(line);
    clean(text.trim_end_matches(['\r', '\n']), MAX_VERSION)
}

impl AsyncRead for Tap {
    fn poll_read(self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = poll {
            this.saw(&buf.filled()[before..]);
        }
        poll
    }
}

impl AsyncWrite for Tap {
    fn poll_write(self: Pin<&mut Self>, cx: &mut TaskContext<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// host_key(db)
// The decoy's host key: the one in the database, or a new Ed25519 key, stored
// there on the first start.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn host_key(db: &SqlitePool) -> Result<PrivateKey> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM meta WHERE key = ?").bind(META_HOST_KEY).fetch_optional(db).await?;
    if let Some(pem) = stored {
        return PrivateKey::from_openssh(pem).context("the SSH decoy's stored host key cannot be read");
    }

    let mut seed = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    let key = PrivateKey::from(Ed25519Keypair::from_seed(&seed));
    let pem = key.to_openssh(LineEnding::LF).context("encoding the SSH decoy's host key")?;
    sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)")
        .bind(META_HOST_KEY)
        .bind(pem.as_str())
        .execute(db)
        .await?;
    tracing::info!("SSH decoy: made a host key, {}", fingerprint(&key));
    Ok(key)
}

/// The host key's fingerprint, as `ssh` shows it: "SHA256:...".
pub fn fingerprint(key: &PrivateKey) -> String {
    key.public_key().fingerprint(HashAlg::Sha256).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_is_recorded_without_its_ending() {
        assert_eq!(version_text(b"SSH-2.0-OpenSSH_9.6\r\n"), "SSH-2.0-OpenSSH_9.6");
        assert_eq!(version_text(b"SSH-2.0-Go\n"), "SSH-2.0-Go");
        assert_eq!(version_text(b"GET / HTTP/1.1\x1b\r\n"), "GET / HTTP/1.1.");
    }

    #[tokio::test]
    async fn the_host_key_is_made_once_and_kept() {
        let db = crate::db::memory().await;
        let first = host_key(&db).await.unwrap();
        let again = host_key(&db).await.unwrap();
        assert_eq!(fingerprint(&first), fingerprint(&again));
        assert!(fingerprint(&first).starts_with("SHA256:"));
        assert_eq!(first.algorithm().as_str(), "ssh-ed25519");
    }

    /// A client that trusts any host key: it is talking to the test's decoy.
    struct Trusting;

    impl russh::client::Handler for Trusting {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _: &russh::keys::PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn every_login_is_refused_and_recorded() {
        use russh::client::AuthResult;
        use russh::keys::PrivateKeyWithHashAlg;

        let db = crate::db::memory().await;
        let (store, _) = crate::settings::SettingsStore::load(db.clone()).await.unwrap();
        let (events, _writer) = crate::event::spawn_writer(db.clone(), store).await;
        let listener = super::super::bind("ssh", "127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = super::super::Shared {
            events,
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
            idle: Duration::from_secs(10),
            session: Duration::from_secs(30),
        };
        let ssh = Ssh::new(&SshSettings::default(), host_key(&db).await.unwrap(), shared.idle);
        let server = tokio::spawn(super::super::serve(Arc::new(ssh), listener, shared));

        let config = Arc::new(russh::client::Config::default());
        let mut client = russh::client::connect(config, addr, Trusting).await.unwrap();
        let tried = client.authenticate_password("root", "hunter2").await.unwrap();
        assert!(matches!(tried, AuthResult::Failure { .. }));
        // Still offered after a wrong one, so a client asks again.
        let AuthResult::Failure { remaining_methods, .. } = tried else { unreachable!() };
        assert!(remaining_methods.contains(&MethodKind::Password));

        let key = Arc::new(PrivateKey::from(Ed25519Keypair::from_seed(&[7; 32])));
        let print = fingerprint(&key);
        let tried = client.authenticate_publickey("deploy", PrivateKeyWithHashAlg::new(key, None)).await.unwrap();
        assert!(matches!(tried, AuthResult::Failure { .. }));
        drop(client);

        let mut lines: Vec<String> = Vec::new();
        for _ in 0..100 {
            lines = sqlx::query_scalar("SELECT line FROM events WHERE kind != 'connect' ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();
            if lines.len() >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        server.abort();
        let events: Vec<serde_json::Value> = lines.iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(events.len(), 3, "{lines:?}");
        assert_eq!(events[0]["kind"], "handshake");
        assert!(events[0]["client"].as_str().unwrap().starts_with("SSH-2.0-"));
        assert_eq!(events[1]["kind"], "login_attempt");
        assert_eq!(events[1]["method"], "password");
        assert_eq!(events[1]["username"], "root");
        assert_eq!(events[1]["password_length"], 7);
        assert!(!lines[1].contains("hunter2"), "the password itself is never recorded");
        assert_eq!(events[2]["method"], "publickey");
        assert_eq!(events[2]["username"], "deploy");
        assert_eq!(events[2]["key_type"], "ssh-ed25519");
        assert_eq!(events[2]["key_fingerprint"], print.as_str());
    }
}
