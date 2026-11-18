// =============================================================================
// web/auth.rs — who may use the GUI
//
// The first visit to a GUI with no accounts creates the administrator (setup);
// after that every page needs a signed-in session.
//
//   - Passwords are bcrypt hashes, checked off the async runtime. A username
//     that doesn't exist is checked against a dummy hash, so the time a reply
//     takes doesn't say which usernames exist.
//   - A session is a random token in an HttpOnly, Secure, SameSite=Strict
//     cookie; the database keeps only its SHA-256, and it ends after 12 hours.
//   - Every form behind the sign-in carries the session's CSRF token.
//   - Five failed sign-ins from one address in 15 minutes lock that address
//     out until they age off, and every failure costs a second.
//   - Sign-ins, failures and sign-outs go to the audit trail.
// =============================================================================

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Query, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::{Duration as Span, OffsetDateTime};
use tera::Context as Ctx;

use super::{AppState, WebError};
use crate::event::stamp;
use crate::store::audit;

const COOKIE: &str = "easydecoy_session";
const SESSION_HOURS: i64 = 12;
const MAX_FAILURES: usize = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);
const FAILURE_DELAY: Duration = Duration::from_secs(1);
pub const MIN_PASSWORD: usize = 10;
// bcrypt reads only the first 72 bytes; a longer password would be a promise
// it can't keep.
pub const MAX_PASSWORD_BYTES: usize = 72;

/// The signed-in user, put on every request behind the sign-in.
#[derive(Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub csrf: String,
    /// The SHA-256 of this session's token.
    pub session: String,
}

impl User {
    /// Whether a form's CSRF token is this session's, compared in constant time.
    pub fn csrf_ok(&self, token: &str) -> bool {
        let (a, b) = (self.csrf.as_bytes(), token.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

/// A form token that is not this session's.
pub fn forbidden() -> Response {
    (axum::http::StatusCode::FORBIDDEN, "This form has expired. Go back, reload the page and try again.")
        .into_response()
}

// ═════════════════════════════════════════════════════════════════════════════
// Tokens and passwords
// ═════════════════════════════════════════════════════════════════════════════

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

fn sha256(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn hash_password(password: String) -> anyhow::Result<String> {
    Ok(tokio::task::spawn_blocking(move || bcrypt::hash(password, bcrypt::DEFAULT_COST)).await??)
}

pub async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash).unwrap_or(false))
        .await
        .unwrap_or(false)
}

/// A hash no password matches, checked when the username doesn't exist.
fn dummy_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| bcrypt::hash(random_token(), bcrypt::DEFAULT_COST).unwrap_or_default()).clone()
}

/// What makes a new password unacceptable, or None.
pub fn password_problem(password: &str, confirm: &str) -> Option<String> {
    if password.chars().count() < MIN_PASSWORD {
        return Some(format!("The password must be at least {MIN_PASSWORD} characters."));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Some(format!("The password can be at most {MAX_PASSWORD_BYTES} bytes."));
    }
    if password != confirm {
        return Some("The two passwords don't match.".to_string());
    }
    None
}

// ═════════════════════════════════════════════════════════════════════════════
// Sessions
// ═════════════════════════════════════════════════════════════════════════════

async fn start_session(state: &AppState, user_id: i64, jar: CookieJar) -> Result<CookieJar, WebError> {
    let token = random_token();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, csrf, expires_at) VALUES (?, ?, ?, ?)")
        .bind(sha256(&token))
        .bind(user_id)
        .bind(random_token())
        .bind(stamp(OffsetDateTime::now_utc() + Span::hours(SESSION_HOURS)))
        .execute(&state.db)
        .await?;
    let cookie = Cookie::build((COOKIE, token))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        // No max-age: the browser forgets it when it closes, and the server
        // ends it after SESSION_HOURS either way.
        .path("/");
    Ok(jar.add(cookie))
}

async fn has_users(state: &AppState) -> Result<bool, WebError> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&state.db).await?;
    Ok(n > 0)
}

// ─────────────────────────────────────────────────────────────────────────────
// require_login
// Lets a request through only with a live session, putting its User on the
// request. Without one: to setup when there are no accounts yet, else to the
// sign-in page.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn require_login(
    State(state): State<AppState>,
    jar: CookieJar,
    mut request: Request,
    next: Next,
) -> Result<Response, WebError> {
    if let Some(cookie) = jar.get(COOKIE) {
        let hash = sha256(cookie.value());
        let row: Option<(i64, String, String)> = sqlx::query_as(
            "SELECT u.id, u.username, s.csrf FROM sessions s JOIN users u ON u.id = s.user_id \
             WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(&hash)
        .bind(stamp(OffsetDateTime::now_utc()))
        .fetch_optional(&state.db)
        .await?;
        if let Some((id, username, csrf)) = row {
            request.extensions_mut().insert(User { id, username, csrf, session: hash });
            return Ok(next.run(request).await);
        }
    }
    let to = if has_users(&state).await? { "/login" } else { "/setup" };
    Ok(Redirect::to(to).into_response())
}

// ═════════════════════════════════════════════════════════════════════════════
// The sign-in limiter
// ═════════════════════════════════════════════════════════════════════════════

/// Recent failed sign-ins, by client address.
#[derive(Default)]
pub struct Limiter(Mutex<HashMap<IpAddr, VecDeque<Instant>>>);

impl Limiter {
    fn locked_out(&self, ip: IpAddr) -> bool {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        map.retain(|_, times| {
            while times.front().is_some_and(|t| now.duration_since(*t) > FAILURE_WINDOW) {
                times.pop_front();
            }
            !times.is_empty()
        });
        map.get(&ip).is_some_and(|times| times.len() >= MAX_FAILURES)
    }

    fn failed(&self, ip: IpAddr) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let times = map.entry(ip).or_default();
        times.push_back(Instant::now());
        // Only the most recent failures matter.
        while times.len() > MAX_FAILURES {
            times.pop_front();
        }
    }

    fn succeeded(&self, ip: IpAddr) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&ip);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Handlers
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
    #[serde(default)]
    confirm: String,
}

fn login_ctx(setup: bool, username: &str, error: Option<&str>) -> Ctx {
    let mut ctx = Ctx::new();
    ctx.insert("setup", &setup);
    ctx.insert("username", username);
    ctx.insert("error", &error);
    ctx.insert("min_password", &MIN_PASSWORD);
    ctx
}

#[derive(Deserialize)]
pub struct LoginQuery {
    #[serde(default)]
    signed_out: bool,
}

pub async fn login_page(State(state): State<AppState>, Query(q): Query<LoginQuery>) -> Result<Response, WebError> {
    if !has_users(&state).await? {
        return Ok(Redirect::to("/setup").into_response());
    }
    let mut ctx = login_ctx(false, "", None);
    ctx.insert("signed_out", &q.signed_out);
    Ok(state.render("login.html", ctx)?.into_response())
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<Credentials>,
) -> Result<Response, WebError> {
    let ip = peer.ip().to_canonical();
    let client = ip.to_string();
    let username = form.username.trim();
    let shown = crate::event::clean(username, 64);

    if state.limiter.locked_out(ip) {
        audit(&state.db, &shown, &client, "sign_in_blocked", "too many failed sign-ins").await;
        tokio::time::sleep(FAILURE_DELAY).await;
        let error = "Too many failed sign-ins from your address. Try again in 15 minutes.";
        return Ok(state.render("login.html", login_ctx(false, username, Some(error)))?.into_response());
    }

    let row: Option<(i64, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&state.db)
        .await?;
    let (id, hash) = match &row {
        Some((id, hash)) => (Some(*id), hash.clone()),
        None => (None, dummy_hash()),
    };
    let ok = verify_password(form.password, hash).await && id.is_some();

    match (ok, id) {
        (true, Some(id)) => {
            state.limiter.succeeded(ip);
            audit(&state.db, username, &client, "sign_in", "").await;
            let jar = start_session(&state, id, jar).await?;
            Ok((jar, Redirect::to("/")).into_response())
        }
        _ => {
            state.limiter.failed(ip);
            audit(&state.db, &shown, &client, "sign_in_failed", "").await;
            tokio::time::sleep(FAILURE_DELAY).await;
            let error = "Wrong username or password.";
            Ok(state.render("login.html", login_ctx(false, username, Some(error)))?.into_response())
        }
    }
}

pub async fn setup_page(State(state): State<AppState>) -> Result<Response, WebError> {
    if has_users(&state).await? {
        return Ok(Redirect::to("/login").into_response());
    }
    Ok(state.render("login.html", login_ctx(true, "admin", None))?.into_response())
}

pub async fn setup(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<Credentials>,
) -> Result<Response, WebError> {
    let username = form.username.trim();
    let problem = if username.is_empty() || username.chars().count() > 64 || username.chars().any(char::is_control) {
        Some("The username must be 1 to 64 characters, without line breaks.".to_string())
    } else {
        password_problem(&form.password, &form.confirm)
    };
    if let Some(error) = problem {
        return Ok(state.render("login.html", login_ctx(true, username, Some(&error)))?.into_response());
    }

    let hash = hash_password(form.password).await?;
    // Only while there are no accounts: two first visits racing each other
    // can't both create one.
    let created = sqlx::query(
        "INSERT INTO users (username, password_hash, created_at) \
         SELECT ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(username)
    .bind(hash)
    .bind(stamp(OffsetDateTime::now_utc()))
    .execute(&state.db)
    .await?;
    if created.rows_affected() == 0 {
        return Ok(Redirect::to("/login").into_response());
    }
    let id = created.last_insert_rowid();
    audit(&state.db, username, &peer.ip().to_canonical().to_string(), "setup", "created the administrator").await;
    let jar = start_session(&state, id, jar).await?;
    Ok((jar, Redirect::to("/")).into_response())
}

#[derive(Deserialize)]
pub struct CsrfForm {
    #[serde(default)]
    csrf: String,
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<CsrfForm>,
) -> Result<Response, WebError> {
    if !user.csrf_ok(&form.csrf) {
        return Ok(forbidden());
    }
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?").bind(&user.session).execute(&state.db).await?;
    audit(&state.db, &user.username, &peer.ip().to_canonical().to_string(), "sign_out", "").await;
    let jar = jar.remove(Cookie::build(COOKIE).path("/"));
    Ok((jar, Redirect::to("/login?signed_out=true")).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_tokens_must_match_exactly() {
        let user = User { id: 1, username: "a".into(), csrf: "abc123".into(), session: String::new() };
        assert!(user.csrf_ok("abc123"));
        assert!(!user.csrf_ok("abc124"));
        assert!(!user.csrf_ok("abc12"));
        assert!(!user.csrf_ok(""));
    }

    #[test]
    fn five_failures_lock_an_address_out() {
        let limiter = Limiter::default();
        let ip: IpAddr = "10.0.0.5".parse().unwrap();
        for _ in 0..MAX_FAILURES {
            assert!(!limiter.locked_out(ip));
            limiter.failed(ip);
        }
        assert!(limiter.locked_out(ip));
        assert!(!limiter.locked_out("10.0.0.6".parse().unwrap()), "only that address");
    }

    #[test]
    fn passwords_have_rules() {
        assert!(password_problem("short", "short").is_some());
        assert!(password_problem("long enough!", "long enough?").is_some());
        assert!(password_problem(&"x".repeat(73), &"x".repeat(73)).is_some());
        assert!(password_problem("long enough!", "long enough!").is_none());
    }
}
