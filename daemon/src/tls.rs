use std::io;
use std::path::Path;

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

fn pem_to_der(pem: &str) -> io::Result<Vec<u8>> {
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

/// Replace the local CA with a mesh CA received from a peer. Deletes the
/// existing leaf cert so it gets regenerated on next `load_or_generate`.
pub fn replace_ca(hush_dir: &Path, cert_pem: &str, key_pem: &str) -> io::Result<()> {
    let tls_dir = hush_dir.join("tls");
    std::fs::create_dir_all(&tls_dir)?;

    std::fs::write(tls_dir.join("ca.crt"), cert_pem)?;
    std::fs::write(tls_dir.join("ca.key"), key_pem)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tls_dir.join("ca.key"),
            std::fs::Permissions::from_mode(0o600),
        )?;
        std::fs::set_permissions(
            tls_dir.join("ca.crt"),
            std::fs::Permissions::from_mode(0o644),
        )?;
    }

    // Invalidate leaf cert so it gets re-signed by the new CA
    let _ = std::fs::remove_file(tls_dir.join("cert.pem"));
    let _ = std::fs::remove_file(tls_dir.join("key.pem"));
    // Remove trusted marker — needs re-trust with new CA
    let _ = std::fs::remove_file(tls_dir.join(".trusted"));

    tracing::info!("Replaced local CA with mesh CA");
    Ok(())
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
