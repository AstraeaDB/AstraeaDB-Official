//! TLS/mTLS support for AstraeaDB server.
//!
//! Provides mutual TLS authentication where both server and client present certificates.
//! Supports optional client certificate verification for standard TLS or enforced mTLS.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use tokio_rustls::TlsAcceptor;
use x509_parser::prelude::*;

/// Errors that can occur during TLS configuration and operations.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("no certificates found in {0}")]
    NoCertificates(PathBuf),

    #[error("no private key found in {0}")]
    NoPrivateKey(PathBuf),

    #[error("failed to build TLS config: {0}")]
    ConfigBuild(String),

    #[error("invalid certificate: {0}")]
    InvalidCertificate(String),

    #[error("TLS error: {0}")]
    Rustls(#[from] rustls::Error),
}

/// TLS configuration for the AstraeaDB server.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to the server certificate chain (PEM format).
    pub cert_path: PathBuf,
    /// Path to the server private key (PEM format).
    pub key_path: PathBuf,
    /// Optional path to CA certificate for client verification.
    /// When set, enables client certificate verification.
    pub ca_cert_path: Option<PathBuf>,
    /// Whether to require a valid client certificate (mTLS).
    /// If false and ca_cert_path is set, client certs are optional but validated if present.
    pub require_client_cert: bool,
}

impl TlsConfig {
    /// Create a new TLS configuration for server-only TLS (no client verification).
    pub fn new(cert_path: impl Into<PathBuf>, key_path: impl Into<PathBuf>) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
            ca_cert_path: None,
            require_client_cert: false,
        }
    }

    /// Create a new TLS configuration with mutual TLS (client certificate required).
    pub fn with_mtls(
        cert_path: impl Into<PathBuf>,
        key_path: impl Into<PathBuf>,
        ca_cert_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
            ca_cert_path: Some(ca_cert_path.into()),
            require_client_cert: true,
        }
    }

    /// Load and build a rustls ServerConfig from this configuration.
    pub fn load_server_config(&self) -> Result<ServerConfig, TlsError> {
        // Ensure the crypto provider is installed
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // Load server certificate chain
        let certs = load_certs(&self.cert_path)?;

        // Load server private key
        let key = load_private_key(&self.key_path)?;

        // Build the config
        let builder = ServerConfig::builder();

        let config = if let Some(ca_path) = &self.ca_cert_path {
            // Load CA certificates for client verification
            let ca_certs = load_certs(ca_path)?;
            let mut root_store = RootCertStore::empty();
            for cert in ca_certs {
                root_store
                    .add(cert)
                    .map_err(|e| TlsError::ConfigBuild(format!("failed to add CA cert: {e}")))?;
            }

            // Create client verifier
            let verifier = if self.require_client_cert {
                WebPkiClientVerifier::builder(Arc::new(root_store))
                    .build()
                    .map_err(|e| TlsError::ConfigBuild(format!("failed to build verifier: {e}")))?
            } else {
                WebPkiClientVerifier::builder(Arc::new(root_store))
                    .allow_unauthenticated()
                    .build()
                    .map_err(|e| TlsError::ConfigBuild(format!("failed to build verifier: {e}")))?
            };

            builder
                .with_client_cert_verifier(verifier)
                .with_single_cert(certs, key)?
        } else {
            // No client verification
            builder.with_no_client_auth().with_single_cert(certs, key)?
        };

        Ok(config)
    }

    /// Create a TlsAcceptor from this configuration.
    pub fn build_acceptor(&self) -> Result<TlsAcceptor, TlsError> {
        let config = self.load_server_config()?;
        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

/// Load PEM-encoded certificates from a file.
pub fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let file = File::open(path).map_err(|e| TlsError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut reader = BufReader::new(file);

    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;

    if certs.is_empty() {
        return Err(TlsError::NoCertificates(path.to_path_buf()));
    }

    Ok(certs)
}

/// Load a PEM-encoded private key from a file.
/// Supports RSA, EC, and PKCS#8 private keys.
pub fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    let file = File::open(path).map_err(|e| TlsError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut reader = BufReader::new(file);

    loop {
        match rustls_pemfile::read_one(&mut reader).map_err(|e| TlsError::Io {
            path: path.to_path_buf(),
            source: e,
        })? {
            Some(rustls_pemfile::Item::Pkcs1Key(key)) => {
                return Ok(PrivateKeyDer::Pkcs1(key));
            }
            Some(rustls_pemfile::Item::Pkcs8Key(key)) => {
                return Ok(PrivateKeyDer::Pkcs8(key));
            }
            Some(rustls_pemfile::Item::Sec1Key(key)) => {
                return Ok(PrivateKeyDer::Sec1(key));
            }
            Some(_) => continue, // Skip other PEM items (certs, etc.)
            None => break,
        }
    }

    Err(TlsError::NoPrivateKey(path.to_path_buf()))
}

/// Extract the Common Name (CN) from the first certificate in a chain.
/// Returns None if the certificate chain is empty or CN cannot be extracted.
pub fn extract_client_cn(certs: &[CertificateDer<'_>]) -> Option<String> {
    let cert = certs.first()?;

    // Parse the X.509 certificate
    let (_, parsed) = X509Certificate::from_der(cert.as_ref()).ok()?;

    // Extract CN from subject
    for rdn in parsed.subject().iter_rdn() {
        for attr in rdn.iter() {
            if attr.attr_type() == &oid_registry::OID_X509_COMMON_NAME {
                if let Ok(cn) = attr.attr_value().as_str() {
                    return Some(cn.to_string());
                }
            }
        }
    }

    None
}

/// Extract the Subject Alternative Names (SANs) from a certificate.
/// Returns DNS names and IP addresses found in the SAN extension.
pub fn extract_sans(cert: &CertificateDer<'_>) -> Vec<String> {
    let mut sans = Vec::new();

    if let Ok((_, parsed)) = X509Certificate::from_der(cert.as_ref()) {
        if let Ok(Some(ext)) = parsed.subject_alternative_name() {
            for name in &ext.value.general_names {
                match name {
                    GeneralName::DNSName(dns) => sans.push(dns.to_string()),
                    GeneralName::IPAddress(ip) => {
                        if ip.len() == 4 {
                            sans.push(format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]));
                        } else if ip.len() == 16 {
                            // IPv6
                            let parts: Vec<String> = ip
                                .chunks(2)
                                .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
                                .collect();
                            sans.push(parts.join(":"));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    sans
}

/// Map a client certificate CN to a role name.
/// This is a simple mapping that can be customized.
///
/// Default mapping:
/// - CN ending with "-admin" -> "admin"
/// - CN ending with "-writer" -> "writer"
/// - All other CNs -> "reader"
pub fn cn_to_role(cn: &str) -> &'static str {
    if cn.ends_with("-admin") {
        "admin"
    } else if cn.ends_with("-writer") {
        "writer"
    } else {
        "reader"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
        KeyUsagePurpose, SanType,
    };
    use std::io::Write;
    use tempfile::TempDir;

    /// Generate a self-signed CA certificate.
    fn generate_ca() -> (String, String, CertificateParams, KeyPair) {
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, "Test CA");
        params
            .distinguished_name
            .push(DnType::OrganizationName, "AstraeaDB Test");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

        let key_pair = KeyPair::generate().unwrap();
        let cert = params.clone().self_signed(&key_pair).unwrap();

        (cert.pem(), key_pair.serialize_pem(), params, key_pair)
    }

    /// Generate a certificate signed by a CA.
    fn generate_signed_cert(
        cn: &str,
        ca_params: &CertificateParams,
        ca_key: &KeyPair,
        is_server: bool,
    ) -> (String, String) {
        let mut params = CertificateParams::default();
        params.distinguished_name.push(DnType::CommonName, cn);
        params
            .distinguished_name
            .push(DnType::OrganizationName, "AstraeaDB Test");

        if is_server {
            params.subject_alt_names = vec![
                SanType::DnsName("localhost".try_into().unwrap()),
                SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
            ];
            params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        } else {
            params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        }

        let key_pair = KeyPair::generate().unwrap();

        // Sign with CA - clone params since self_signed takes ownership
        let ca_cert = ca_params.clone().self_signed(ca_key).unwrap();
        let cert = params.signed_by(&key_pair, &ca_cert, ca_key).unwrap();

        (cert.pem(), key_pair.serialize_pem())
    }

    /// Write content to a file and return the path.
    fn write_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_load_certs_valid() {
        let (ca_pem, _, _, _) = generate_ca();
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "ca.pem", &ca_pem);

        let certs = load_certs(&path).unwrap();
        assert_eq!(certs.len(), 1);
    }

    #[test]
    fn test_load_certs_missing_file() {
        let result = load_certs(Path::new("/nonexistent/path/cert.pem"));
        assert!(matches!(result, Err(TlsError::Io { .. })));
    }

    #[test]
    fn test_load_certs_empty_file() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "empty.pem", "");

        let result = load_certs(&path);
        assert!(matches!(result, Err(TlsError::NoCertificates(_))));
    }

    #[test]
    fn test_load_private_key_valid() {
        let (_, key_pem, _, _) = generate_ca();
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "key.pem", &key_pem);

        let key = load_private_key(&path);
        assert!(key.is_ok());
    }

    #[test]
    fn test_load_private_key_missing_file() {
        let result = load_private_key(Path::new("/nonexistent/path/key.pem"));
        assert!(matches!(result, Err(TlsError::Io { .. })));
    }

    #[test]
    fn test_load_private_key_no_key() {
        let (ca_pem, _, _, _) = generate_ca();
        let dir = TempDir::new().unwrap();
        // Write cert instead of key
        let path = write_file(&dir, "cert.pem", &ca_pem);

        let result = load_private_key(&path);
        assert!(matches!(result, Err(TlsError::NoPrivateKey(_))));
    }

    #[test]
    fn test_extract_client_cn() {
        let (_, _, ca_params, ca_key) = generate_ca();
        let (client_pem, _) = generate_signed_cert("test-client-admin", &ca_params, &ca_key, false);

        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "client.pem", &client_pem);
        let certs = load_certs(&path).unwrap();

        let cn = extract_client_cn(&certs);
        assert_eq!(cn, Some("test-client-admin".to_string()));
    }

    #[test]
    fn test_extract_client_cn_empty() {
        let certs: Vec<CertificateDer<'static>> = vec![];
        assert_eq!(extract_client_cn(&certs), None);
    }

    #[test]
    fn test_extract_sans() {
        let (_, _, ca_params, ca_key) = generate_ca();
        let (server_pem, _) = generate_signed_cert("test-server", &ca_params, &ca_key, true);

        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "server.pem", &server_pem);
        let certs = load_certs(&path).unwrap();

        let sans = extract_sans(&certs[0]);
        assert!(sans.contains(&"localhost".to_string()));
        assert!(sans.contains(&"127.0.0.1".to_string()));
    }

    #[test]
    fn test_cn_to_role() {
        assert_eq!(cn_to_role("service-admin"), "admin");
        assert_eq!(cn_to_role("service-writer"), "writer");
        assert_eq!(cn_to_role("service-reader"), "reader");
        assert_eq!(cn_to_role("some-other-service"), "reader");
    }

    #[test]
    fn test_tls_config_new() {
        let config = TlsConfig::new("/path/to/cert.pem", "/path/to/key.pem");
        assert_eq!(config.cert_path, PathBuf::from("/path/to/cert.pem"));
        assert_eq!(config.key_path, PathBuf::from("/path/to/key.pem"));
        assert!(config.ca_cert_path.is_none());
        assert!(!config.require_client_cert);
    }

    #[test]
    fn test_tls_config_with_mtls() {
        let config = TlsConfig::with_mtls(
            "/path/to/cert.pem",
            "/path/to/key.pem",
            "/path/to/ca.pem",
        );
        assert!(config.ca_cert_path.is_some());
        assert!(config.require_client_cert);
    }

    #[test]
    fn test_load_server_config_no_client_auth() {
        let (_ca_pem, _, ca_params, ca_key) = generate_ca();
        let (server_pem, server_key_pem) = generate_signed_cert("test-server", &ca_params, &ca_key, true);

        let dir = TempDir::new().unwrap();
        let cert_path = write_file(&dir, "server.pem", &server_pem);
        let key_path = write_file(&dir, "server-key.pem", &server_key_pem);

        let config = TlsConfig::new(&cert_path, &key_path);
        let server_config = config.load_server_config();
        assert!(server_config.is_ok());
    }

    #[test]
    fn test_load_server_config_with_mtls() {
        let (ca_pem, _, ca_params, ca_key) = generate_ca();
        let (server_pem, server_key_pem) = generate_signed_cert("test-server", &ca_params, &ca_key, true);

        let dir = TempDir::new().unwrap();
        let cert_path = write_file(&dir, "server.pem", &server_pem);
        let key_path = write_file(&dir, "server-key.pem", &server_key_pem);
        let ca_path = write_file(&dir, "ca.pem", &ca_pem);

        let config = TlsConfig::with_mtls(&cert_path, &key_path, &ca_path);
        let server_config = config.load_server_config();
        assert!(server_config.is_ok());
    }

    #[test]
    fn test_load_server_config_invalid_cert() {
        let dir = TempDir::new().unwrap();
        let cert_path = write_file(&dir, "invalid.pem", "not a certificate");
        let key_path = write_file(&dir, "key.pem", "not a key");

        let config = TlsConfig::new(&cert_path, &key_path);
        let result = config.load_server_config();
        assert!(result.is_err());
    }

    #[test]
    fn test_build_acceptor() {
        let (_, _, ca_params, ca_key) = generate_ca();
        let (server_pem, server_key_pem) = generate_signed_cert("test-server", &ca_params, &ca_key, true);

        let dir = TempDir::new().unwrap();
        let cert_path = write_file(&dir, "server.pem", &server_pem);
        let key_path = write_file(&dir, "server-key.pem", &server_key_pem);

        let config = TlsConfig::new(&cert_path, &key_path);
        let acceptor = config.build_acceptor();
        assert!(acceptor.is_ok());
    }
}
