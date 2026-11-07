// =============================================================================
// decoy/mod.rs — what every decoy shares
//
// A decoy is a fake network service. This module owns the part they all have
// in common: binding the listener, refusing connections beyond the configured
// limit, recording each connection, and bounding how long a client may hold a
// session. Each protocol implements only `Decoy::handle`, which is given one
// connected client and talks to it.
//
// Every decoy is low-interaction by design: it imitates a service's prompts and
// replies, but nothing a client sends is ever executed or passed to the host.
// =============================================================================

pub mod http;
pub mod telnet;

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

use crate::event::{Event, EventTx};

/// A fake network service.
pub trait Decoy: Send + Sync + 'static {
    /// Name used in events and logs ("telnet", "http").
    const NAME: &'static str;

    /// Talks to one connected client until the session ends.
    fn handle(&self, conn: Conn) -> impl Future<Output = Result<()>> + Send;
}

/// What every decoy runs with: where events go and the limits from config.
#[derive(Clone)]
pub struct Shared {
    pub events: EventTx,
    /// One permit per open session, shared by all decoys.
    pub slots: Arc<Semaphore>,
    pub idle: Duration,
    pub session: Duration,
}

/// One connected client.
pub struct Conn {
    stream: TcpStream,
    decoy: &'static str,
    peer: SocketAddr,
    local: SocketAddr,
    events: EventTx,
    idle: Duration,
}

impl Conn {
    /// Starts an event about this client.
    pub fn event(&self, kind: &'static str) -> Event {
        Event::new(self.decoy, kind, self.peer, self.local)
    }

    pub async fn record(&self, event: Event) {
        self.events.send(event).await;
    }

    /// Reads what the client sent. 0 means the client is gone: it closed the
    /// connection, or went quiet for longer than the idle timeout.
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        match tokio::time::timeout(self.idle, self.stream.read(buf)).await {
            Ok(read) => Ok(read?),
            Err(_) => Ok(0),
        }
    }

    pub async fn write(&mut self, data: &[u8]) -> Result<()> {
        self.stream.write_all(data).await?;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// serve(decoy, bind, shared)
// Listens on `bind` and gives each client to `decoy`, recording a connect
// event first. Returns only if the listener cannot be bound; failures of a
// single session end that session and nothing else.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn serve<D: Decoy>(decoy: Arc<D>, bind: String, shared: Shared) -> Result<()> {
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("{} decoy: binding {bind}", D::NAME))?;
    tracing::info!("{} decoy listening on {bind}", D::NAME);

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                // Usually out of file descriptors. Pausing keeps a flood from
                // turning this loop into a busy spin.
                tracing::warn!("{} decoy: accept failed: {e}", D::NAME);
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        // At the session limit the connection is dropped (closed) at once: a
        // flood costs a socket for a moment, never a task or a buffer.
        let Ok(permit) = shared.slots.clone().try_acquire_owned() else {
            tracing::debug!("{} decoy: session limit reached, dropped {peer}", D::NAME);
            continue;
        };
        let Ok(local) = stream.local_addr() else { continue };

        let conn = Conn {
            stream,
            decoy: D::NAME,
            peer,
            local,
            events: shared.events.clone(),
            idle: shared.idle,
        };
        let decoy = decoy.clone();
        let session = shared.session;
        tokio::spawn(async move {
            let _permit = permit;
            conn.record(conn.event("connect")).await;
            match tokio::time::timeout(session, decoy.handle(conn)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::debug!("{} decoy: session from {peer} ended: {e:#}", D::NAME),
                Err(_) => tracing::debug!("{} decoy: session from {peer} hit the time limit", D::NAME),
            }
        });
    }
}
