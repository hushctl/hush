use std::io;
use std::path::Path;
use std::sync::Arc;

use rcgen::{BasicConstraints, Certificate, CertificateParams, DnType, IsCa, KeyPair};
use sha2::{Digest, Sha256};

pub struct TlsMaterial {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    /// SHA-256 fingerprint of the leaf cert DER, colon-separated hex bytes.
    pub fingerprint: String,
}

/// A loaded or freshly generated CA — kept in memory to sign leaf certs.
pub struct CaBundle {
    pub cert: Certificate,
    pub key_pair: KeyPair,
    /// Path to ca.crt on disk (used by the `trust` subcommand).
    pub cert_pem_path: std::path::PathBuf,
}

/// Load existing CA cert+key from `{hush_dir}/tls/ca.*`, or generate and
/// persist a new one. The returned `CaBundle` can sign leaf certs via
/// `CertificateParams::signed_by`.
pub fn load_or_generate_ca(hush_dir: &Path) -> io::Result<CaBundle> {
    let tls_dir = hush_dir.join("tls");
    let ca_cert_path = tls_dir.join("ca.crt");
    let ca_key_path = tls_dir.join("ca.key");

    if ca_cert_path.exists() && ca_key_path.exists() {
        let cert_pem = std::fs::read_to_string(&ca_cert_path)?;
        let key_pem = std::fs::read_to_string(&ca_key_path)?;

        let params = CertificateParams::from_ca_cert_pem(&cert_pem)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let key_pair = KeyPair::from_pem(&key_pem)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        // Reconstruct Certificate for signing — same DN, same SKI (PreSpecified from
        // from_ca_cert_pem), same key ⟹ leaf AKI will match installed ca.crt's SKI.
        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        return Ok(CaBundle {
            cert,
            key_pair,
            cert_pem_path: ca_cert_path,
        });
    }

    generate_ca(tls_dir, ca_cert_path, ca_key_path)
}

fn generate_ca(
    tls_dir: std::path::PathBuf,
    ca_cert_path: std::path::PathBuf,
    ca_key_path: std::path::PathBuf,
) -> io::Result<CaBundle> {
    std::fs::create_dir_all(&tls_dir)?;

    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "Hush Local CA");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    let key_pair =
        KeyPair::generate().map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    std::fs::write(&ca_cert_path, cert.pem())?;
    std::fs::write(&ca_key_path, key_pair.serialize_pem())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ca_key_path, std::fs::Permissions::from_mode(0o600))?;
        std::fs::set_permissions(&ca_cert_path, std::fs::Permissions::from_mode(0o644))?;
    }

    tracing::info!("Generated new local CA → {}", ca_cert_path.display());

    Ok(CaBundle {
        cert,
        key_pair,
        cert_pem_path: ca_cert_path,
    })
}

/// Load or generate the daemon's leaf TLS cert, signed by the local CA.
///
/// SANs: localhost, 127.0.0.1, ::1, `machine_name`, and every non-loopback
/// interface IP. Regenerates if the cert is corrupt or misses any current IP.
pub fn load_or_generate(hush_dir: &Path, machine_name: &str) -> io::Result<TlsMaterial> {
    let tls_dir = hush_dir.join("tls");
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");

    let desired_sans = build_sans(machine_name);
    let ca = load_or_generate_ca(hush_dir)?;

    if cert_path.exists() && key_path.exists() {
        match try_load(&cert_path, &key_path) {
            Ok(mat) => {
                if sans_are_covered(&cert_path, &desired_sans) {
                    return Ok(mat);
                }
                tracing::info!(
                    "TLS cert missing some SANs (new IPs?) — regenerating: {}",
                    desired_sans.join(", ")
                );
            }
            Err(_) => {
                tracing::warn!("TLS cert/key could not be parsed — regenerating");
            }
        }
    }

    generate_and_save(&tls_dir, &cert_path, &key_path, desired_sans, &ca)
}

fn build_sans(machine_name: &str) -> Vec<String> {
    let mut sans = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];

    if !machine_name.is_empty() && machine_name != "localhost" && machine_name != "unknown" {
        sans.push(machine_name.to_string());
    }

    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            let ip = iface.ip().to_string();
            if !sans.contains(&ip) {
                sans.push(ip);
            }
        }
    }

    sans
}

/// Returns true if the cert at `cert_path` already contains all `wanted` SANs.
fn sans_are_covered(cert_path: &Path, wanted: &[String]) -> bool {
    let Ok(pem) = std::fs::read_to_string(cert_path) else {
        return false;
    };
    let Ok(der) = pem_to_der(&pem) else {
        return false;
    };
    for san in wanted {
        let needle = san.as_bytes();
        if !der.windows(needle.len()).any(|w| w == needle) {
            return false;
        }
    }
    true
}

pub(crate) fn pem_to_der(pem: &str) -> io::Result<Vec<u8>> {
    let bytes = pem.as_bytes().to_vec();
    let mut cursor = std::io::Cursor::new(bytes);
    let certs: Vec<_> = rustls_pemfile::certs(&mut cursor)
        .collect::<Result<_, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    certs
        .into_iter()
        .next()
        .map(|d| d.to_vec())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no cert in pem"))
}

fn generate_and_save(
    tls_dir: &Path,
    cert_path: &Path,
    key_path: &Path,
    sans: Vec<String>,
    ca: &CaBundle,
) -> io::Result<TlsMaterial> {
    std::fs::create_dir_all(tls_dir)?;

    tracing::info!(
        "Generating TLS leaf cert (CA-signed) with SANs: {}",
        sans.join(", ")
    );

    let leaf_key =
        KeyPair::generate().map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let leaf_params = CertificateParams::new(sans)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let leaf_cert = leaf_params
        .signed_by(&leaf_key, &ca.cert, &ca.key_pair)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let cert_pem = leaf_cert.pem().into_bytes();
    let key_pem = leaf_key.serialize_pem().into_bytes();

    std::fs::write(cert_path, &cert_pem)?;
    std::fs::write(key_path, &key_pem)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))?;
        std::fs::set_permissions(cert_path, std::fs::Permissions::from_mode(0o644))?;
    }

    let fingerprint = fingerprint_pem(&cert_pem)?;
    Ok(TlsMaterial {
        cert_pem,
        key_pem,
        fingerprint,
    })
}

fn try_load(cert_path: &Path, key_path: &Path) -> io::Result<TlsMaterial> {
    let cert_pem = std::fs::read(cert_path)?;
    let key_pem = std::fs::read(key_path)?;

    let mut cert_cursor = std::io::Cursor::new(&cert_pem);
    rustls_pemfile::certs(&mut cert_cursor)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut key_cursor = std::io::Cursor::new(&key_pem);
    rustls_pemfile::private_key(&mut key_cursor)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key found"))?;

    let fingerprint = fingerprint_pem(&cert_pem)?;
    Ok(TlsMaterial {
        cert_pem,
        key_pem,
        fingerprint,
    })
}

/// Read CA cert + key PEM from the hush dir derived from the state file path.
/// Returns `(Some(cert), Some(key))` if both files exist, `(None, None)` otherwise.
pub fn read_ca_pems_from_state(state_path: &Path) -> (Option<String>, Option<String>) {
    let hush_dir = state_path.parent().unwrap_or_else(|| Path::new("."));
    let ca_crt = hush_dir.join("tls").join("ca.crt");
    let ca_key = hush_dir.join("tls").join("ca.key");
    match (
        std::fs::read_to_string(&ca_crt),
        std::fs::read_to_string(&ca_key),
    ) {
        (Ok(cert), Ok(key)) => (Some(cert), Some(key)),
        _ => (None, None),
    }
}


/// Build a TLS connector for peer-to-peer connections. When a CA cert is
/// available, the connector verifies the peer's certificate chain against it
/// (hostnames are not checked — certs use IP SANs that may differ across
/// machines). Falls back to accepting any certificate on first boot before
/// the CA has been shared via gossip.
pub fn make_peer_tls_connector(ca_cert_pem: Option<&str>) -> tokio_tungstenite::Connector {
    make_peer_tls_connector_inner(ca_cert_pem, None, None)
}

/// Like `make_peer_tls_connector` but also presents the local leaf cert as a
/// TLS client identity, enabling mTLS authentication to the peer's `/peer` endpoint.
pub fn make_peer_tls_connector_with_identity(
    ca_cert_pem: Option<&str>,
    leaf_cert_pem: &[u8],
    leaf_key_pem: &[u8],
) -> tokio_tungstenite::Connector {
    make_peer_tls_connector_inner(ca_cert_pem, Some(leaf_cert_pem), Some(leaf_key_pem))
}

fn make_peer_tls_connector_inner(
    ca_cert_pem: Option<&str>,
    leaf_cert_pem: Option<&[u8]>,
    leaf_key_pem: Option<&[u8]>,
) -> tokio_tungstenite::Connector {
    let mut builder = native_tls::TlsConnector::builder();
    if let Some(pem) = ca_cert_pem {
        if let Ok(cert) = native_tls::Certificate::from_pem(pem.as_bytes()) {
            builder.add_root_certificate(cert);
            // Certs use IP SANs; the peer may be reached via a different IP
            builder.danger_accept_invalid_hostnames(true);
        } else {
            tracing::warn!("Invalid CA cert PEM — falling back to unverified TLS");
            builder.danger_accept_invalid_certs(true);
            builder.danger_accept_invalid_hostnames(true);
        }
    } else {
        // First boot — CA not yet available
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
    }
    // Optionally present client identity for mTLS
    if let (Some(cert), Some(key)) = (leaf_cert_pem, leaf_key_pem) {
        match native_tls::Identity::from_pkcs8(cert, key) {
            Ok(identity) => {
                builder.identity(identity);
            }
            Err(e) => {
                tracing::warn!("Failed to load leaf cert identity for mTLS: {e}");
            }
        }
    }
    tokio_tungstenite::Connector::NativeTls(
        builder.build().expect("Failed to build TLS connector").into(),
    )
}

/// Build a `rustls::ServerConfig` that optionally verifies client certificates
/// against the mesh CA. Browsers connecting to `/ws` won't have a cert (allowed);
/// peer daemons connecting to `/peer` must present one signed by the mesh CA.
///
/// The optional client verifier uses `allow_unauthenticated()` so both routes
/// can share the same TLS listener — the `/peer` handler enforces cert presence.
pub fn build_server_config(
    cert_pem: &[u8],
    key_pem: &[u8],
    ca_cert_pem: Option<&str>,
) -> io::Result<rustls::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    // rustls_pemfile 2.x returns 'static items (bytes are copied from the reader)
    let certs: Vec<CertificateDer<'static>> = {
        let mut cursor = std::io::Cursor::new(cert_pem);
        rustls_pemfile::certs(&mut cursor)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
    };

    let key: PrivateKeyDer<'static> = {
        let mut cursor = std::io::Cursor::new(key_pem);
        rustls_pemfile::private_key(&mut cursor)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key found"))?
    };

    let builder = rustls::ServerConfig::builder();

    let config = if let Some(ca_pem) = ca_cert_pem {
        let ca_der = pem_to_der(ca_pem)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(ca_der).into_owned())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
            .allow_unauthenticated()
            .build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
    };

    Ok(config)
}

pub(crate) fn fingerprint_pem(cert_pem: &[u8]) -> io::Result<String> {
    let mut cursor = std::io::Cursor::new(cert_pem);
    let der = rustls_pemfile::certs(&mut cursor)
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no cert in pem"))?
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let hash = Sha256::digest(&der);
    let hex = hash
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok(hex)
}

/// Sign arbitrary data with the CA private key. Returns the raw signature bytes.
/// Uses ECDSA P-256 with SHA-256 (matching rcgen's default key type).
pub fn sign_with_ca(ca_key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let key_pair =
        KeyPair::from_pem(ca_key_pem).map_err(|e| format!("parse CA key: {e}"))?;
    let signing_key = ring::signature::EcdsaKeyPair::from_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
        key_pair.serialized_der(),
        &ring::rand::SystemRandom::new(),
    )
    .map_err(|e| format!("load signing key: {e}"))?;
    let sig = signing_key
        .sign(&ring::rand::SystemRandom::new(), data)
        .map_err(|e| format!("sign: {e}"))?;
    Ok(sig.as_ref().to_vec())
}

/// Verify a signature against the CA certificate's public key.
/// Extracts the SPKI from the CA cert PEM and verifies ECDSA P-256 SHA-256.
pub fn verify_ca_signature(
    ca_cert_pem: &str,
    data: &[u8],
    signature: &[u8],
) -> Result<bool, String> {
    let der = pem_to_der(ca_cert_pem).map_err(|e| format!("parse CA cert: {e}"))?;
    let (_, cert) = x509_parser::parse_x509_certificate(&der)
        .map_err(|e| format!("parse x509: {e}"))?;
    let key_data = &cert.public_key().subject_public_key.data;
    let public_key = ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_ASN1,
        key_data,
    );
    match public_key.verify(data, signature) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_produces_nonempty_signature() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ca = load_or_generate_ca(tmp.path()).unwrap();
        let key_pem = ca.key_pair.serialize_pem();

        let data = b"hello world upgrade tarball bytes";
        let sig = sign_with_ca(&key_pem, data).expect("sign should succeed");
        assert!(!sig.is_empty());

        // Signing the same data twice should produce valid (possibly different) signatures
        let sig2 = sign_with_ca(&key_pem, data).expect("sign should succeed");
        assert!(!sig2.is_empty());
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ca = load_or_generate_ca(tmp.path()).unwrap();
        let key_pem = ca.key_pair.serialize_pem();
        let cert_pem = std::fs::read_to_string(tmp.path().join("tls/ca.crt")).unwrap();

        let data = b"hello world upgrade tarball bytes";
        let sig = sign_with_ca(&key_pem, data).expect("sign should succeed");

        let ok = verify_ca_signature(&cert_pem, data, &sig).expect("verify should not error");
        assert!(ok, "valid signature should verify");

        // Tampered data should not verify
        let bad = verify_ca_signature(&cert_pem, b"tampered", &sig).expect("verify should not error");
        assert!(!bad, "tampered data should fail verification");
    }

    #[test]
    fn ca_generation_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ca1 = load_or_generate_ca(tmp.path()).unwrap();
        let ca2 = load_or_generate_ca(tmp.path()).unwrap();
        // Should load the same key pair, not generate a new one
        assert_eq!(ca1.key_pair.serialize_pem(), ca2.key_pair.serialize_pem());
    }

    #[test]
    fn leaf_cert_contains_localhost_san() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mat = load_or_generate(tmp.path(), "test-machine").unwrap();
        let cert_str = String::from_utf8_lossy(&mat.cert_pem);
        assert!(cert_str.contains("BEGIN CERTIFICATE"));
        assert!(!mat.fingerprint.is_empty());
    }
}
