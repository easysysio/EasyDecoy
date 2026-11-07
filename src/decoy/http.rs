// =============================================================================
// decoy/http.rs — a fake web administration panel
//
// Answers like a small appliance's web interface: / and /login serve a sign-in
// page, any other path is a 404, and a submitted sign-in always fails. Each
// request is recorded with its method, path, Host and User-Agent; a sign-in is
// recorded as a login_attempt with the username and the password's length.
//
// The password itself is never stored, for the same reason as the Telnet
// decoy's: whoever typed it may have typed their real one.
//
// One request per connection, read under fixed limits: a request head over
// MAX_HEAD or a body over MAX_BODY is cut off rather than buffered.
// =============================================================================

use std::time::Duration;

use anyhow::Result;

use super::{Conn, Decoy};
use crate::config::HttpConfig;
use crate::event::clean;

const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 4 * 1024;
// A real sign-in takes a moment to fail; an instant answer looks like a decoy.
const FAIL_DELAY: Duration = Duration::from_secs(1);
// Bytes of a non-HTTP request kept as a sample.
const MAX_SAMPLE: usize = 64;

// Form fields a sign-in form might name its username and password.
const USER_FIELDS: &[&str] = &["username", "user", "login", "email"];
const PASSWORD_FIELDS: &[&str] = &["password", "pass", "pwd"];

const NOT_FOUND: &str = "<html>\r\n<head><title>404 Not Found</title></head>\r\n\
<body>\r\n<center><h1>404 Not Found</h1></center>\r\n</body>\r\n</html>\r\n";

pub struct Http {
    server: String,
    title: String,
}

impl Http {
    pub fn new(config: &HttpConfig) -> Self {
        // clean() keeps a CR or LF in the config from breaking the header block.
        Self { server: clean(&config.server_header, 128), title: escape_html(&config.title) }
    }

    fn login_page(&self, failed: bool) -> String {
        let title = &self.title;
        let error = if failed { r#"<p class="error">Invalid username or password.</p>"# } else { "" };
        format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{title}</title>
<style>
body{{font-family:sans-serif;background:#eef0f3;margin:0}}
form{{width:300px;margin:120px auto;padding:28px;background:#fff;border-radius:6px;box-shadow:0 1px 4px #0002}}
h2{{margin-top:0}}input{{display:block;width:100%;margin:6px 0 16px;padding:8px;box-sizing:border-box}}
button{{width:100%;padding:9px}}.error{{color:#b00}}
</style></head>
<body><form method="post" action="/login"><h2>{title}</h2>{error}
<label>Username<input name="username" autocomplete="username"></label>
<label>Password<input name="password" type="password" autocomplete="current-password"></label>
<button type="submit">Sign in</button></form></body></html>
"#
        )
    }
}

impl Decoy for Http {
    const NAME: &'static str = "http";

    async fn handle(&self, mut conn: Conn) -> Result<()> {
        // Read until the end of the request head.
        let mut buf = Vec::with_capacity(1024);
        let head_end = loop {
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
            let mut chunk = [0u8; 1024];
            let n = conn.read(&mut chunk).await?;
            if n == 0 || buf.len() >= MAX_HEAD {
                // Closed early, or never sent a complete head: probably not a
                // browser. Keep a sample of what it did send.
                if !buf.is_empty() {
                    conn.record(conn.event("non_http").with("sample", sample(&buf))).await;
                }
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
        };

        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let Some(request) = Request::parse(&head) else {
            conn.record(conn.event("non_http").with("sample", sample(&buf))).await;
            return Ok(());
        };

        // The body, up to Content-Length and never more than MAX_BODY.
        let want = request
            .header("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(MAX_BODY);
        let mut body = buf[head_end..].to_vec();
        while body.len() < want {
            let mut chunk = [0u8; 1024];
            let n = conn.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(want);

        let mut event = conn
            .event("request")
            .with("method", clean(&request.method, 16))
            .with("path", clean(&request.target, 512));
        if let Some(host) = request.header("host") {
            event = event.with("host", clean(host, 256));
        }
        if let Some(agent) = request.header("user-agent") {
            event = event.with("user_agent", clean(agent, 256));
        }

        let path = request.target.split('?').next().unwrap_or("");
        let (status, page) = match (request.method.as_str(), path) {
            ("POST", "/login") => {
                let form = String::from_utf8_lossy(&body);
                event.kind = "login_attempt";
                if let Some(user) = form_field(&form, USER_FIELDS) {
                    event = event.with("username", clean(&user, 128));
                }
                if let Some(password) = form_field(&form, PASSWORD_FIELDS) {
                    event = event.with("password_length", password.chars().count());
                }
                ("200 OK", self.login_page(true))
            }
            (_, "/" | "/login") => ("200 OK", self.login_page(false)),
            _ => ("404 Not Found", NOT_FOUND.to_string()),
        };
        let failed_login = event.kind == "login_attempt";
        conn.record(event).await;
        if failed_login {
            tokio::time::sleep(FAIL_DELAY).await;
        }

        let server = if self.server.is_empty() { String::new() } else { format!("Server: {}\r\n", self.server) };
        let response = format!(
            "HTTP/1.1 {status}\r\n{server}Content-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            page.len()
        );
        conn.write(response.as_bytes()).await?;
        if request.method != "HEAD" {
            conn.write(page.as_bytes()).await?;
        }
        Ok(())
    }
}

/// The parts of an HTTP request head the decoy looks at.
#[derive(Debug)]
struct Request {
    method: String,
    target: String,
    /// Header names lowercased.
    headers: Vec<(String, String)>,
}

impl Request {
    /// None when the head does not start with an HTTP request line.
    fn parse(head: &str) -> Option<Self> {
        let mut lines = head.split("\r\n");
        let mut parts = lines.next()?.split(' ');
        let method = parts.next()?;
        let target = parts.next()?;
        let version = parts.next()?;
        if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) || !version.starts_with("HTTP/") {
            return None;
        }
        let headers = lines
            .take_while(|line| !line.is_empty())
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
            .collect();
        Some(Self { method: method.to_string(), target: target.to_string(), headers })
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// The value of the first field in a urlencoded form named one of `names`.
fn form_field(form: &str, names: &[&str]) -> Option<String> {
    form.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        names.contains(&percent_decode(name).as_str()).then(|| percent_decode(value))
    })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (bytes[i], hex_pair(bytes.get(i + 1..i + 3))) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_pair(pair: Option<&[u8]>) -> Option<u8> {
    let pair = pair?;
    if !pair.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The start of what a client sent, readable in a log.
fn sample(buf: &[u8]) -> String {
    clean(&String::from_utf8_lossy(&buf[..buf.len().min(MAX_SAMPLE)]), MAX_SAMPLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_request_head() {
        let request = Request::parse(
            "GET /admin?x=1 HTTP/1.1\r\nHost: 10.0.0.9\r\nUser-Agent: curl/8.7\r\n\r\n",
        )
        .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.target, "/admin?x=1");
        assert_eq!(request.header("user-agent"), Some("curl/8.7"));
        assert_eq!(request.header("cookie"), None);
    }

    #[test]
    fn rejects_what_is_not_http() {
        assert!(Request::parse("SSH-2.0-OpenSSH_9.6\r\n\r\n").is_none());
        assert!(Request::parse("get / HTTP/1.1\r\n\r\n").is_none());
    }

    #[test]
    fn reads_form_fields() {
        let form = "user=admin%40corp&pass=hunter+2%21";
        assert_eq!(form_field(form, USER_FIELDS).as_deref(), Some("admin@corp"));
        assert_eq!(form_field(form, PASSWORD_FIELDS).as_deref(), Some("hunter 2!"));
        assert_eq!(form_field("a=1", USER_FIELDS), None);
    }

    #[test]
    fn a_broken_escape_is_kept_as_text() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn the_title_cannot_inject_markup() {
        let config = HttpConfig { title: "<script>".into(), ..HttpConfig::default() };
        assert!(!Http::new(&config).login_page(false).contains("<script>"));
    }
}
