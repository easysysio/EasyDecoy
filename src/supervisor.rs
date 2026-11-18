// =============================================================================
// supervisor.rs — keeps the decoys running as the settings say
//
// Each enabled decoy runs in its own task. When a decoy's settings change in
// the GUI, only that decoy is restarted (its listener closed and opened again,
// its open sessions ended); a change to the limits restarts them all, since
// they share one session limit.
//
// A decoy that cannot bind its address (a port in use, an address this host
// doesn't have) does not stop EasyDecoy: the GUI shows why, and the supervisor
// tries again every 30 seconds or as soon as its settings change.
// =============================================================================

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::decoy::http::Http;
use crate::decoy::telnet::Telnet;
use crate::decoy::{self, Decoy, Shared};
use crate::event::EventTx;
use crate::settings::{Limits, Settings, SettingsStore};

// How long a decoy that could not bind waits before trying again.
const RETRY: Duration = Duration::from_secs(30);

/// What a decoy is doing, as the GUI shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "lowercase")]
pub enum Status {
    Disabled,
    Starting,
    /// Listening on this address.
    Running(String),
    /// Could not start, and why.
    Failed(String),
}

/// Every decoy's status, by name. Cheap to clone.
#[derive(Clone, Default)]
pub struct StatusMap(Arc<Mutex<BTreeMap<&'static str, Status>>>);

impl StatusMap {
    fn set(&self, name: &'static str, status: Status) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(name, status);
    }

    pub fn get(&self, name: &str) -> Status {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(name).cloned().unwrap_or(Status::Starting)
    }
}

/// A task that is stopped when this is dropped.
struct Task(JoinHandle<()>);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// spawn(settings, events)
// Starts every enabled decoy and the task that keeps them in line with the
// settings. Stopping (aborting) the returned task stops every decoy and every
// open session.
// ─────────────────────────────────────────────────────────────────────────────
pub fn spawn(settings: SettingsStore, events: EventTx) -> (StatusMap, JoinHandle<()>) {
    let status = StatusMap::default();
    let map = status.clone();
    let handle = tokio::spawn(async move {
        let mut watch = settings.subscribe();
        let mut s: Arc<Settings> = watch.borrow_and_update().clone();
        let mut shared = shared(&events, &s.limits);
        let mut telnet = start_telnet(&s, &shared, &map);
        let mut http = start_http(&s, &shared, &map);

        while watch.changed().await.is_ok() {
            let new = watch.borrow_and_update().clone();
            let limits = new.limits != s.limits;
            if limits {
                shared = self::shared(&events, &new.limits);
            }
            // Dropping a Task stops it before its replacement binds the address.
            if limits || new.telnet != s.telnet {
                drop(telnet.take());
                telnet = start_telnet(&new, &shared, &map);
            }
            if limits || new.http != s.http {
                drop(http.take());
                http = start_http(&new, &shared, &map);
            }
            s = new;
        }
        // The settings store is gone: keep the decoys as they are.
        std::future::pending::<()>().await;
        drop((telnet, http));
    });
    (status, handle)
}

fn shared(events: &EventTx, limits: &Limits) -> Shared {
    Shared {
        events: events.clone(),
        slots: Arc::new(Semaphore::new(limits.max_connections)),
        idle: Duration::from_secs(limits.idle_timeout_secs),
        session: Duration::from_secs(limits.session_limit_secs),
    }
}

fn start_telnet(s: &Settings, shared: &Shared, status: &StatusMap) -> Option<Task> {
    let t = &s.telnet;
    start(t.enabled, Telnet::new(t), &t.bind, shared, status)
}

fn start_http(s: &Settings, shared: &Shared, status: &StatusMap) -> Option<Task> {
    let h = &s.http;
    start(h.enabled, Http::new(h), &h.bind, shared, status)
}

fn start<D: Decoy>(enabled: bool, decoy: D, bind: &str, shared: &Shared, status: &StatusMap) -> Option<Task> {
    if !enabled {
        tracing::info!("{} decoy: disabled", D::NAME);
        status.set(D::NAME, Status::Disabled);
        return None;
    }
    status.set(D::NAME, Status::Starting);
    let (decoy, bind, shared, status) = (Arc::new(decoy), bind.trim().to_string(), shared.clone(), status.clone());
    Some(Task(tokio::spawn(async move {
        loop {
            match decoy::bind(D::NAME, &bind).await {
                Ok(listener) => {
                    tracing::info!("{} decoy listening on {bind}", D::NAME);
                    status.set(D::NAME, Status::Running(bind.clone()));
                    decoy::serve(decoy.clone(), listener, shared.clone()).await;
                }
                Err(e) => {
                    let why = format!("{:#}", e);
                    tracing::warn!("{why}; trying again in {}s", RETRY.as_secs());
                    status.set(D::NAME, Status::Failed(why));
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn wait_for(status: &StatusMap, name: &str, want: impl Fn(&Status) -> bool) -> Status {
        for _ in 0..100 {
            let s = status.get(name);
            if want(&s) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        status.get(name)
    }

    #[tokio::test]
    async fn decoys_follow_the_settings() {
        let db = crate::db::memory().await;
        let (store, _) = SettingsStore::load(db.clone()).await.unwrap();
        let mut s = Settings::default();
        s.telnet.bind = "127.0.0.1:0".into();
        s.http.enabled = false;
        store.save(s.clone()).await.unwrap();

        let (events, _writer) = crate::event::spawn_writer(db, store.clone()).await;
        let (status, task) = spawn(store.clone(), events);
        let running = wait_for(&status, "telnet", |s| matches!(s, Status::Running(_))).await;
        assert_eq!(running, Status::Running("127.0.0.1:0".into()));
        assert_eq!(status.get("http"), Status::Disabled);

        // A bind that can't work is reported, not fatal.
        s.telnet.bind = "192.0.2.1:2323".into();
        store.save(s).await.unwrap();
        let failed = wait_for(&status, "telnet", |s| matches!(s, Status::Failed(_))).await;
        assert!(matches!(failed, Status::Failed(_)), "{failed:?}");
        task.abort();
    }
}
