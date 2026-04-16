//! Mesh join flow — secure enrollment of a new daemon into an existing mesh.
//!
//! ## Flow
//!
//! 1. On the CA-holding machine, run `hush invite` to generate a short-lived
//!    join token and print it to stdout.
//! 2. On the joining machine, run
//!    `hush --join wss://peer:9111/peer --join-token hush-join-XXXX-XXXX`.
//!    The daemon POSTs to the existing peer's `/join` endpoint, receives a
//!    signed leaf cert + CA cert, writes them to `~/.hush/tls/`, and starts.
//!
//! ## Security properties
//! - Join tokens are single-use and expire in 10 minutes.
//! - The CA private key never leaves the CA-origin machine.
//! - The `/join` endpoint is unauthenticated — the token is the secret.

use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const TOKEN_TTL_SECS: u64 = 600; // 10 minutes
const TOKEN_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const TOKEN_SEGMENT_LEN: usize = 4;

// ─── Token generation & validation ───────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JoinToken {
    pub token: String,
    pub expires_at: u64,
}

fn token_file(hush_dir: &Path) -> std::path::PathBuf {
    hush_dir.join("join_tokens.json")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_tokens(hush_dir: &Path) -> Vec<JoinToken> {
    let path = token_file(hush_dir);
    let Ok(data) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    serde_json::from_str(&data).unwrap_or_default()
}

fn save_tokens(hush_dir: &Path, tokens: &[JoinToken]) {
    let path = token_file(hush_dir);
    if let Ok(json) = serde_json::to_string_pretty(tokens) {
        let _ = std::fs::write(&path, json);
    }
}

/// Generate a new join token (format: `hush-join-XXXX-XXXX`), persist it,
/// and return the token string. Cleans up expired tokens as a side effect.
pub fn generate_token(hush_dir: &Path) -> io::Result<String> {
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; 8];
    use ring::rand::SecureRandom;
    rng.fill(&mut bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "rng failure"))?;

    let mut token_chars = [0u8; 8];
    for (i, b) in bytes.iter().enumerate() {
        token_chars[i] = TOKEN_CHARS[*b as usize % TOKEN_CHARS.len()];
    }

    let seg1 = std::str::from_utf8(&token_chars[..TOKEN_SEGMENT_LEN]).unwrap();
    let seg2 = std::str::from_utf8(&token_chars[TOKEN_SEGMENT_LEN..]).unwrap();
    let token = format!("hush-join-{seg1}-{seg2}");

    let now = now_secs();
    let expires_at = now + TOKEN_TTL_SECS;

    let mut tokens = load_tokens(hush_dir);
    // Prune expired tokens
    tokens.retain(|t| t.expires_at > now);
    tokens.push(JoinToken {
        token: token.clone(),
        expires_at,
    });
    save_tokens(hush_dir, &tokens);

    Ok(token)
}

/// Validate and consume a join token. Returns `Ok(())` if the token is valid,
/// `Err` if it is expired, already used, or not found.
pub fn consume_token(hush_dir: &Path, token: &str) -> io::Result<()> {
    let now = now_secs();
    let mut tokens = load_tokens(hush_dir);
    // Prune expired tokens first
    tokens.retain(|t| t.expires_at > now);

    let pos = tokens.iter().position(|t| t.token == token);
    match pos {
        Some(i) => {
            tokens.remove(i);
            save_tokens(hush_dir, &tokens);
            Ok(())
        }
        None => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid or expired join token",
        )),
    }
}

// ─── HTTP request/response types ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    pub token: String,
    pub machine_id: String,
    /// IP addresses and hostnames to include as SANs in the issued leaf cert.
    #[serde(default)]
    pub sans: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinResponse {
    pub ca_cert_pem: String,
    pub leaf_cert_pem: String,
    pub leaf_key_pem: String,
}

// ─── Axum handler ────────────────────────────────────────────────────────────

pub async fn join_handler(
    axum::extract::State(state): axum::extract::State<crate::JoinHandlerState>,
    axum::Json(req): axum::Json<JoinRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    // Validate + consume the token
    if let Err(e) = consume_token(&state.hush_dir, &req.token) {
        tracing::warn!("Join request from '{}' rejected: {e}", req.machine_id);
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }

    tracing::info!(
        "Join request from '{}' — issuing leaf cert",
        req.machine_id
    );

    // Sign a new leaf cert for the joining machine
    match issue_leaf_cert(&state.hush_dir, &req.machine_id, &req.sans) {
        Ok(resp) => axum::Json(resp).into_response(),
        Err(e) => {
            tracing::warn!("Failed to issue leaf cert for '{}': {e}", req.machine_id);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Generate a new keypair and leaf cert for a joining machine, signed by our CA.
fn issue_leaf_cert(
    hush_dir: &Path,
    machine_id: &str,
    extra_sans: &[String],
) -> io::Result<JoinResponse> {
    use rcgen::{CertificateParams, DnType, KeyPair, SanType};

    let ca_bundle = crate::tls::load_or_generate_ca(hush_dir)?;
    let ca_cert_pem = std::fs::read_to_string(ca_bundle.cert_pem_path)?;

    let leaf_key =
        KeyPair::generate().map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, machine_id);
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2035, 1, 1);

    // Always include localhost SANs
    params.subject_alt_names = vec![
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
        SanType::DnsName("localhost".try_into().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("invalid SAN: {e}"))
        })?),
    ];

    // Add caller-supplied SANs (IPs and hostnames)
    for san in extra_sans {
        if let Ok(ip) = san.parse::<std::net::IpAddr>() {
            params.subject_alt_names.push(SanType::IpAddress(ip));
        } else {
            match san.as_str().try_into() {
                Ok(dns) => params.subject_alt_names.push(SanType::DnsName(dns)),
                Err(e) => tracing::warn!("Skipping invalid SAN '{san}': {e}"),
            }
        }
    }

    // Add all local non-loopback IPs as SANs (same as main cert generation)
    if let Ok(addrs) = if_addrs::get_if_addrs() {
        for iface in addrs {
            if !iface.is_loopback() {
                params
                    .subject_alt_names
                    .push(SanType::IpAddress(iface.ip()));
            }
        }
    }

    let leaf_cert = params
        .signed_by(&leaf_key, &ca_bundle.cert, &ca_bundle.key_pair)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    Ok(JoinResponse {
        ca_cert_pem,
        leaf_cert_pem: leaf_cert.pem(),
        leaf_key_pem: leaf_key.serialize_pem(),
    })
}

// ─── Joining machine: POST to /join and write received certs ─────────────────

/// POST to an existing peer's `/join` endpoint with a join token, receive a
/// signed leaf cert and CA cert, write them to `~/.hush/tls/`.
pub async fn perform_join(
    peer_base_url: &str,
    join_token: &str,
    machine_id: &str,
    hush_dir: &Path,
) -> io::Result<()> {
    // Build the /join URL from the peer base URL
    // peer_base_url may be wss://host:9111/peer or wss://host:9111/ws — strip path
    let base = peer_base_url
        .trim_end_matches("/peer")
        .trim_end_matches("/ws");
    let http_base = base
        .replace("wss://", "https://")
        .replace("ws://", "http://");
    let join_url = format!("{http_base}/join");

    // Collect local IPs for SANs
    let sans: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.is_loopback())
        .map(|a| a.ip().to_string())
        .collect();

    let req_body = serde_json::json!({
        "token": join_token,
        "machine_id": machine_id,
        "sans": sans,
    });

    // Use rustls with dangerous cert acceptance for the join request (we don't
    // have the CA yet — bootstrapping). After joining we'll have the CA cert.
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let resp = client
        .post(&join_url)
        .json(&req_body)
        .send()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("join POST failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("join rejected: HTTP {status}: {body}"),
        ));
    }

    let join_resp: JoinResponse = resp
        .json()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid response: {e}")))?;

    // Write received certs to ~/.hush/tls/
    let tls_dir = hush_dir.join("tls");
    std::fs::create_dir_all(&tls_dir)?;

    std::fs::write(tls_dir.join("ca.crt"), &join_resp.ca_cert_pem)?;
    std::fs::write(tls_dir.join("cert.pem"), &join_resp.leaf_cert_pem)?;
    std::fs::write(tls_dir.join("key.pem"), &join_resp.leaf_key_pem)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tls_dir.join("key.pem"),
            std::fs::Permissions::from_mode(0o600),
        )?;
        std::fs::set_permissions(
            tls_dir.join("ca.crt"),
            std::fs::Permissions::from_mode(0o644),
        )?;
        std::fs::set_permissions(
            tls_dir.join("cert.pem"),
            std::fs::Permissions::from_mode(0o644),
        )?;
    }

    tracing::info!(
        "✓ Joined mesh — CA cert and leaf cert written to {}",
        tls_dir.display()
    );

    // Install the received CA into the OS trust store
    let ca_cert_path = tls_dir.join("ca.crt");
    match crate::trust::install_ca(&ca_cert_path) {
        Ok(()) => {
            crate::trust::write_trusted_marker(hush_dir);
            tracing::info!("✓ Mesh CA trusted — browsers will accept certificates");
        }
        Err(e) => {
            tracing::warn!("CA trust install failed: {e} — run `hush trust` manually");
        }
    }

    Ok(())
}
