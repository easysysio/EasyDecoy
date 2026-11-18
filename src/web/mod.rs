// =============================================================================
// web/mod.rs — the management GUI
//
// An HTTPS-only web interface for everything an administrator does: the
// dashboard, the events, the decoys' settings, syslog, and accounts. Pages are
// server-rendered Tera templates styled with Bootstrap; templates and static
// files are compiled into the binary, so the GUI needs nothing on disk.
//
// A honeypot's GUI has one risk an ordinary admin page doesn't: it runs on a
// machine built to be found and probed. So it listens on loopback by default
// (reached through an SSH tunnel), a decoy may never take its port, and every
// page warns when it listens somewhere an intruder could reach it.
//
// The modules:
//   auth.rs   sign-in, first-run setup, sessions, CSRF, the sign-in limiter
//   pages.rs  the pages behind the sign-in
//   tls.rs    the GUI's certificate
// =============================================================================

mod auth;
mod pages;
pub mod tls;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Path, Request};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum_server::tls_rustls::RustlsConfig;
use sqlx::SqlitePool;
use tera::{Context as Ctx, Tera};

use crate::settings::{Settings, SettingsStore};
use crate::supervisor::StatusMap;

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub settings: SettingsStore,
    pub status: StatusMap,
    /// Where the GUI listens: decoys may not take this port.
    pub gui: SocketAddr,
    /// The database file, shown on the settings page.
    pub db_file: String,
    /// The SSH decoy's host key fingerprint, shown on the decoys page.
    pub ssh_fingerprint: String,
    tera: Arc<Tera>,
    limiter: Arc<auth::Limiter>,
}

impl AppState {
    pub fn new(
        db: SqlitePool,
        settings: SettingsStore,
        status: StatusMap,
        gui: SocketAddr,
        db_file: String,
        ssh_fingerprint: String,
    ) -> Result<Self> {
        Ok(Self {
            db,
            settings,
            status,
            gui,
            db_file,
            ssh_fingerprint,
            tera: Arc::new(templates()?),
            limiter: Arc::new(auth::Limiter::default()),
        })
    }

    /// Renders a template, adding what every page uses.
    fn render(&self, name: &str, mut ctx: Ctx) -> Result<Html<String>, WebError> {
        ctx.insert("version", env!("CARGO_PKG_VERSION"));
        ctx.insert("gui_warning", &gui_warning(self.gui, &self.settings.current()));
        Ok(Html(self.tera.render(name, &ctx).with_context(|| format!("rendering {name}"))?))
    }
}

// The templates, compiled in. Tera escapes everything in a .html template.
fn templates() -> Result<Tera> {
    let mut tera = Tera::default();
    tera.add_raw_templates(vec![
        ("base.html", include_str!("../../templates/base.html")),
        ("login.html", include_str!("../../templates/login.html")),
        ("dashboard.html", include_str!("../../templates/dashboard.html")),
        ("events.html", include_str!("../../templates/events.html")),
        ("decoys.html", include_str!("../../templates/decoys.html")),
        ("settings.html", include_str!("../../templates/settings.html")),
        ("account.html", include_str!("../../templates/account.html")),
    ])
    .context("loading templates")?;
    Ok(tera)
}

// ─────────────────────────────────────────────────────────────────────────────
// gui_warning(gui, settings)
// Why the GUI's address is a risk, or None. On all addresses, or on one a
// decoy also listens on, the sign-in page is something an intruder scanning
// the decoys finds too.
// ─────────────────────────────────────────────────────────────────────────────
pub fn gui_warning(gui: SocketAddr, settings: &Settings) -> Option<String> {
    let ip = gui.ip();
    if ip.is_loopback() {
        return None;
    }
    if ip.is_unspecified() {
        return Some(format!(
            "The GUI listens on every address of this host ({gui}), so anyone who can reach a decoy \
             can reach its sign-in page too. Set gui_bind to 127.0.0.1 or a management-only address."
        ));
    }
    let decoys = [
        (settings.telnet.enabled, &settings.telnet.bind),
        (settings.http.enabled, &settings.http.bind),
        (settings.ftp.enabled, &settings.ftp.bind),
        (settings.ssh.enabled, &settings.ssh.bind),
    ];
    let shared = decoys.iter().any(|(enabled, bind)| {
        *enabled && bind.trim().parse::<SocketAddr>().is_ok_and(|b| b.ip() == ip || b.ip().is_unspecified())
    });
    shared.then(|| {
        format!(
            "The GUI listens on {ip}, an address a decoy also listens on, so anyone who finds the \
             decoy can find the GUI. Use 127.0.0.1 or a management-only address for gui_bind."
        )
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// The router
// ═════════════════════════════════════════════════════════════════════════════

pub fn router(state: AppState) -> Router {
    let private = Router::new()
        .route("/", get(pages::dashboard))
        .route("/events", get(pages::events))
        .route("/events/export", get(pages::export))
        .route("/decoys", get(pages::decoys))
        .route("/decoys/telnet", post(pages::save_telnet))
        .route("/decoys/http", post(pages::save_http))
        .route("/decoys/ftp", post(pages::save_ftp))
        .route("/decoys/ssh", post(pages::save_ssh))
        .route("/settings", get(pages::settings))
        .route("/settings/general", post(pages::save_general))
        .route("/settings/syslog", post(pages::save_syslog))
        .route("/account", get(pages::account))
        .route("/account/password", post(pages::change_password))
        .route("/logout", post(auth::logout))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_login));

    Router::new()
        .merge(private)
        .route("/login", get(auth::login_page).post(auth::login))
        .route("/setup", get(auth::setup_page).post(auth::setup))
        .route("/static/{*path}", get(asset))
        .layer(middleware::from_fn(same_origin))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

// What the GUI's pages may load, and from where: itself. As EasyWAF's.
const CSP: &str = "default-src 'self'; \
     script-src 'self' 'unsafe-inline'; \
     style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; \
     font-src 'self'; \
     connect-src 'self'; \
     form-action 'self'; \
     base-uri 'self'; \
     frame-ancestors 'none'";

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    let set = |h: &mut axum::http::HeaderMap, name: HeaderName, value: &'static str| {
        h.entry(name).or_insert(HeaderValue::from_static(value));
    };
    set(h, header::CONTENT_SECURITY_POLICY, CSP);
    set(h, header::X_FRAME_OPTIONS, "DENY");
    set(h, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(h, header::REFERRER_POLICY, "no-referrer");
    // No HSTS: the GUI is usually reached as localhost, and HSTS there would
    // force every other localhost service on the admin's machine to HTTPS.
    // Pages show attack data; nothing is kept by the browser or a proxy.
    // Static files set their own.
    set(h, header::CACHE_CONTROL, "no-store");
    response
}

// ─────────────────────────────────────────────────────────────────────────────
// same_origin
// Refuses a form post sent from another site. The session cookie is
// SameSite=Strict and every form carries a CSRF token; this is the third lock,
// and the only one on the sign-in and setup forms.
// ─────────────────────────────────────────────────────────────────────────────
async fn same_origin(request: Request, next: Next) -> Response {
    if request.method() == Method::POST {
        let host = request.headers().get(header::HOST).and_then(|v| v.to_str().ok());
        let origin = request.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok());
        if let (Some(origin), Some(host)) = (origin, host) {
            let origin_host = origin.split_once("://").map_or(origin, |(_, rest)| rest);
            if origin_host != host {
                return (StatusCode::FORBIDDEN, "This form was sent from another site.").into_response();
            }
        }
    }
    next.run(request).await
}

// The static files, compiled in.
async fn asset(Path(path): Path<String>) -> Response {
    let (body, kind): (&'static [u8], &str) = match path.as_str() {
        "bootstrap.min.css" => (include_bytes!("../../static/bootstrap.min.css"), "text/css"),
        "bootstrap-icons.css" => (include_bytes!("../../static/bootstrap-icons.css"), "text/css"),
        "bootstrap.bundle.min.js" => (include_bytes!("../../static/bootstrap.bundle.min.js"), "text/javascript"),
        "fonts/bootstrap-icons.woff2" => (include_bytes!("../../static/fonts/bootstrap-icons.woff2"), "font/woff2"),
        "favicon.svg" => (include_bytes!("../../static/favicon.svg"), "image/svg+xml"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, "public, max-age=86400")], body).into_response()
}

// ═════════════════════════════════════════════════════════════════════════════
// Serving
// ═════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────
// bind(addr)
// Binds the GUI's port, before anything announces it. The socket must be
// non-blocking: axum-server adopts it with tokio's from_std (see EasyWAF's
// tls::bind_listener).
// ─────────────────────────────────────────────────────────────────────────────
pub fn bind(addr: SocketAddr) -> Result<std::net::TcpListener> {
    let listener = std::net::TcpListener::bind(addr).with_context(|| {
        format!("binding the GUI to {addr} (is another EasyDecoy running? check with: ss -lntp | grep {})", addr.port())
    })?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

pub async fn serve(state: AppState, listener: std::net::TcpListener, cert_pem: String, key_pem: String) -> Result<()> {
    let tls = RustlsConfig::from_pem(cert_pem.into_bytes(), key_pem.into_bytes())
        .await
        .context("loading the GUI certificate")?;
    let app = router(state);
    axum_server::from_tcp_rustls(listener, tls)?
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .context("serving the GUI")
}

// ═════════════════════════════════════════════════════════════════════════════
// Errors
// ═════════════════════════════════════════════════════════════════════════════

/// Something that went wrong on our side: logged in full, shown as a plain
/// 500 so a page never reveals internals.
pub struct WebError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for WebError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        tracing::error!("GUI: {:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong; the EasyDecoy log has the details.")
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gui_warns_when_intruders_could_reach_it() {
        let s = Settings::default(); // decoys on 0.0.0.0
        assert!(gui_warning("127.0.0.1:9443".parse().unwrap(), &s).is_none());
        assert!(gui_warning("0.0.0.0:9443".parse().unwrap(), &s).is_some());
        assert!(gui_warning("10.9.0.1:9443".parse().unwrap(), &s).is_some(), "a decoy is on every address");

        let mut s = Settings::default();
        s.telnet.bind = "203.0.113.5:23".into();
        s.http.bind = "203.0.113.5:80".into();
        s.ftp.bind = "0.0.0.0:21".into();
        assert!(gui_warning("10.9.0.1:9443".parse().unwrap(), &s).is_some(), "FTP is on every address");
        s.ftp.bind = "203.0.113.5:21".into();
        assert!(gui_warning("10.9.0.1:9443".parse().unwrap(), &s).is_none(), "a management address");
        assert!(gui_warning("203.0.113.5:9443".parse().unwrap(), &s).is_some());
        s.ssh.enabled = true;
        s.ssh.bind = "0.0.0.0:22".into();
        assert!(gui_warning("10.9.0.1:9443".parse().unwrap(), &s).is_some(), "SSH is on every address");
    }

    #[test]
    fn every_template_parses() {
        templates().unwrap();
    }

    // ── The GUI, driven through its router ────────────────────────────────

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    struct Gui {
        app: Router,
        state: AppState,
        cookie: String,
    }

    impl Gui {
        async fn new() -> Self {
            let db = crate::db::memory().await;
            let (settings, _) = SettingsStore::load(db.clone()).await.unwrap();
            let state =
                AppState::new(db, settings, StatusMap::default(), "127.0.0.1:9443".parse().unwrap(), "test.db".into(), "SHA256:test".into())
                    .unwrap();
            Self { app: router(state.clone()), state, cookie: String::new() }
        }

        async fn call(&mut self, method: &str, uri: &str, form: &str, origin: Option<&str>) -> (StatusCode, String, String) {
            let mut req = HttpRequest::builder().method(method).uri(uri).header(header::HOST, "127.0.0.1:9443");
            if !self.cookie.is_empty() {
                req = req.header(header::COOKIE, &self.cookie);
            }
            if let Some(origin) = origin {
                req = req.header(header::ORIGIN, origin);
            }
            if method == "POST" {
                req = req.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
            }
            let mut req = req.body(Body::from(form.to_string())).unwrap();
            req.extensions_mut().insert(ConnectInfo::<SocketAddr>("127.0.0.1:50000".parse().unwrap()));
            let res = self.app.clone().oneshot(req).await.unwrap();
            let status = res.status();
            if let Some(c) = res.headers().get(header::SET_COOKIE) {
                self.cookie = c.to_str().unwrap().split(';').next().unwrap().to_string();
            }
            let location = res.headers().get(header::LOCATION).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
            let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
            (status, location, String::from_utf8_lossy(&body).into_owned())
        }

        async fn get(&mut self, uri: &str) -> (StatusCode, String, String) {
            self.call("GET", uri, "", None).await
        }

        async fn post(&mut self, uri: &str, form: &str) -> (StatusCode, String, String) {
            self.call("POST", uri, form, None).await
        }
    }

    fn csrf(page: &str) -> String {
        let needle = "name=\"csrf\" value=\"";
        let start = page.find(needle).unwrap() + needle.len();
        page[start..start + 64].to_string()
    }

    #[tokio::test]
    async fn the_gui_from_setup_to_sign_out() {
        let mut gui = Gui::new().await;

        // No accounts yet: everything leads to setup.
        assert_eq!(gui.get("/").await.1, "/setup");
        assert_eq!(gui.get("/login").await.1, "/setup");
        let (_, _, page) = gui.post("/setup", "username=admin&password=short&confirm=short").await;
        assert!(page.contains("at least 10 characters"), "{page}");
        let (status, to, _) = gui.post("/setup", "username=admin&password=correct+horse&confirm=correct+horse").await;
        assert_eq!((status, to.as_str()), (StatusCode::SEE_OTHER, "/"));
        assert!(gui.cookie.starts_with("easydecoy_session="));
        // Setup happens once.
        assert_eq!(gui.get("/setup").await.1, "/login");

        // Some events, so the pages have something to show.
        let peer: SocketAddr = "203.0.113.9:40000".parse().unwrap();
        let local: SocketAddr = "10.0.0.1:23".parse().unwrap();
        let attempt = crate::event::Event::new("telnet", "login_attempt", peer, local)
            .with("username", "<script>root</script>")
            .with("password_length", 8);
        let line = serde_json::to_string(&attempt).unwrap();
        crate::store::insert(&gui.state.db, &[(attempt, line)]).await.unwrap();

        for (uri, expect) in [
            ("/", "Login attempts"),
            ("/?range=7d", "Top sources"),
            ("/?range=30d", "203.0.113.9"),
            ("/events", "203.0.113.9"),
            ("/events?kind=login_attempt&q=root", "password of 8 characters"),
            ("/decoys", "fileserver01"),
            ("/settings", "test.db"),
            ("/account", "setup"),
        ] {
            let (status, _, page) = gui.get(uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert!(page.contains(expect), "{uri} should show {expect}");
            assert!(!page.contains("<script>root"), "{uri} must escape what clients sent");
        }
        let (status, _, export) = gui.get("/events/export").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(export.lines().count(), 1);

        // Every form needs the session's token.
        let (_, _, page) = gui.get("/decoys").await;
        let token = csrf(&page);
        let form = |csrf: &str, bind: &str| {
            format!("csrf={csrf}&enabled=on&bind={bind}&banner=Ubuntu&hostname=fs01")
        };
        assert_eq!(gui.post("/decoys/telnet", &form("nope", "0.0.0.0:23")).await.0, StatusCode::FORBIDDEN);
        // A decoy may not take the GUI's port.
        let (status, _, page) = gui.post("/decoys/telnet", &form(&token, "0.0.0.0:9443")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(page.contains("already uses"), "{page}");
        assert!(page.contains("0.0.0.0:9443"), "the form keeps what was typed");
        assert_eq!(gui.state.settings.current().telnet.bind, "0.0.0.0:2323");
        // A good one is saved.
        let (_, to, _) = gui.post("/decoys/telnet", &form(&token, "0.0.0.0:23")).await;
        assert_eq!(to, "/decoys?saved=telnet");
        assert_eq!(gui.state.settings.current().telnet.bind, "0.0.0.0:23");
        assert_eq!(gui.state.settings.current().telnet.hostname, "fs01");

        let general = format!(
            "csrf={token}&max_connections=abc&idle_timeout_secs=30&session_limit_secs=120&retention_days=90&events_file="
        );
        assert!(gui.post("/settings/general", &general).await.2.contains("must be a whole number"));

        // A form posted from another site is refused before anything else.
        let (status, _, _) = gui.call("POST", "/decoys/telnet", &form(&token, "0.0.0.0:24"), Some("https://evil.example")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Signing out ends the session.
        let session = gui.cookie.clone();
        assert_eq!(gui.post("/logout", &format!("csrf={token}")).await.1, "/login?signed_out=true");
        gui.cookie = session;
        assert_eq!(gui.get("/").await.1, "/login");

        // And a wrong password is just that.
        let (_, _, page) = gui.post("/login", "username=admin&password=wrong+password").await;
        assert!(page.contains("Wrong username or password"));
        let (_, to, _) = gui.post("/login", "username=admin&password=correct+horse").await;
        assert_eq!(to, "/");
    }
}
