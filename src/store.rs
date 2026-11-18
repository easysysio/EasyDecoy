// =============================================================================
// store.rs — events and the audit trail, in the database
//
// The event writer (src/event.rs) hands over events in batches, each batch one
// transaction, so a flood of connections costs one write per batch rather
// than one per event. The GUI reads them back here: the dashboard's totals,
// timeline and top lists, the filtered event list, and an export that streams
// page by page so a large one never sits in memory.
//
// Each event is stored as the exact JSON line that goes to the events file and
// to syslog, with the fields the GUI filters on beside it. The line is what an
// export returns, so an export and the events file never differ.
//
// Retention runs at startup and hourly: events older than the configured
// number of days are deleted and their space handed back to the disk, expired
// sessions are cleared, and the audit trail is kept for a year.
// =============================================================================

use std::time::Duration;

use anyhow::Result;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{AssertSqlSafe, QueryBuilder, Row, Sqlite, SqlitePool};
use time::{Duration as Span, OffsetDateTime};

use crate::event::{Event, stamp};
use crate::settings::SettingsStore;

// An event that is a login attempt: Telnet's, or a sign-in posted to the
// HTTP decoy's form (a request with a username).
const LOGIN: &str = "(kind = 'login_attempt' OR json_extract(line, '$.username') IS NOT NULL)";
// Rows per page of an export.
const EXPORT_PAGE: i64 = 1000;
// How long the audit trail is kept, whatever the event retention.
const AUDIT_DAYS: i64 = 365;
// How often retention runs.
const PRUNE_EVERY: Duration = Duration::from_secs(3600);

// ═════════════════════════════════════════════════════════════════════════════
// Writing and pruning
// ═════════════════════════════════════════════════════════════════════════════

/// Stores a batch of events, each with its JSON line, in one transaction.
pub async fn insert(db: &SqlitePool, batch: &[(Event, String)]) -> Result<()> {
    let mut tx = db.begin().await?;
    for (event, line) in batch {
        sqlx::query(
            "INSERT INTO events (time, decoy, kind, src_ip, src_port, dst_port, line) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(stamp(event.time))
        .bind(event.decoy)
        .bind(event.kind)
        .bind(event.src_ip.to_string())
        .bind(event.src_port as i64)
        .bind(event.dst_port as i64)
        .bind(line)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// prune(db, retention_days)
// Deletes events older than the retention window (0 keeps them all), expired
// sessions, and audit entries over a year old. Returns the events deleted.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn prune(db: &SqlitePool, retention_days: u32) -> Result<u64> {
    let now = OffsetDateTime::now_utc();
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?").bind(stamp(now)).execute(db).await?;
    sqlx::query("DELETE FROM audit WHERE time < ?")
        .bind(stamp(now - Span::days(AUDIT_DAYS)))
        .execute(db)
        .await?;
    if retention_days == 0 {
        return Ok(0);
    }
    let deleted = sqlx::query("DELETE FROM events WHERE time < ?")
        .bind(stamp(now - Span::days(retention_days as i64)))
        .execute(db)
        .await?
        .rows_affected();
    if deleted > 0 {
        // Hands the freed pages back to the filesystem (auto_vacuum=INCREMENTAL).
        sqlx::raw_sql("PRAGMA incremental_vacuum").execute(db).await?;
    }
    Ok(deleted)
}

/// Runs retention now and every hour, with whatever window is set at the time.
pub async fn serve_retention(db: SqlitePool, settings: SettingsStore) {
    let mut every = tokio::time::interval(PRUNE_EVERY);
    loop {
        every.tick().await;
        let days = settings.current().recording.retention_days;
        match prune(&db, days).await {
            Ok(0) => {}
            Ok(n) => tracing::info!("retention: deleted {n} event(s) older than {days} days"),
            Err(e) => tracing::warn!("retention: {e:#}"),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Reading events
// ═════════════════════════════════════════════════════════════════════════════

/// What the events page filters on. Empty fields don't filter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub decoy: String,
    pub kind: String,
    /// An exact source address.
    pub src: String,
    /// Text anywhere in the event's JSON line.
    pub q: String,
    pub page: u32,
}

impl Filter {
    fn apply(&self, qb: &mut QueryBuilder<Sqlite>) {
        qb.push(" WHERE 1 = 1");
        if !self.decoy.trim().is_empty() {
            qb.push(" AND decoy = ").push_bind(self.decoy.trim().to_string());
        }
        if !self.kind.trim().is_empty() {
            qb.push(" AND kind = ").push_bind(self.kind.trim().to_string());
        }
        if !self.src.trim().is_empty() {
            qb.push(" AND src_ip = ").push_bind(self.src.trim().to_string());
        }
        if !self.q.trim().is_empty() {
            qb.push(" AND line LIKE ").push_bind(like(self.q.trim())).push(" ESCAPE '\\'");
        }
    }
}

/// A LIKE pattern matching `text` literally, anywhere.
fn like(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

/// One event, as the GUI shows it.
#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub time: String,
    pub decoy: String,
    pub kind: String,
    pub src_ip: String,
    pub src_port: i64,
    pub dst_port: i64,
    /// What happened, in a few words: the username tried, the path asked for.
    pub summary: String,
    /// Every field of the event beyond the common ones, for a closer look.
    pub details: Vec<(String, String)>,
}

impl EventRow {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Self {
        let line: String = row.get("line");
        let fields: Map<String, Value> = serde_json::from_str(&line).unwrap_or_default();
        let text = |key: &str| fields.get(key).map(value_text).unwrap_or_default();
        let kind: String = row.get("kind");
        let summary = match kind.as_str() {
            "login_attempt" => {
                let user = text("username");
                let length = match text("password_length").as_str() {
                    "" => String::new(),
                    "1" => "password of 1 character".to_string(),
                    n => format!("password of {n} characters"),
                };
                match (user.is_empty(), length.is_empty()) {
                    (false, false) => format!("{user} · {length}"),
                    (true, _) => length,
                    (false, true) => user,
                }
            }
            "request" => match text("username") {
                user if user.is_empty() => format!("{} {}", text("method"), text("path")),
                user => format!("{} {} · {user}", text("method"), text("path")),
            },
            "non_http" => text("sample"),
            _ => String::new(),
        };
        const COMMON: [&str; 6] = ["time", "decoy", "kind", "src_ip", "src_port", "dst_port"];
        let details = fields
            .iter()
            .filter(|(k, _)| !COMMON.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), value_text(v)))
            .collect();
        Self {
            id: row.get("id"),
            time: row.get("time"),
            decoy: row.get("decoy"),
            kind,
            src_ip: row.get("src_ip"),
            src_port: row.get("src_port"),
            dst_port: row.get("dst_port"),
            summary,
            details,
        }
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A page of events matching `filter`, newest first, and how many match in all.
pub async fn list(db: &SqlitePool, filter: &Filter, per_page: u32) -> Result<(Vec<EventRow>, i64)> {
    let mut count = QueryBuilder::new("SELECT count(*) FROM events");
    filter.apply(&mut count);
    let total: i64 = count.build_query_scalar().fetch_one(db).await?;

    let mut qb = QueryBuilder::new("SELECT id, time, decoy, kind, src_ip, src_port, dst_port, line FROM events");
    filter.apply(&mut qb);
    qb.push(" ORDER BY id DESC LIMIT ")
        .push_bind(per_page as i64)
        .push(" OFFSET ")
        .push_bind(filter.page as i64 * per_page as i64);
    let rows = qb.build().fetch_all(db).await?;
    Ok((rows.iter().map(EventRow::from_row).collect(), total))
}

/// Every event matching `filter`, oldest first, as JSON lines, a page at a
/// time.
pub fn export(db: SqlitePool, filter: Filter) -> impl Stream<Item = Result<String, sqlx::Error>> {
    futures::stream::try_unfold((db, filter, 0i64, false), |(db, filter, after, done)| async move {
        if done {
            return Ok(None);
        }
        let mut qb = QueryBuilder::new("SELECT id, line FROM events");
        filter.apply(&mut qb);
        qb.push(" AND id > ").push_bind(after).push(" ORDER BY id LIMIT ").push_bind(EXPORT_PAGE);
        let rows = qb.build().fetch_all(&db).await?;
        let done = (rows.len() as i64) < EXPORT_PAGE;
        let last = rows.last().map(|r| r.get::<i64, _>("id")).unwrap_or(after);
        let mut page = String::new();
        for row in &rows {
            page.push_str(row.get::<&str, _>("line"));
            page.push('\n');
        }
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some((page, (db, filter, last, done))))
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// The dashboard
// ═════════════════════════════════════════════════════════════════════════════

/// The period the dashboard looks back over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum Range {
    #[default]
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
}

impl Range {
    /// (number of buckets, length of one, characters of `time` naming one)
    fn buckets(self) -> (i64, Span, usize) {
        match self {
            Self::Day => (24, Span::hours(1), 13),
            Self::Week => (7, Span::days(1), 10),
            Self::Month => (30, Span::days(1), 10),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub events: i64,
    pub logins: i64,
    pub sources: i64,
    pub timeline: Vec<Bucket>,
    /// The busiest bucket, for scaling the bars.
    pub peak: i64,
    pub top_sources: Vec<Ranked>,
    pub top_usernames: Vec<Ranked>,
    pub top_paths: Vec<Ranked>,
}

#[derive(Debug, Serialize)]
pub struct Bucket {
    /// Start of the bucket, seconds since the epoch, for the browser to show in
    /// its own time zone.
    pub start: i64,
    pub events: i64,
    pub logins: i64,
    /// Height of the bar, percent of the peak.
    pub height: i64,
}

#[derive(Debug, Serialize)]
pub struct Ranked {
    pub value: String,
    pub count: i64,
    /// When it was last seen (top sources only).
    pub last: String,
}

pub async fn summary(db: &SqlitePool, range: Range) -> Result<Summary> {
    let (count, width, chars) = range.buckets();
    // Buckets start on whole hours or whole days (UTC), the oldest
    // `count - 1` widths before the current one.
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let step = width.whole_seconds();
    let current = OffsetDateTime::from_unix_timestamp(now - now.rem_euclid(step))?;
    let first = current - width * (count as i32 - 1);
    let since = stamp(first);

    // The two queries below are built with format!, which sqlx makes us vouch
    // for: they splice in only the LOGIN constant and `chars`, a number from
    // Range. Nothing a client or user typed reaches them; values are bound.
    let totals = sqlx::query(AssertSqlSafe(format!(
        "SELECT count(*) AS events, \
                coalesce(sum({LOGIN}), 0) AS logins, \
                count(DISTINCT src_ip) AS sources \
         FROM events WHERE time >= ?"
    )))
    .bind(&since)
    .fetch_one(db)
    .await?;

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT substr(time, 1, {chars}) AS b, count(*), coalesce(sum({LOGIN}), 0) \
         FROM events WHERE time >= ? GROUP BY b"
    )))
    .bind(&since)
    .fetch_all(db)
    .await?;

    let mut timeline: Vec<Bucket> = (0..count)
        .map(|i| {
            let start = first + width * i as i32;
            let key = &stamp(start)[..chars];
            let (events, logins) =
                rows.iter().find(|(b, _, _)| b == key).map(|(_, e, l)| (*e, *l)).unwrap_or((0, 0));
            Bucket { start: start.unix_timestamp(), events, logins, height: 0 }
        })
        .collect();
    let peak = timeline.iter().map(|b| b.events).max().unwrap_or(0);
    for b in &mut timeline {
        b.height = if peak == 0 { 0 } else { (b.events * 100 / peak).max(if b.events > 0 { 2 } else { 0 }) };
    }

    let ranked = |sql: &'static str| {
        let since = since.clone();
        async move {
            let rows: Vec<(String, i64, String)> = sqlx::query_as(sql).bind(since).fetch_all(db).await?;
            Ok::<_, anyhow::Error>(
                rows.into_iter().map(|(value, count, last)| Ranked { value, count, last }).collect::<Vec<_>>(),
            )
        }
    };
    let top_sources = ranked(
        "SELECT src_ip, count(*) AS n, max(time) FROM events WHERE time >= ? \
         GROUP BY src_ip ORDER BY n DESC LIMIT 10",
    )
    .await?;
    let top_usernames = ranked(
        "SELECT json_extract(line, '$.username') AS u, count(*) AS n, max(time) FROM events \
         WHERE time >= ? AND u IS NOT NULL \
         GROUP BY u ORDER BY n DESC LIMIT 10",
    )
    .await?;
    let top_paths = ranked(
        "SELECT json_extract(line, '$.path') AS p, count(*) AS n, max(time) FROM events \
         WHERE time >= ? AND decoy = 'http' AND p IS NOT NULL \
         GROUP BY p ORDER BY n DESC LIMIT 10",
    )
    .await?;

    Ok(Summary {
        events: totals.get("events"),
        logins: totals.get("logins"),
        sources: totals.get("sources"),
        timeline,
        peak,
        top_sources,
        top_usernames,
        top_paths,
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// The audit trail
// ═════════════════════════════════════════════════════════════════════════════

/// Records something done in the GUI. A failure is logged, not returned: the
/// thing itself has already happened.
pub async fn audit(db: &SqlitePool, username: &str, client_ip: &str, action: &str, detail: &str) {
    tracing::info!(target: "audit", "{username} from {client_ip}: {action} {detail}");
    let result = sqlx::query(
        "INSERT INTO audit (time, username, client_ip, action, detail) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(stamp(OffsetDateTime::now_utc()))
    .bind(username)
    .bind(client_ip)
    .bind(action)
    .bind(detail)
    .execute(db)
    .await;
    if let Err(e) = result {
        tracing::warn!("audit entry could not be stored: {e}");
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditRow {
    pub time: String,
    pub username: String,
    pub client_ip: String,
    pub action: String,
    pub detail: String,
}

pub async fn recent_audit(db: &SqlitePool, limit: i64) -> Result<Vec<AuditRow>> {
    Ok(sqlx::query_as(
        "SELECT time, username, client_ip, action, detail FROM audit ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryStreamExt;
    use std::net::SocketAddr;

    fn event(kind: &'static str, ip: &str, age: Span) -> (Event, String) {
        let peer: SocketAddr = format!("{ip}:50000").parse().unwrap();
        let local: SocketAddr = "10.0.0.1:23".parse().unwrap();
        let mut e = Event::new("telnet", kind, peer, local);
        e.time = OffsetDateTime::now_utc() - age;
        if kind == "login_attempt" {
            e = e.with("username", "root").with("password_length", 6);
        }
        let line = serde_json::to_string(&e).unwrap();
        (e, line)
    }

    #[tokio::test]
    async fn events_are_listed_filtered_and_counted() {
        let db = crate::db::memory().await;
        insert(
            &db,
            &[
                event("connect", "10.0.0.5", Span::minutes(3)),
                event("login_attempt", "10.0.0.5", Span::minutes(2)),
                event("connect", "10.0.0.6", Span::minutes(1)),
            ],
        )
        .await
        .unwrap();

        let (rows, total) = list(&db, &Filter::default(), 50).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(rows[0].src_ip, "10.0.0.6", "newest first");

        let only = Filter { kind: "login_attempt".into(), ..Filter::default() };
        let (rows, total) = list(&db, &only, 50).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].summary, "root · password of 6 characters");

        let search = Filter { q: "roo".into(), ..Filter::default() };
        assert_eq!(list(&db, &search, 50).await.unwrap().1, 1);
        // A % in the search is a percent sign, not a wildcard.
        let literal = Filter { q: "%".into(), ..Filter::default() };
        assert_eq!(list(&db, &literal, 50).await.unwrap().1, 0);
    }

    #[tokio::test]
    async fn the_dashboard_counts_only_its_range() {
        let db = crate::db::memory().await;
        insert(
            &db,
            &[
                event("login_attempt", "10.0.0.5", Span::hours(2)),
                event("connect", "10.0.0.6", Span::minutes(5)),
                event("connect", "10.0.0.7", Span::days(3)),
            ],
        )
        .await
        .unwrap();
        let day = summary(&db, Range::Day).await.unwrap();
        assert_eq!((day.events, day.logins, day.sources), (2, 1, 2));
        assert_eq!(day.timeline.len(), 24);
        assert_eq!(day.timeline.iter().map(|b| b.events).sum::<i64>(), 2);
        assert_eq!(day.top_usernames[0].value, "root");

        let week = summary(&db, Range::Week).await.unwrap();
        assert_eq!(week.events, 3);
    }

    #[tokio::test]
    async fn retention_deletes_only_old_events() {
        let db = crate::db::memory().await;
        insert(&db, &[event("connect", "10.0.0.5", Span::days(40)), event("connect", "10.0.0.6", Span::days(1))])
            .await
            .unwrap();
        assert_eq!(prune(&db, 0).await.unwrap(), 0, "0 keeps everything");
        assert_eq!(prune(&db, 30).await.unwrap(), 1);
        assert_eq!(list(&db, &Filter::default(), 50).await.unwrap().1, 1);
    }

    #[tokio::test]
    async fn an_export_is_every_matching_line_in_order() {
        let db = crate::db::memory().await;
        let batch: Vec<_> = (0..2500).map(|i| event("connect", "10.0.0.5", Span::seconds(5000 - i))).collect();
        insert(&db, &batch).await.unwrap();
        let pages: Vec<String> = export(db, Filter::default()).try_collect().await.unwrap();
        assert_eq!(pages.len(), 3, "a page at a time");
        let text = pages.concat();
        assert_eq!(text.lines().count(), 2500);
        assert_eq!(text.lines().next().unwrap(), batch[0].1);
    }
}
