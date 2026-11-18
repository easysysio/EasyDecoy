// =============================================================================
// decoy/ftp.rs — a fake FTP server
//
// Answers the way vsftpd does with anonymous logins turned off: a "220"
// greeting, then USER and PASS, and every login refused with "530 Login
// incorrect." after a short pause. After three failed logins the connection is
// closed, as vsftpd's max_login_fails does. Each try is recorded as a
// login_attempt with the username and the password's length, never the
// password. "anonymous" is refused like any other name.
//
// A client never gets past the login, so there is no data connection and no
// file to read: every command that would need a login is answered "530 Please
// login with USER and PASS.". Commands a client sends while finding its way in
// (FEAT, SYST, AUTH TLS, ...) are answered and not recorded; anything else,
// such as SITE, RETR or PORT sent without a login, is recorded as a `command`
// event, since an ordinary client has no reason to send it.
// =============================================================================

use std::time::Duration;

use anyhow::Result;

use super::lines::LineReader;
use super::{Conn, Decoy};
use crate::event::clean;
use crate::settings::FtpSettings;

const MAX_ATTEMPTS: usize = 3;
// vsftpd's delay_failed_login: a failure takes a second to arrive.
const FAIL_DELAY: Duration = Duration::from_secs(1);
// Commands answered before the connection is closed, whatever they are.
const MAX_COMMANDS: usize = 50;
// Longest username, command and argument recorded.
const MAX_USERNAME: usize = 128;
const MAX_VERB: usize = 16;
const MAX_ARGUMENT: usize = 256;

// What vsftpd 3 answers to FEAT before a login.
const FEATURES: &str = "211-Features:\r\n EPRT\r\n EPSV\r\n MDTM\r\n PASV\r\n REST STREAM\r\n SIZE\r\n TVFS\r\n211 End\r\n";

// Commands every client sends on the way to a login. Answered, not recorded.
const ROUTINE: [&str; 9] = ["AUTH", "FEAT", "HELP", "NOOP", "OPTS", "PBSZ", "PROT", "QUIT", "SYST"];

pub struct Ftp {
    greeting: String,
}

impl Ftp {
    pub fn new(config: &FtpSettings) -> Self {
        // clean() keeps a line break in the banner from ending the reply early.
        Self { greeting: format!("220 {}\r\n", clean(&config.banner, 200)) }
    }
}

impl Decoy for Ftp {
    const NAME: &'static str = "ftp";

    async fn handle(&self, mut conn: Conn) -> Result<()> {
        let mut lines = LineReader::default();
        let mut user: Option<String> = None;
        let mut failures = 0;

        conn.write(self.greeting.as_bytes()).await?;
        for _ in 0..MAX_COMMANDS {
            let Some(line) = lines.next(&mut conn).await? else { return Ok(()) };
            let (verb, argument) = split(&line);

            if !verb.is_empty() && !ROUTINE.contains(&verb.as_str()) && verb != "USER" && verb != "PASS" {
                let mut event = conn.event("command").with("command", clean(&verb, MAX_VERB));
                if !argument.is_empty() {
                    event = event.with("argument", clean(argument, MAX_ARGUMENT));
                }
                conn.record(event).await;
            }

            let reply: &str = match verb.as_str() {
                "USER" => {
                    user = Some(argument.to_string());
                    "331 Please specify the password.\r\n"
                }
                "PASS" => {
                    let Some(username) = user.take() else {
                        conn.write(b"503 Login with USER first.\r\n").await?;
                        continue;
                    };
                    conn.record(
                        conn.event("login_attempt")
                            .with("username", clean(&username, MAX_USERNAME))
                            .with("password_length", argument.chars().count()),
                    )
                    .await;
                    tokio::time::sleep(FAIL_DELAY).await;
                    conn.write(b"530 Login incorrect.\r\n").await?;
                    failures += 1;
                    if failures >= MAX_ATTEMPTS {
                        return Ok(());
                    }
                    continue;
                }
                "QUIT" => {
                    conn.write(b"221 Goodbye.\r\n").await?;
                    return Ok(());
                }
                "FEAT" => FEATURES,
                "NOOP" => "200 NOOP ok.\r\n",
                "OPTS" if argument.eq_ignore_ascii_case("UTF8 ON") => "200 Always in UTF8 mode.\r\n",
                "OPTS" => "501 Option not understood.\r\n",
                "" => "500 Unknown command.\r\n",
                // vsftpd without TLS answers AUTH like any command before a login.
                _ => "530 Please login with USER and PASS.\r\n",
            };
            conn.write(reply.as_bytes()).await?;
        }
        // vsftpd's answer when a client won't stop.
        conn.write(b"421 Timeout.\r\n").await?;
        Ok(())
    }
}

/// A command line split into its verb (upper case) and the rest.
fn split(line: &str) -> (String, &str) {
    let line = line.trim_start();
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    (verb.to_ascii_uppercase(), rest.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_split_into_verb_and_argument() {
        assert_eq!(split("user admin"), ("USER".to_string(), "admin"));
        assert_eq!(split("PASS  my secret "), ("PASS".to_string(), "my secret"));
        assert_eq!(split("FEAT"), ("FEAT".to_string(), ""));
        assert_eq!(split(""), (String::new(), ""));
    }

    #[test]
    fn a_banner_cannot_add_a_reply_line() {
        let ftp = Ftp::new(&FtpSettings { banner: "(vsFTPd)\r\n230 ok".into(), ..FtpSettings::default() });
        assert_eq!(ftp.greeting, "220 (vsFTPd)..230 ok\r\n");
    }
}
