// =============================================================================
// decoy/telnet.rs — a fake Telnet login
//
// Greets a client the way a Linux host's telnetd does: the banner, then
// "<hostname> login:" and "Password:". Every login fails after a short pause,
// and after three tries the connection is closed. Each try is recorded as a
// login_attempt with the username and the password's length.
//
// The password itself is never stored. Anyone who reaches this prompt might be
// a colleague typing their real password into the wrong window, and the decoy
// learns all it needs from the fact that somebody tried.
//
// Lines are read with LineReader (decoy/lines.rs), which strips the option
// negotiation Telnet clients interleave with what the user types.
// =============================================================================

use std::time::Duration;

use anyhow::Result;

use super::lines::{IAC, LineReader};
use super::{Conn, Decoy};
use crate::event::clean;
use crate::settings::TelnetSettings;

const MAX_ATTEMPTS: usize = 3;
// A real login takes a moment to fail; an instant answer looks like a decoy.
const FAIL_DELAY: Duration = Duration::from_secs(2);
// Longest username recorded.
const MAX_FIELD: usize = 128;

// Telnet option negotiation (RFC 854, RFC 857): asking the client to stop
// echoing what is typed, and to start again.
const WILL: u8 = 251;
const WONT: u8 = 252;
const ECHO: u8 = 1;

pub struct Telnet {
    banner: String,
    hostname: String,
}

impl Telnet {
    pub fn new(config: &TelnetSettings) -> Self {
        Self { banner: config.banner.clone(), hostname: config.hostname.clone() }
    }
}

impl Decoy for Telnet {
    const NAME: &'static str = "telnet";

    async fn handle(&self, mut conn: Conn) -> Result<()> {
        let mut lines = LineReader::default();
        if !self.banner.is_empty() {
            conn.write(format!("{}\r\n", self.banner).as_bytes()).await?;
        }
        for _ in 0..MAX_ATTEMPTS {
            conn.write(format!("{} login: ", self.hostname).as_bytes()).await?;
            let Some(username) = lines.next(&mut conn).await? else { return Ok(()) };

            // Tell the client we will echo, so it stops showing what is typed,
            // as a real password prompt does.
            conn.write(&[IAC, WILL, ECHO]).await?;
            conn.write(b"Password: ").await?;
            let Some(password) = lines.next(&mut conn).await? else { return Ok(()) };
            conn.write(&[IAC, WONT, ECHO]).await?;

            conn.record(
                conn.event("login_attempt")
                    .with("username", clean(&username, MAX_FIELD))
                    .with("password_length", password.chars().count()),
            )
            .await;
            tokio::time::sleep(FAIL_DELAY).await;
            conn.write(b"\r\nLogin incorrect\r\n").await?;
        }
        Ok(())
    }
}
