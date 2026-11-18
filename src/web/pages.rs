// =============================================================================
// web/pages.rs — the pages behind the sign-in
//
//   /            the dashboard: totals, a timeline, top sources, usernames and
//                paths, the decoys' status and the latest events
//   /events      every event, filtered and paged; /events/export streams the
//                same selection as JSON lines
//   /decoys      each decoy's status and settings
//   /settings    limits, recording (retention, events file) and syslog
//   /account     the password, and the audit trail
//
// A save checks the whole of the new settings (Settings::check) before
// storing them. A rejected form is shown again with what was typed and why;
// a saved one redirects, so reloading the page doesn't post it twice.
// =============================================================================

use std::net::SocketAddr;

use axum::Extension;
use axum::body::Body;
use axum::extract::{ConnectInfo, Form, Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Redirect, Response};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use tera::Context as Ctx;

use super::auth::{self, User, forbidden};
use super::{AppState, WebError};
use crate::settings::{Facility, Settings, SyslogFormat, SyslogProtocol};
use crate::store::{self, Filter, Range};
use crate::supervisor::Status;

const PER_PAGE: u32 = 50;

/// What every page behind the sign-in needs.
fn page(user: &User, active: &str) -> Ctx {
    let mut ctx = Ctx::new();
    ctx.insert("user", &user.username);
    ctx.insert("csrf", &user.csrf);
    ctx.insert("active", active);
    ctx
}

fn client(peer: SocketAddr) -> String {
    peer.ip().to_canonical().to_string()
}

#[derive(Serialize)]
struct DecoyStatus {
    name: &'static str,
    label: &'static str,
    icon: &'static str,
    bind: String,
    #[serde(flatten)]
    status: Status,
}

fn statuses(state: &AppState, s: &Settings) -> Vec<DecoyStatus> {
    vec![
        DecoyStatus {
            name: "telnet",
            label: "Telnet",
            icon: "bi-terminal",
            bind: s.telnet.bind.clone(),
            status: state.status.get("telnet"),
        },
        DecoyStatus {
            name: "http",
            label: "HTTP",
            icon: "bi-globe2",
            bind: s.http.bind.clone(),
            status: state.status.get("http"),
        },
    ]
}

// ═════════════════════════════════════════════════════════════════════════════
// Dashboard
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RangeQuery {
    range: Range,
}

pub async fn dashboard(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(q): Query<RangeQuery>,
) -> Result<Html<String>, WebError> {
    let summary = store::summary(&state.db, q.range).await?;
    let (latest, _) = store::list(&state.db, &Filter::default(), 10).await?;
    let mut ctx = page(&user, "dashboard");
    ctx.insert("range", &q.range);
    ctx.insert("summary", &summary);
    ctx.insert("latest", &latest);
    ctx.insert("decoys", &statuses(&state, &state.settings.current()));
    // How often the timeline's axis is labelled.
    ctx.insert("label_every", &match q.range {
        Range::Day => 4,
        Range::Week => 1,
        Range::Month => 5,
    });
    ctx.insert("granularity", if q.range == Range::Day { "hour" } else { "day" });
    state.render("dashboard.html", ctx)
}

// ═════════════════════════════════════════════════════════════════════════════
// Events
// ═════════════════════════════════════════════════════════════════════════════

/// The events page's address for `filter` at `page`.
fn events_url(path: &str, filter: &Filter, page: Option<u32>) -> String {
    let mut parts = Vec::new();
    for (key, value) in [("decoy", &filter.decoy), ("kind", &filter.kind), ("src", &filter.src), ("q", &filter.q)] {
        if !value.trim().is_empty() {
            parts.push(format!("{key}={}", encode(value.trim())));
        }
    }
    if let Some(page) = page.filter(|p| *p > 0) {
        parts.push(format!("page={page}"));
    }
    if parts.is_empty() { path.to_string() } else { format!("{path}?{}", parts.join("&")) }
}

/// Percent-encodes a query value.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub async fn events(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(filter): Query<Filter>,
) -> Result<Html<String>, WebError> {
    let (rows, total) = store::list(&state.db, &filter, PER_PAGE).await?;
    let pages = ((total + PER_PAGE as i64 - 1) / PER_PAGE as i64).max(1);
    let mut ctx = page(&user, "events");
    ctx.insert("rows", &rows);
    ctx.insert("total", &total);
    ctx.insert("filter", &filter);
    ctx.insert("page", &(filter.page + 1));
    ctx.insert("pages", &pages);
    ctx.insert("filtered", &!events_url("", &filter, None).is_empty());
    let prev = (filter.page > 0).then(|| events_url("/events", &filter, Some(filter.page - 1)));
    let next = ((filter.page as i64 + 1) < pages).then(|| events_url("/events", &filter, Some(filter.page + 1)));
    ctx.insert("prev_url", &prev);
    ctx.insert("next_url", &next);
    ctx.insert("export_url", &events_url("/events/export", &filter, None));
    ctx.insert("kinds", &["connect", "login_attempt", "request", "non_http"]);
    state.render("events.html", ctx)
}

pub async fn export(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(mut filter): Query<Filter>,
) -> Response {
    filter.page = 0;
    let what = events_url("", &filter, None);
    store::audit(&state.db, &user.username, &client(peer), "export_events", what.trim_start_matches('?')).await;
    let format = time::macros::format_description!("[year][month][day]-[hour][minute][second]");
    let now = time::OffsetDateTime::now_utc().format(format).unwrap_or_default();
    let name = format!("easydecoy-events-{now}.jsonl");
    let stream = store::export(state.db.clone(), filter).map_err(std::io::Error::other);
    (
        [
            (header::CONTENT_TYPE, "application/x-ndjson".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

// ═════════════════════════════════════════════════════════════════════════════
// Decoys
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Saved {
    saved: String,
}

/// The decoys page, with `s` in its forms and maybe an error for one of them.
fn decoys_page(state: &AppState, user: &User, s: &Settings, saved: &str, error: Option<(&str, String)>) -> Result<Html<String>, WebError> {
    let mut ctx = page(user, "decoys");
    ctx.insert("s", s);
    ctx.insert("decoys", &statuses(state, &state.settings.current()));
    ctx.insert("saved", saved);
    ctx.insert("error_for", &error.as_ref().map(|(f, _)| *f));
    ctx.insert("error", &error.map(|(_, e)| e));
    ctx.insert("gui", &state.gui.to_string());
    state.render("decoys.html", ctx)
}

pub async fn decoys(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(q): Query<Saved>,
) -> Result<Html<String>, WebError> {
    decoys_page(&state, &user, &state.settings.current(), &q.saved, None)
}

#[derive(Deserialize)]
pub struct TelnetForm {
    csrf: String,
    enabled: Option<String>,
    bind: String,
    banner: String,
    hostname: String,
}

#[derive(Deserialize)]
pub struct HttpForm {
    csrf: String,
    enabled: Option<String>,
    bind: String,
    server_header: String,
    title: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// save(state, user, peer, new, action, sections)
// Checks and stores `new`, the `sections` of which a form changed, and
// records `action` in the audit trail with what those sections now hold.
// Returns the problem, for the form to show, if there is one.
// ─────────────────────────────────────────────────────────────────────────────
async fn save(
    state: &AppState,
    user: &User,
    peer: SocketAddr,
    new: Settings,
    action: &str,
    sections: &[&str],
) -> Result<Option<String>, WebError> {
    if let Err(problem) = new.check(state.gui) {
        return Ok(Some(problem));
    }
    let old = state.settings.current();
    if new.recording.events_file.trim() != old.recording.events_file.trim()
        && let Err(problem) = crate::event::check_events_file(&new.recording.events_file).await
    {
        return Ok(Some(format!("EasyDecoy cannot write the events file: {problem}")));
    }
    if new == *old {
        return Ok(None);
    }
    let all = serde_json::to_value(&new)?;
    let detail: serde_json::Map<String, serde_json::Value> =
        sections.iter().filter_map(|k| all.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
    state.settings.save(new).await?;
    store::audit(&state.db, &user.username, &client(peer), action, &serde_json::Value::Object(detail).to_string()).await;
    Ok(None)
}

pub async fn save_telnet(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<TelnetForm>,
) -> Result<Response, WebError> {
    if !user.csrf_ok(&form.csrf) {
        return Ok(forbidden());
    }
    let mut new = (*state.settings.current()).clone();
    new.telnet.enabled = form.enabled.is_some();
    new.telnet.bind = form.bind.trim().to_string();
    new.telnet.banner = form.banner.trim().to_string();
    new.telnet.hostname = form.hostname.trim().to_string();
    match save(&state, &user, peer, new.clone(), "save_telnet", &["telnet"]).await? {
        None => Ok(Redirect::to("/decoys?saved=telnet").into_response()),
        Some(problem) => Ok(decoys_page(&state, &user, &new, "", Some(("telnet", problem)))?.into_response()),
    }
}

pub async fn save_http(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<HttpForm>,
) -> Result<Response, WebError> {
    if !user.csrf_ok(&form.csrf) {
        return Ok(forbidden());
    }
    let mut new = (*state.settings.current()).clone();
    new.http.enabled = form.enabled.is_some();
    new.http.bind = form.bind.trim().to_string();
    new.http.server_header = form.server_header.trim().to_string();
    new.http.title = form.title.trim().to_string();
    match save(&state, &user, peer, new.clone(), "save_http", &["http"]).await? {
        None => Ok(Redirect::to("/decoys?saved=http").into_response()),
        Some(problem) => Ok(decoys_page(&state, &user, &new, "", Some(("http", problem)))?.into_response()),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Settings
// ═════════════════════════════════════════════════════════════════════════════

fn settings_page(state: &AppState, user: &User, s: &Settings, saved: &str, error: Option<(&str, String)>) -> Result<Html<String>, WebError> {
    let mut ctx = page(user, "settings");
    ctx.insert("s", s);
    ctx.insert("saved", saved);
    ctx.insert("error_for", &error.as_ref().map(|(f, _)| *f));
    ctx.insert("error", &error.map(|(_, e)| e));
    ctx.insert("facilities", &[
        "user", "daemon", "auth", "authpriv", "local0", "local1", "local2", "local3", "local4", "local5", "local6",
        "local7",
    ]);
    ctx.insert("db_path", &state.db_file);
    state.render("settings.html", ctx)
}

pub async fn settings(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(q): Query<Saved>,
) -> Result<Html<String>, WebError> {
    settings_page(&state, &user, &state.settings.current(), &q.saved, None)
}

/// Numbers come in as text, so a blank or a typo is a message, not a 422.
#[derive(Deserialize)]
pub struct GeneralForm {
    csrf: String,
    max_connections: String,
    idle_timeout_secs: String,
    session_limit_secs: String,
    retention_days: String,
    events_file: String,
}

fn number<T: std::str::FromStr>(what: &str, text: &str) -> Result<T, String> {
    text.trim().parse().map_err(|_| format!("{what} must be a whole number"))
}

pub async fn save_general(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<GeneralForm>,
) -> Result<Response, WebError> {
    if !user.csrf_ok(&form.csrf) {
        return Ok(forbidden());
    }
    let mut new = (*state.settings.current()).clone();
    new.recording.events_file = form.events_file.trim().to_string();
    let parsed = (|| {
        new.limits.max_connections = number("Concurrent sessions", &form.max_connections)?;
        new.limits.idle_timeout_secs = number("Idle timeout", &form.idle_timeout_secs)?;
        new.limits.session_limit_secs = number("Session limit", &form.session_limit_secs)?;
        new.recording.retention_days = number("Retention", &form.retention_days)?;
        Ok::<_, String>(())
    })();
    let problem = match parsed {
        Err(problem) => Some(problem),
        Ok(()) => save(&state, &user, peer, new.clone(), "save_general", &["limits", "recording"]).await?,
    };
    match problem {
        None => Ok(Redirect::to("/settings?saved=general").into_response()),
        Some(problem) => Ok(settings_page(&state, &user, &new, "", Some(("general", problem)))?.into_response()),
    }
}

#[derive(Deserialize)]
pub struct SyslogForm {
    csrf: String,
    enabled: Option<String>,
    server: String,
    protocol: SyslogProtocol,
    format: SyslogFormat,
    facility: Facility,
    hostname: String,
}

pub async fn save_syslog(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<SyslogForm>,
) -> Result<Response, WebError> {
    if !user.csrf_ok(&form.csrf) {
        return Ok(forbidden());
    }
    let mut new = (*state.settings.current()).clone();
    new.syslog.enabled = form.enabled.is_some();
    new.syslog.server = form.server.trim().to_string();
    new.syslog.protocol = form.protocol;
    new.syslog.format = form.format;
    new.syslog.facility = form.facility;
    new.syslog.hostname = form.hostname.trim().to_string();
    match save(&state, &user, peer, new.clone(), "save_syslog", &["syslog"]).await? {
        None => Ok(Redirect::to("/settings?saved=syslog").into_response()),
        Some(problem) => Ok(settings_page(&state, &user, &new, "", Some(("syslog", problem)))?.into_response()),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Account
// ═════════════════════════════════════════════════════════════════════════════

async fn account_page(state: &AppState, user: &User, saved: bool, error: Option<String>) -> Result<Html<String>, WebError> {
    let mut ctx = page(user, "account");
    ctx.insert("saved", &saved);
    ctx.insert("error", &error);
    ctx.insert("min_password", &auth::MIN_PASSWORD);
    ctx.insert("audit", &store::recent_audit(&state.db, 100).await?);
    state.render("account.html", ctx)
}

pub async fn account(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(q): Query<Saved>,
) -> Result<Html<String>, WebError> {
    account_page(&state, &user, q.saved == "password", None).await
}

#[derive(Deserialize)]
pub struct PasswordForm {
    csrf: String,
    current: String,
    password: String,
    confirm: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<PasswordForm>,
) -> Result<Response, WebError> {
    if !user.csrf_ok(&form.csrf) {
        return Ok(forbidden());
    }
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    let problem = if !auth::verify_password(form.current, hash).await {
        Some("The current password is wrong.".to_string())
    } else {
        auth::password_problem(&form.password, &form.confirm)
    };
    if let Some(problem) = problem {
        return Ok(account_page(&state, &user, false, Some(problem)).await?.into_response());
    }
    let hash = auth::hash_password(form.password).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash).bind(user.id).execute(&mut *tx).await?;
    // Every other browser signed in as this user is signed out.
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?")
        .bind(user.id)
        .bind(&user.session)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    store::audit(&state.db, &user.username, &client(peer), "change_password", "other sessions signed out").await;
    Ok(Redirect::to("/account?saved=password").into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_links_keep_the_filter() {
        let f = Filter { kind: "request".into(), q: "a b&c".into(), ..Filter::default() };
        assert_eq!(events_url("/events", &f, Some(2)), "/events?kind=request&q=a%20b%26c&page=2");
        assert_eq!(events_url("/events", &Filter::default(), Some(0)), "/events");
    }
}
