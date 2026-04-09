use std::io;
use std::path::Path;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use sha2::{Digest, Sha256};

pub struct TlsMaterial {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    /// SHA-256 fingerprint of the DER cert, hex-encoded (colon-separated bytes).
    pub fingerprint: String,
}

/// Load existing cert+key from `{hush_dir}/tls/`, or generate and persist them.
///
/// SANs include:
/// - localhost, 127.0.0.1, ::1
/// - `machine_name` (the daemon's human-readable name / hostname)
/// - Every non-loopback IP address currently assigned to a network interface
///
/// If the cert exists but its SANs no longer cover all current IPs (e.g. a new
/// Tailscale IP appeared), delete and regenerate so the new IP is covered.
pub fn load_or_generate(hush_dir: &Path, machine_name: &str) -> io::Result<TlsMaterial> {
    let tls_dir = hush_dir.join("tls");
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");

    let desired_sans = build_sans(machine_name);

    // Try loading existing cert; regenerate if corrupt or if SANs are stale
    if cert_path.exists() && key_path.exists() {
        match try_load(&cert_path, &key_path) {
            Ok(mat) => {
                // Check whether the cert already covers all desired SANs
                if sans_are_covered(&cert_path, &desired_sans) {
                    return Ok(mat);
                }
                tracing::info!(
                    "TLS cert missing some SANs (new IPs?) — regenerating to cover: {}",
                    desired_sans.join(", ")
                );
            }
            Err(_) => {
                tracing::warn!("TLS cert/key could not be parsed — regenerating");
            }
        }
    }

    generate_and_save(&tls_dir, &cert_path, &key_path, desired_sans)
}

fn build_sans(machine_name: &str) -> Vec<String> {
    let mut sans = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];

    if !machine_name.is_empty()
        && machine_name != "localhost"
        && machine_name != "unknown"
    {
        sans.push(machine_name.to_string());
    }

    // Include all non-loopback interface IPs (LAN, Tailscale, etc.)
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
/// Uses a simple string scan of the PEM text — good enough for our purposes.
fn sans_are_covered(cert_path: &Path, wanted: &[String]) -> bool {
    let Ok(pem) = std::fs::read_to_string(cert_path) else { return false };
    // Decode and check subject alternative names via DER
    // Fall back to always-regenerate if we can't parse
    let Ok(der) = pem_to_der(&pem) else { return false };
    // rcgen doesn't expose SAN parsing; use a simple check: re-parse with
    // rustls-pemfile and look for IP/DNS strings in the raw DER bytes.
    // This is a best-effort check — false negatives cause a harmless regeneration.
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
    certs.into_iter()
        .next()
        .map(|d| d.to_vec())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no cert in pem"))
}

fn generate_and_save(
    tls_dir: &Path,
    cert_path: &Path,
    key_path: &Path,
    sans: Vec<String>,
) -> io::Result<TlsMaterial> {
    std::fs::create_dir_all(tls_dir)?;

    tracing::info!("Generating self-signed TLS cert with SANs: {}", sans.join(", "));

    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(sans)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let cert_pem = cert.pem().into_bytes();
    let key_pem = key_pair.serialize_pem().into_bytes();

    std::fs::write(cert_path, &cert_pem)?;
    std::fs::write(key_path, &key_pem)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))?;
        std::fs::set_permissions(cert_path, std::fs::Permissions::from_mode(0o644))?;
    }

    let fingerprint = fingerprint_pem(&cert_pem)?;
    Ok(TlsMaterial { cert_pem, key_pem, fingerprint })
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
    Ok(TlsMaterial { cert_pem, key_pem, fingerprint })
}

fn fingerprint_pem(cert_pem: &[u8]) -> io::Result<String> {
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
