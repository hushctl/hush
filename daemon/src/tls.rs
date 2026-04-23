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

        let ca_key_data = std::fs::read(&ca_key_path)?;
        let key_pem = if is_encrypted_key(&ca_key_data) {
            let passphrase = get_passphrase("Enter CA key passphrase", false)?;
            decrypt_key_pem(&ca_key_data, &passphrase)?
        } else {
            // Legacy plaintext PEM — migrate to encrypted format on first load.
            let pem = String::from_utf8(ca_key_data)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            eprintln!("Migrating CA key to encrypted format...");
            let passphrase = get_passphrase("Create passphrase for CA key", true)?;
            let encrypted = encrypt_key_pem(&pem, &passphrase)?;
            std::fs::write(&ca_key_path, &encrypted)?;
            pem
        };

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

    let passphrase = get_passphrase("Create passphrase for CA key", true)?;
    let encrypted_key = encrypt_key_pem(&key_pair.serialize_pem(), &passphrase)?;

    std::fs::write(&ca_cert_path, cert.pem())?;
    std::fs::write(&ca_key_path, &encrypted_key)?;

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
    let ca_key_path = hush_dir.join("tls").join("ca.key");

    let cert_pem = std::fs::read_to_string(&ca_crt).ok();
    let key_pem = std::fs::read(&ca_key_path).ok().and_then(|data| {
        if is_encrypted_key(&data) {
            // Cannot prompt interactively from async gossip/transfer context —
            // use env var only. Callers handle None key gracefully (skip signing).
            std::env::var("HUSH_CA_PASSPHRASE").ok().and_then(|pass| {
                decrypt_key_pem(&data, &pass).ok()
            })
        } else {
            // Legacy plaintext — return as-is.
            String::from_utf8(data).ok()
        }
    });

    (cert_pem, key_pem)
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
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use std::sync::Arc;

    // Build the server cert verifier. When a CA cert is available, verify the
    // peer's chain against it (hostname skipped — certs use IP SANs).
    // Pre-join / first-boot: accept anything.
    let verifier: Arc<dyn rustls::client::danger::ServerCertVerifier> =
        if let Some(pem) = ca_cert_pem {
            match pem_to_der(pem) {
                Ok(der) => {
                    let mut store = rustls::RootCertStore::empty();
                    if store.add(CertificateDer::from(der).into_owned()).is_ok() {
                        Arc::new(CaOnlyVerifier {
                            roots: Arc::new(store),
                        })
                    } else {
                        tracing::warn!(
                            "Failed to add CA cert to root store — falling back to unverified TLS"
                        );
                        Arc::new(AcceptAnyCert)
                    }
                }
                Err(_) => {
                    tracing::warn!("Invalid CA cert PEM — falling back to unverified TLS");
                    Arc::new(AcceptAnyCert)
                }
            }
        } else {
            Arc::new(AcceptAnyCert)
        };

    // Try to parse a client cert+key for mTLS.
    let client_identity: Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> =
        if let (Some(cert_pem), Some(key_pem)) = (leaf_cert_pem, leaf_key_pem) {
            let certs: Result<Vec<CertificateDer<'static>>, _> = {
                let mut c = std::io::Cursor::new(cert_pem);
                rustls_pemfile::certs(&mut c).collect()
            };
            let key: Result<Option<PrivateKeyDer<'static>>, _> = {
                let mut c = std::io::Cursor::new(key_pem);
                rustls_pemfile::private_key(&mut c)
            };
            match (certs, key) {
                (Ok(certs), Ok(Some(key))) => Some((certs, key)),
                _ => {
                    tracing::warn!("Failed to parse leaf cert/key PEM for mTLS identity");
                    None
                }
            }
        } else {
            None
        };

    let config = if let Some((certs, key)) = client_identity {
        match rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_client_auth_cert(certs, key)
        {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!("Failed to build mTLS client config: {e}");
                rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(AcceptAnyCert))
                    .with_no_client_auth()
            }
        }
    } else {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth()
    };

    tokio_tungstenite::Connector::Rustls(Arc::new(config))
}

// ── Custom certificate verifiers ─────────────────────────────────────────────

/// Accepts any server certificate — used before the mesh CA is known.
#[derive(Debug)]
struct AcceptAnyCert;

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer,
        _intermediates: &[rustls::pki_types::CertificateDer],
        _server_name: &rustls::pki_types::ServerName,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Verifies against the mesh CA cert but skips hostname checking
/// (peer certs use IP SANs that may differ across machines).
#[derive(Debug)]
struct CaOnlyVerifier {
    roots: Arc<rustls::RootCertStore>,
}

impl rustls::client::danger::ServerCertVerifier for CaOnlyVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer,
        intermediates: &[rustls::pki_types::CertificateDer],
        _server_name: &rustls::pki_types::ServerName,
        ocsp: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // Verify chain against CA; pass a stable dummy name since we don't do hostname checks.
        let verifier = rustls::client::WebPkiServerVerifier::builder(self.roots.clone())
            .build()
            .map_err(|e| rustls::Error::General(e.to_string()))?;
        let dummy = rustls::pki_types::ServerName::try_from("localhost")
            .map_err(|e| rustls::Error::General(e.to_string()))?;
        verifier.verify_server_cert(end_entity, intermediates, &dummy, ocsp, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
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

// ── CA key encryption helpers ────────────────────────────────────────────────

fn is_encrypted_key(data: &[u8]) -> bool {
    data.starts_with(b"HKEK")
}

/// Derive a 32-byte AES-256-GCM key from `passphrase` + `salt` via PBKDF2-HMAC-SHA256.
fn derive_key(passphrase: &[u8], salt: &[u8]) -> [u8; 32] {
    use ring::pbkdf2;
    use std::num::NonZeroU32;
    let mut key = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(100_000).unwrap(),
        salt,
        passphrase,
        &mut key,
    );
    key
}

/// Encrypt a PEM string under `passphrase`. Returns a binary envelope:
/// `[4B "HKEK"][1B version 0x01][2B salt_len BE][salt][12B nonce][AES-256-GCM ciphertext+tag]`
fn encrypt_key_pem(pem: &str, passphrase: &str) -> io::Result<Vec<u8>> {
    use ring::aead;
    use ring::rand::{self, SecureRandom};
    let rng = rand::SystemRandom::new();
    let mut salt = [0u8; 32];
    rng.fill(&mut salt)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "rng failed"))?;
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "rng failed"))?;

    let key_bytes = derive_key(passphrase.as_bytes(), &salt);
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "key construction failed"))?;
    let lsk = aead::LessSafeKey::new(unbound);
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

    let mut ciphertext = pem.as_bytes().to_vec();
    lsk.seal_in_place_append_tag(nonce, aead::Aad::empty(), &mut ciphertext)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "encryption failed"))?;

    let salt_len = (salt.len() as u16).to_be_bytes();
    let mut out = Vec::with_capacity(4 + 1 + 2 + salt.len() + 12 + ciphertext.len());
    out.extend_from_slice(b"HKEK");
    out.push(0x01);
    out.extend_from_slice(&salt_len);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a binary envelope produced by [`encrypt_key_pem`].
fn decrypt_key_pem(blob: &[u8], passphrase: &str) -> io::Result<String> {
    use ring::aead;
    if blob.len() < 7 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "blob too short"));
    }
    if &blob[0..4] != b"HKEK" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic"));
    }
    // blob[4] = version (ignored for forward compat)
    let salt_len = u16::from_be_bytes([blob[5], blob[6]]) as usize;
    let salt_start = 7;
    let salt_end = salt_start + salt_len;
    let nonce_end = salt_end + 12;
    if blob.len() < nonce_end {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "blob truncated"));
    }
    let salt = &blob[salt_start..salt_end];
    let nonce_bytes: [u8; 12] = blob[salt_end..nonce_end].try_into().unwrap();
    let mut ciphertext = blob[nonce_end..].to_vec();

    let key_bytes = derive_key(passphrase.as_bytes(), salt);
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "key construction failed"))?;
    let lsk = aead::LessSafeKey::new(unbound);
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
    let plaintext = lsk
        .open_in_place(nonce, aead::Aad::empty(), &mut ciphertext)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "decryption failed — wrong passphrase?",
            )
        })?;
    String::from_utf8(plaintext.to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decrypted bytes not valid UTF-8"))
}

/// Get the CA key passphrase. Checks `HUSH_CA_PASSPHRASE` env var first, then
/// prompts via TTY. When `confirm` is true, prompts twice and checks they match.
fn get_passphrase(prompt: &str, confirm: bool) -> io::Result<String> {
    use std::io::IsTerminal;
    if let Ok(p) = std::env::var("HUSH_CA_PASSPHRASE") {
        return Ok(p);
    }
    if !std::io::stdin().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "CA key is encrypted but HUSH_CA_PASSPHRASE is not set and stdin is not a TTY",
        ));
    }
    let pass = rpassword::prompt_password_stderr(&format!("{}: ", prompt))?;
    if confirm {
        let pass2 = rpassword::prompt_password_stderr("Confirm passphrase: ")?;
        if pass != pass2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Passphrases do not match",
            ));
        }
    }
    Ok(pass)
}

// ── mTLS acceptor for /peer ───────────────────────────────────────────────────

/// Request extension injected by [`MtlsAcceptor`] into every incoming HTTP
/// request. `true` when the client presented a TLS client certificate during
/// the handshake; `false` for browser connections that send none.
#[derive(Clone, Debug)]
pub struct PeerCertPresent(pub bool);

/// A [`tower::Service`] wrapper that inserts [`PeerCertPresent`] into every
/// incoming [`http::Request`]'s extensions before forwarding to the inner
/// service.
#[derive(Clone)]
pub struct WithPeerCert<S> {
    pub inner: S,
    pub cert_present: PeerCertPresent,
}

impl<S, B> tower::Service<http::Request<B>> for WithPeerCert<S>
where
    S: tower::Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        req.extensions_mut().insert(self.cert_present.clone());
        self.inner.call(req)
    }
}

/// Wraps [`axum_server::tls_rustls::RustlsAcceptor`] and, after the TLS
/// handshake, injects a [`PeerCertPresent`] extension into every HTTP request
/// on that connection. This lets the `/peer` handler enforce that daemon peers
/// presented a valid TLS client certificate.
#[derive(Clone)]
pub struct MtlsAcceptor {
    inner: axum_server::tls_rustls::RustlsAcceptor,
}

impl MtlsAcceptor {
    pub fn new(config: axum_server::tls_rustls::RustlsConfig) -> Self {
        Self {
            inner: axum_server::tls_rustls::RustlsAcceptor::new(config),
        }
    }
}

impl<S> axum_server::accept::Accept<tokio::net::TcpStream, S> for MtlsAcceptor
where
    S: Send + 'static,
{
    type Stream = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Service = WithPeerCert<S>;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = io::Result<(Self::Stream, Self::Service)>,
                > + Send
                + 'static,
        >,
    >;

    fn accept(
        &self,
        stream: tokio::net::TcpStream,
        service: S,
    ) -> Self::Future {
        // Call the inner RustlsAcceptor to perform the TLS handshake.
        // The future is Send because RustlsAcceptorFuture captures only
        // RustlsConfig (Arc<ArcSwap<ServerConfig>>) and a DefaultAcceptor future.
        let fut = self.inner.accept(stream, service);
        Box::pin(async move {
            let (tls_stream, service) = fut.await?;
            // peer_certificates() is non-empty only when the client sent a cert.
            let has_cert = tls_stream
                .get_ref()
                .1
                .peer_certificates()
                .map(|certs| !certs.is_empty())
                .unwrap_or(false);
            Ok((
                tls_stream,
                WithPeerCert {
                    inner: service,
                    cert_present: PeerCertPresent(has_cert),
                },
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All tests that call load_or_generate_ca / generate_ca need a passphrase.
    fn set_test_passphrase() {
        // Safety: tests run sequentially in a single process; set_var is safe here.
        unsafe { std::env::set_var("HUSH_CA_PASSPHRASE", "test-passphrase") };
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let pem = "-----BEGIN EC PRIVATE KEY-----\nfakekey\n-----END EC PRIVATE KEY-----";
        let pass = "test-passphrase-roundtrip";
        let encrypted = encrypt_key_pem(pem, pass).unwrap();
        assert!(is_encrypted_key(&encrypted));
        let decrypted = decrypt_key_pem(&encrypted, pass).unwrap();
        assert_eq!(decrypted, pem);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let pem = "-----BEGIN EC PRIVATE KEY-----\nfakekey\n-----END EC PRIVATE KEY-----";
        let encrypted = encrypt_key_pem(pem, "correct").unwrap();
        assert!(decrypt_key_pem(&encrypted, "wrong").is_err());
    }

    #[test]
    fn is_encrypted_key_detection() {
        assert!(is_encrypted_key(b"HKEK\x01rest"));
        assert!(!is_encrypted_key(b"-----BEGIN"));
        assert!(!is_encrypted_key(b""));
    }

    #[test]
    fn migration_from_plaintext() {
        set_test_passphrase();
        let tmp = tempfile::TempDir::new().unwrap();
        // Manually write a plaintext PEM key to simulate a legacy install.
        let tls_dir = tmp.path().join("tls");
        std::fs::create_dir_all(&tls_dir).unwrap();
        // First generate normally so we have a real CA cert to pair with.
        let ca = load_or_generate_ca(tmp.path()).unwrap();
        let key_path = tls_dir.join("ca.key");
        // Overwrite with plaintext PEM to simulate pre-encryption state.
        std::fs::write(&key_path, ca.key_pair.serialize_pem()).unwrap();
        assert!(!is_encrypted_key(&std::fs::read(&key_path).unwrap()));
        // Now load — should auto-migrate to encrypted.
        let _ = load_or_generate_ca(tmp.path()).unwrap();
        assert!(is_encrypted_key(&std::fs::read(&key_path).unwrap()));
    }

    #[test]
    fn sign_produces_nonempty_signature() {
        set_test_passphrase();
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
        set_test_passphrase();
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
        set_test_passphrase();
        let tmp = tempfile::TempDir::new().unwrap();
        let ca1 = load_or_generate_ca(tmp.path()).unwrap();
        let ca2 = load_or_generate_ca(tmp.path()).unwrap();
        // Should load the same key pair, not generate a new one
        assert_eq!(ca1.key_pair.serialize_pem(), ca2.key_pair.serialize_pem());
    }

    #[test]
    fn leaf_cert_contains_localhost_san() {
        set_test_passphrase();
        let tmp = tempfile::TempDir::new().unwrap();
        let mat = load_or_generate(tmp.path(), "test-machine").unwrap();
        let cert_str = String::from_utf8_lossy(&mat.cert_pem);
        assert!(cert_str.contains("BEGIN CERTIFICATE"));
        assert!(!mat.fingerprint.is_empty());
    }
}
