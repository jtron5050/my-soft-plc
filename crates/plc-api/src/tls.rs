//! rustls server config from device `auth.tls_*` paths.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use plc_config::{DeviceConfig, ProfileKind};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};

use crate::error::ApiError;

/// How the listener should present itself.
#[derive(Clone)]
pub enum ListenMode {
    /// Plain HTTP (dev, empty TLS paths).
    Http,
    /// TLS, optionally requiring client certs.
    Https(Arc<ServerConfig>),
}

/// Resolve listen mode from config.
pub fn listen_mode(cfg: &DeviceConfig) -> Result<ListenMode, ApiError> {
    let cert = cfg.auth.tls_cert_path.trim();
    let key = cfg.auth.tls_key_path.trim();
    if cert.is_empty() && key.is_empty() {
        if cfg.profile == ProfileKind::Prod {
            return Err(ApiError::bad_request(
                "config",
                "profile=prod refuses plaintext HTTP (set auth.tls_cert_path / tls_key_path)",
            ));
        }
        return Ok(ListenMode::Http);
    }
    if cert.is_empty() || key.is_empty() {
        return Err(ApiError::bad_request(
            "config",
            "both auth.tls_cert_path and auth.tls_key_path are required for HTTPS",
        ));
    }
    let certs = load_certs(Path::new(cert))?;
    let key = load_key(Path::new(key))?;
    let builder = ServerConfig::builder();
    let mut config = if cfg.auth.client_ca_path.trim().is_empty() {
        builder
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let mut roots = RootCertStore::empty();
        for c in load_certs(Path::new(cfg.auth.client_ca_path.trim()))? {
            roots
                .add(c)
                .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .map_err(|e| ApiError::internal(e.to_string()))?
    };
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(ListenMode::Https(Arc::new(config)))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, ApiError> {
    let mut reader = BufReader::new(
        File::open(path)
            .map_err(|e| ApiError::internal(format!("tls cert {}: {e}", path.display())))?,
    );
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(e.to_string()))
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>, ApiError> {
    let mut reader = BufReader::new(
        File::open(path)
            .map_err(|e| ApiError::internal(format!("tls key {}: {e}", path.display())))?,
    );
    let keys: Vec<PrivateKeyDer<'static>> = rustls_pemfile::private_key(&mut reader)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .collect();
    keys.into_iter()
        .next()
        .ok_or_else(|| ApiError::internal(format!("no private key in {}", path.display())))
}
