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
// Telnet clients interleave option negotiation (IAC sequences) with what the
// user types. LineReader strips those as bytes arrive, so a sequence split
// across two reads is handled the same as one that arrives whole.
// =============================================================================

use std::time::Duration;

use anyhow::Result;

use super::{Conn, Decoy};
use crate::config::TelnetConfig;
use crate::event::clean;

const MAX_ATTEMPTS: usize = 3;
// A real login takes a moment to fail; an instant answer looks like a decoy.
const FAIL_DELAY: Duration = Duration::from_secs(2);
// Longest username recorded.
const MAX_FIELD: usize = 128;
// Unterminated input kept before the rest of the line is discarded.
const MAX_PENDING: usize = 1024;

// Telnet protocol bytes (RFC 854, RFC 857).
const IAC: u8 = 255;
const SB: u8 = 250;
const SE: u8 = 240;
const WILL: u8 = 251;
const WONT: u8 = 252;
const DONT: u8 = 254;
const ECHO: u8 = 1;

pub struct Telnet {
    banner: String,
    hostname: String,
}

impl Telnet {
    pub fn new(config: &TelnetConfig) -> Self {
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

/// Where LineReader is in the Telnet byte stream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Ordinary text.
    #[default]
    Data,
    /// After IAC: the next byte is a command.
    Command,
    /// After IAC WILL/WONT/DO/DONT: the next byte is the option.
    Option,
    /// Inside a subnegotiation (IAC SB ... IAC SE).
    Sub,
    /// After IAC inside a subnegotiation.
    SubIac,
}

/// Turns the bytes a Telnet client sends into the lines the user typed.
#[derive(Debug, Default)]
struct LineReader {
    text: Vec<u8>,
    state: State,
    // A line ended with CR; the client may follow it with LF or NUL.
    after_cr: bool,
}

impl LineReader {
    /// The next line typed, or None once the client is gone.
    async fn next(&mut self, conn: &mut Conn) -> Result<Option<String>> {
        loop {
            if let Some(line) = self.take_line() {
                return Ok(Some(line));
            }
            let mut chunk = [0u8; 512];
            let n = conn.read(&mut chunk).await?;
            if n == 0 {
                return Ok(None);
            }
            self.feed(&chunk[..n]);
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Data, IAC) => State::Command,
                (State::Data, _) => {
                    self.push(b);
                    State::Data
                }
                // IAC IAC is a literal 255.
                (State::Command, IAC) => {
                    self.push(b);
                    State::Data
                }
                (State::Command, WILL..=DONT) => State::Option,
                (State::Command, SB) => State::Sub,
                (State::Command, _) => State::Data,
                (State::Option, _) => State::Data,
                (State::Sub, IAC) => State::SubIac,
                (State::Sub, _) => State::Sub,
                (State::SubIac, SE) => State::Data,
                (State::SubIac, _) => State::Sub,
            };
        }
    }

    fn push(&mut self, b: u8) {
        let after_cr = std::mem::replace(&mut self.after_cr, b == b'\r');
        if after_cr && (b == b'\n' || b == 0) {
            return;
        }
        if b == b'\r' || b == b'\n' || self.text.len() < MAX_PENDING {
            self.text.push(b);
        }
    }

    fn take_line(&mut self) -> Option<String> {
        let end = self.text.iter().position(|&b| b == b'\r' || b == b'\n')?;
        let line = String::from_utf8_lossy(&self.text[..end]).into_owned();
        self.text.drain(..=end);
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(chunks: &[&[u8]]) -> Vec<String> {
        let mut reader = LineReader::default();
        let mut out = Vec::new();
        for chunk in chunks {
            reader.feed(chunk);
            while let Some(line) = reader.take_line() {
                out.push(line);
            }
        }
        out
    }

    #[test]
    fn line_endings_are_crlf_crnul_or_lf() {
        assert_eq!(lines(&[b"root\r\nadmin\r\0guest\n"]), ["root", "admin", "guest"]);
    }

    #[test]
    fn an_empty_line_is_still_a_line() {
        assert_eq!(lines(&[b"\r\n", b"root\r\n"]), ["", "root"]);
    }

    #[test]
    fn negotiation_is_stripped_even_when_split_across_reads() {
        // IAC DO ECHO, then IAC SB TTYPE ... IAC SE split mid-sequence.
        assert_eq!(
            lines(&[b"\xff\xfd\x01\xff\xfa\x18\x00xt", b"erm\xff\xf0ro", b"ot\r", b"\npw\r\n"]),
            ["root", "pw"]
        );
    }

    #[test]
    fn an_endless_line_is_cut_off_not_buffered() {
        let long = vec![b'a'; MAX_PENDING * 4];
        let got = lines(&[&long, b"\r\n"]);
        assert_eq!(got[0].len(), MAX_PENDING);
    }
}
