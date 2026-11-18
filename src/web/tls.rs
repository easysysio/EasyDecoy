// =============================================================================
// web/tls.rs — the GUI's certificate
//
// The GUI is only ever served over HTTPS. With gui_cert and gui_key set, those
// PEM files are used, read at every start. Otherwise EasyDecoy makes a
// self-signed certificate on the first start and keeps it in the database, so
// the browser's exception for it holds across restarts and upgrades.
// =============================================================================

use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use sqlx::SqlitePool;

use crate::config::Config;

const META_CERT: &str = "gui_cert_pem";
const META_KEY: &str = "gui_key_pem";
// A self-signed certificate is trusted by exception, not by expiry, so it is
// made to outlast the install.
const VALID_YEARS: i32 = 10;

/// The certificate and key (PEM) the GUI serves, and where they came from.
pub struct Pem {
    pub cert: String,
    pub key: String,
    pub source: String,
}

pub async fn load(db: &SqlitePool, config: &Config, gui: SocketAddr) -> Result<Pem> {
    let (cert_path, key_path) = (config.gui_cert.trim(), config.gui_key.trim());
    if !cert_path.is_empty() {
        let cert = std::fs::read_to_string(cert_path).with_context(|| format!("reading gui_cert {cert_path}"))?;
        let key = std::fs::read_to_string(key_path).with_context(|| format!("reading gui_key {key_path}"))?;
        return Ok(Pem { cert, key, source: cert_path.to_string() });
    }

    let stored: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM meta WHERE key IN (?, ?)")
        .bind(META_CERT)
        .bind(META_KEY)
        .fetch_all(db)
        .await?;
    let get = |k: &str| stored.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    if let (Some(cert), Some(key)) = (get(META_CERT), get(META_KEY)) {
        return Ok(Pem { cert, key, source: "self-signed".to_string() });
    }

    let (cert, key) = generate(gui)?;
    let mut tx = db.begin().await?;
    for (k, v) in [(META_CERT, &cert), (META_KEY, &key)] {
        sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)").bind(k).bind(v).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    tracing::info!("GUI: made a self-signed certificate (your browser will ask you to accept it once)");
    Ok(Pem { cert, key, source: "self-signed, new".to_string() })
}

// ─────────────────────────────────────────────────────────────────────────────
// generate(gui)
// A self-signed certificate for the names the GUI is reached by: localhost
// (an SSH tunnel), its own address when that is a real one, and this host's
// name.
// ─────────────────────────────────────────────────────────────────────────────
fn generate(gui: SocketAddr) -> Result<(String, String)> {
    let host = crate::syslog::system_hostname();
    let mut names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(IpAddr::from([127, 0, 0, 1])),
        SanType::IpAddress(IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1])),
    ];
    if !gui.ip().is_loopback() && !gui.ip().is_unspecified() {
        names.push(SanType::IpAddress(gui.ip()));
    }
    if let Ok(name) = host.as_str().try_into()
        && host != "localhost"
        && host != "-"
    {
        names.push(SanType::DnsName(name));
    }

    let mut params = CertificateParams::default();
    params.subject_alt_names = names;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("EasyDecoy on {host}"));
    dn.push(DnType::OrganizationName, "EasyDecoy");
    params.distinguished_name = dn;
    let today = time::OffsetDateTime::now_utc().date();
    let (month, day) = (today.month() as u8, today.day().min(28));
    params.not_before = rcgen::date_time_ymd(today.year(), month, day);
    params.not_after = rcgen::date_time_ymd(today.year() + VALID_YEARS, month, day);

    let key = KeyPair::generate().context("making the GUI's key")?;
    let cert = params.self_signed(&key).context("making the GUI's certificate")?;
    Ok((cert.pem(), key.serialize_pem()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_self_signed_certificate_is_made_once_and_kept() {
        let db = crate::db::memory().await;
        let gui: SocketAddr = "127.0.0.1:9443".parse().unwrap();
        let first = load(&db, &Config::default(), gui).await.unwrap();
        assert!(first.cert.starts_with("-----BEGIN CERTIFICATE-----"));
        let again = load(&db, &Config::default(), gui).await.unwrap();
        assert_eq!(first.cert, again.cert);
        assert_eq!(again.source, "self-signed");
    }
}
