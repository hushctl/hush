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
/// SANs include localhost, 127.0.0.1, ::1, and `machine_name`.
pub fn load_or_generate(hush_dir: &Path, machine_name: &str) -> io::Result<TlsMaterial> {
    let tls_dir = hush_dir.join("tls");
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");

    // Try loading existing
    if cert_path.exists() && key_path.exists() {
        if let Ok(mat) = try_load(&cert_path, &key_path) {
            return Ok(mat);
        }
        // Corrupted — fall through to regenerate
        tracing::warn!("TLS cert/key could not be parsed — regenerating");
    }

    std::fs::create_dir_all(&tls_dir)?;

    // Build SANs
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

    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(sans)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let cert_pem = cert.pem().into_bytes();
    let key_pem = key_pair.serialize_pem().into_bytes();

    // Write cert (readable) and key (owner-only)
    std::fs::write(&cert_path, &cert_pem)?;
    std::fs::write(&key_path, &key_pem)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        std::fs::set_permissions(&cert_path, std::fs::Permissions::from_mode(0o644))?;
    }

    let fingerprint = fingerprint_pem(&cert_pem)?;
    Ok(TlsMaterial { cert_pem, key_pem, fingerprint })
}

fn try_load(cert_path: &Path, key_path: &Path) -> io::Result<TlsMaterial> {
    let cert_pem = std::fs::read(cert_path)?;
    let key_pem = std::fs::read(key_path)?;

    // Validate they parse — rustls-pemfile will error on corrupt data
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
