//! P2P daemon upgrade: stream hush + hush-hook binaries from a newer peer to
//! an older one over the existing WebSocket infrastructure.
//!
//! # Wire protocol (source → destination)
//!
//! ```text
//! text:   upgrade_offer   { upgrade_id, from_machine_id, version, platform, total_bytes }
//! text:   upgrade_ack     { machine_id, upgrade_id }                ← destination replies
//! binary: <tar.gz frames>                                           (N × 256 KB)
//! text:   upgrade_commit  { upgrade_id }
//! text:   upgrade_complete { machine_id, upgrade_id, ... }          ← destination replies
//! ```
//!
//! On success the destination atomically replaces its own binaries and exits
//! so that launchd (KeepAlive) restarts it with the new binary.

use std::collections::HashMap;
use std::io::Read as IoRead;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use flate2::write::GzEncoder;
use flate2::Compression;
use futures_util::{SinkExt, StreamExt};
use tar::Builder as TarBuilder;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;
use tracing::{info, warn};

use crate::protocol::ServerMessage;
use crate::transfer::transfers_dir;

const BINARY_CHUNK_SIZE: usize = 256 * 1024;
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const ACK_TIMEOUT: Duration = Duration::from_secs(30);
const COMPLETE_TIMEOUT: Duration = Duration::from_secs(120);

// ─── Destination-side per-upgrade state ──────────────────────────────────────

pub struct InboundUpgrade {
    pub upgrade_id: String,
    pub from_machine_id: String,
    pub version: String,
    pub total_bytes: u64,
    pub bytes_received: u64,
    pub file: Option<std::fs::File>,
    pub temp_path: PathBuf,
}

impl InboundUpgrade {
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        if let Some(ref mut f) = self.file {
            if let Err(e) = std::io::Write::write_all(f, bytes) {
                warn!("Upgrade {}: write_bytes failed: {e}", self.upgrade_id);
            }
            self.bytes_received += bytes.len() as u64;
        }
    }

    pub fn close_file(&mut self) {
        drop(self.file.take());
    }
}

pub type InboundUpgrades = Arc<Mutex<HashMap<String, InboundUpgrade>>>;

pub fn new_inbound_upgrades() -> InboundUpgrades {
    Arc::new(Mutex::new(HashMap::new()))
}

// ─── TLS connector ────────────────────────────────────────────────────────────

fn make_tls_connector() -> Connector {
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("Failed to build TLS connector");
    Connector::NativeTls(tls.into())
}

// ─── Platform identifier ─────────────────────────────────────────────────────

pub fn local_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

// ─── Source side ─────────────────────────────────────────────────────────────

/// Package the running `hush` and its sibling `hush-hook` into a tar.gz in
/// `tmp_dir`. Returns `(path, compressed_byte_size)`.
fn package_binaries(tmp_dir: &Path) -> Result<(PathBuf, u64), String> {
    let cur_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let bin_dir = cur_exe
        .parent()
        .ok_or("cannot determine binary directory")?;

    let hook_path = bin_dir.join("hush-hook");
    if !hook_path.exists() {
        return Err(format!("hush-hook not found at {}", hook_path.display()));
    }

    let tarball_path = tmp_dir.join("hush-upgrade.tar.gz");
    {
        let file =
            std::fs::File::create(&tarball_path).map_err(|e| format!("create tarball: {e}"))?;
        let gz = GzEncoder::new(file, Compression::fast());
        let mut tar = TarBuilder::new(gz);
        tar.append_path_with_name(&cur_exe, "hush")
            .map_err(|e| format!("tar hush: {e}"))?;
        tar.append_path_with_name(&hook_path, "hush-hook")
            .map_err(|e| format!("tar hush-hook: {e}"))?;
        tar.into_inner()
            .map_err(|e| format!("tar finish: {e}"))?
            .finish()
            .map_err(|e| format!("gz finish: {e}"))?;
    }

    let size = std::fs::metadata(&tarball_path)
        .map_err(|e| format!("stat tarball: {e}"))?
        .len();

    Ok((tarball_path, size))
}

/// Dial `dest_url`, offer and stream our binaries, wait for UpgradeComplete.
/// Spawned as a tokio task from ws.rs when a `PeerUpgrade` request is received.
pub async fn send_upgrade(
    dest_url: String,
    dest_machine_id: String,
    machine_id: String,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
) {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let upgrade_id = {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("up-{machine_id}-{ts}")
    };

    info!("Upgrade {upgrade_id}: packaging binaries for {dest_machine_id} ({dest_url})");

    let fail = |msg: String| {
        warn!("Upgrade {upgrade_id}: failed — {msg}");
        let _ = tx.send(ServerMessage::UpgradeError {
            machine_id: machine_id.clone(),
            upgrade_id: upgrade_id.clone(),
            message: msg,
        });
    };

    // 1. Package binaries into a temp tar.gz
    let tmp_dir = transfers_dir(&state_path);
    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
        fail(format!("create tmp dir: {e}"));
        return;
    }
    let (tarball_path, total_bytes) = match package_binaries(&tmp_dir) {
        Ok(r) => r,
        Err(e) => {
            fail(e);
            return;
        }
    };

    // 2. Dial destination
    let connector = make_tls_connector();
    let (mut ws, _) = match tokio::time::timeout(
        DIAL_TIMEOUT,
        connect_async_tls_with_config(&dest_url, None, false, Some(connector)),
    )
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            fail(format!("ws connect: {e}"));
            let _ = std::fs::remove_file(&tarball_path);
            return;
        }
        Err(_) => {
            fail("connect timeout".into());
            let _ = std::fs::remove_file(&tarball_path);
            return;
        }
    };

    // 3. Send UpgradeOffer
    let offer = serde_json::json!({
        "type": "upgrade_offer",
        "upgrade_id": &upgrade_id,
        "from_machine_id": &machine_id,
        "version": &version,
        "platform": local_platform(),
        "total_bytes": total_bytes,
    });
    if let Err(e) = ws.send(Message::Text(offer.to_string().into())).await {
        fail(format!("send offer: {e}"));
        let _ = std::fs::remove_file(&tarball_path);
        return;
    }

    // 4. Wait for UpgradeAck
    if let Err(e) = wait_for_ack(&mut ws, &upgrade_id).await {
        fail(format!("no UpgradeAck: {e}"));
        let _ = std::fs::remove_file(&tarball_path);
        return;
    }
    info!("Upgrade {upgrade_id}: ack'd by {dest_machine_id}, streaming {total_bytes} bytes");

    // 5. Stream binary frames
    let mut bytes_sent = 0u64;
    let stream_result = stream_file(
        &mut ws,
        &tarball_path,
        &mut bytes_sent,
        total_bytes,
        &tx,
        &machine_id,
        &upgrade_id,
        &dest_machine_id,
    )
    .await;
    let _ = std::fs::remove_file(&tarball_path);
    if let Err(e) = stream_result {
        fail(format!("stream: {e}"));
        return;
    }

    // 6. Send UpgradeCommit
    let commit = serde_json::json!({
        "type": "upgrade_commit",
        "upgrade_id": &upgrade_id,
    });
    if let Err(e) = ws.send(Message::Text(commit.to_string().into())).await {
        fail(format!("send commit: {e}"));
        return;
    }

    // 7. Wait for UpgradeComplete (dest will exit after sending, so we expect EOF soon after)
    match wait_for_complete(&mut ws, &upgrade_id).await {
        Ok(()) => {
            info!("Upgrade {upgrade_id}: {dest_machine_id} upgraded to v{version}");
            let _ = tx.send(ServerMessage::UpgradeComplete {
                machine_id: machine_id.clone(),
                upgrade_id,
                dest_machine_id,
                version,
            });
        }
        Err(e) => {
            fail(format!("UpgradeComplete not received: {e}"));
        }
    }
    let _ = ws.close(None).await;
}

async fn wait_for_ack(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    upgrade_id: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ACK_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, ws.next())
            .await
            .map_err(|_| "timeout waiting for upgrade_ack")?
            .ok_or("stream ended")?
            .map_err(|e| format!("ws recv: {e}"))?;
        if let Message::Text(text) = msg {
            let v: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("json: {e}"))?;
            if v.get("type").and_then(|t| t.as_str()) == Some("upgrade_ack")
                && v.get("upgrade_id").and_then(|i| i.as_str()) == Some(upgrade_id)
            {
                return Ok(());
            }
        }
    }
}

async fn stream_file(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    path: &Path,
    bytes_sent: &mut u64,
    total_bytes: u64,
    tx: &broadcast::Sender<ServerMessage>,
    machine_id: &str,
    upgrade_id: &str,
    dest_machine_id: &str,
) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open tarball: {e}"))?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = vec![0u8; BINARY_CHUNK_SIZE];

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("read chunk: {e}"))?;
        if n == 0 {
            break;
        }
        ws.send(Message::Binary(buf[..n].to_vec().into()))
            .await
            .map_err(|e| format!("send binary frame: {e}"))?;
        *bytes_sent += n as u64;
        // Broadcast progress roughly every 1 MB
        if *bytes_sent % (1024 * 1024) < BINARY_CHUNK_SIZE as u64 {
            let _ = tx.send(ServerMessage::UpgradeProgress {
                machine_id: machine_id.to_string(),
                upgrade_id: upgrade_id.to_string(),
                dest_machine_id: dest_machine_id.to_string(),
                bytes_sent: *bytes_sent,
                total_bytes,
            });
        }
    }
    Ok(())
}

async fn wait_for_complete(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    upgrade_id: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + COMPLETE_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, ws.next())
            .await
            .map_err(|_| "timeout waiting for upgrade_complete")?
            .ok_or("stream ended")?
            .map_err(|e| format!("ws recv: {e}"))?;
        if let Message::Text(text) = msg {
            let v: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("json: {e}"))?;
            if v.get("upgrade_id").and_then(|i| i.as_str()) == Some(upgrade_id) {
                match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "upgrade_complete" => return Ok(()),
                    "upgrade_error" => {
                        let msg = v
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown error");
                        return Err(msg.to_string());
                    }
                    _ => {}
                }
            }
        }
    }
}

// ─── Destination side ─────────────────────────────────────────────────────────

/// Extract the upgrade tarball, atomically replace binaries, broadcast
/// UpgradeComplete, then exit so launchd restarts with the new binary.
pub async fn apply_upgrade(
    mut upgrade: InboundUpgrade,
    tx: broadcast::Sender<ServerMessage>,
    machine_id: String,
) {
    let upgrade_id = upgrade.upgrade_id.clone();
    let version = upgrade.version.clone();

    info!(
        "Upgrade {upgrade_id}: applying v{version} from {}",
        upgrade.from_machine_id
    );
    upgrade.close_file();

    let temp_path = upgrade.temp_path.clone();
    let result =
        tokio::task::spawn_blocking(move || crate::upgrade::apply_archive(&temp_path)).await;

    let _ = std::fs::remove_file(&upgrade.temp_path);

    match result {
        Ok(Ok(updated)) => {
            for p in &updated {
                info!("Upgrade {upgrade_id}: updated {p}");
            }
            let _ = tx.send(ServerMessage::UpgradeComplete {
                machine_id: machine_id.clone(),
                upgrade_id: upgrade_id.clone(),
                dest_machine_id: machine_id,
                version: version.clone(),
            });
            info!("Upgraded to v{version}. Restarting...");
            // Brief pause so the UpgradeComplete message reaches the source before we exit.
            tokio::time::sleep(Duration::from_secs(2)).await;

            // If binaries were written to a fallback directory (e.g. ~/.hush/bin)
            // rather than next to the current exe, exec the new binary directly so
            // the process restarts from the new location without requiring launchd
            // to know about the fallback path.
            let cur_exe = std::env::current_exe().ok();
            let new_hush = updated.iter().find(|p| {
                let p = std::path::Path::new(p.as_str());
                p.file_name().map(|n| n == "hush").unwrap_or(false)
            });
            if let (Some(cur), Some(new_path)) = (cur_exe, new_hush) {
                if std::path::Path::new(new_path.as_str()) != cur {
                    info!("Upgrade {upgrade_id}: binary moved to {new_path}, exec'ing new binary");
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::CommandExt;
                        let saved_args = crate::DAEMON_ARGS
                            .get()
                            .map(|a| a.as_slice())
                            .unwrap_or(&[]);
                        // argv[0] = new path, rest = original args (skip old argv[0])
                        let err = std::process::Command::new(new_path)
                            .args(saved_args.iter().skip(1))
                            .exec();
                        warn!("Upgrade {upgrade_id}: exec failed: {err}");
                    }
                    // Fallthrough to plain exit if exec failed or non-Unix
                }
            }

            std::process::exit(0);
        }
        Ok(Err(e)) => {
            warn!("Upgrade {upgrade_id}: apply failed — {e}");
            let _ = tx.send(ServerMessage::UpgradeError {
                machine_id,
                upgrade_id,
                message: e.to_string(),
            });
        }
        Err(e) => {
            warn!("Upgrade {upgrade_id}: spawn_blocking panicked — {e}");
            let _ = tx.send(ServerMessage::UpgradeError {
                machine_id,
                upgrade_id,
                message: format!("internal error: {e}"),
            });
        }
    }
}
