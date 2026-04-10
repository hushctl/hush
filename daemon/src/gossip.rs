//! Peer gossip — each daemon periodically dials all known peers, sends a
//! `peer_hello`, receives a `peer_list`, and merges the results. New peers
//! learned this way are persisted to state.json so the mesh is remembered
//! across restarts. Stale peers (no contact in 24h) are pruned.
//!
//! The gossip loop also handles the initial `--join` seed: those URLs are
//! already in `state.peers` by the time this task starts (inserted in main.rs),
//! so the first tick dials them automatically.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::{interval, MissedTickBehavior};
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;
use tracing::{debug, info};

use crate::peer_upgrade;
use crate::protocol::ServerMessage;
use crate::state::{DaemonState, PeerInfo};

/// Parse a semver string "X.Y.Z" into a comparable tuple.
/// Unknown or malformed versions return (0, 0, 0).
fn parse_version(s: &str) -> (u32, u32, u32) {
    let mut parts = s.splitn(3, '.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

/// Build a TLS connector that accepts any certificate from gossip peers.
/// Network-layer trust (Tailscale, etc.) provides peer authenticity; TLS here
/// only prevents cleartext interception. TOFU pinning is a future follow-up.
fn make_tls_connector() -> Connector {
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("Failed to build native TLS connector");
    Connector::NativeTls(tls.into())
}

const GOSSIP_INTERVAL: Duration = Duration::from_secs(30);
const DIAL_TIMEOUT: Duration = Duration::from_secs(5);
/// Prune peers not seen for 24 hours.
const STALE_AFTER_SECS: u64 = 24 * 3600;

pub fn spawn_gossip(
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    auto_upgrade: bool,
) {
    // Tracks which peers currently have an upgrade in flight so we don't
    // spam the same peer every 30-second gossip round.
    let upgrading: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    tokio::spawn(async move {
        let mut ticker = interval(GOSSIP_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            run_gossip_round(
                Arc::clone(&state),
                &state_path,
                &tx,
                auto_upgrade,
                Arc::clone(&upgrading),
            ).await;
        }
    });
    info!("spawned gossip task (interval={}s, auto_upgrade={auto_upgrade})", GOSSIP_INTERVAL.as_secs());
}

async fn run_gossip_round(
    state: Arc<RwLock<DaemonState>>,
    state_path: &PathBuf,
    tx: &broadcast::Sender<ServerMessage>,
    auto_upgrade: bool,
    upgrading: Arc<Mutex<HashSet<String>>>,
) {
    // Snapshot peers + our own identity before releasing the lock.
    let (machine_id, advertise_url, peers) = {
        let s = state.read().await;
        (s.machine_id.clone(), s.advertise_url.clone(), s.peers.clone())
    };

    if peers.is_empty() {
        return;
    }

    debug!("gossip round: dialling {} peer(s)", peers.len());

    let mut newly_learned: Vec<PeerInfo> = Vec::new();
    // (machine_id, version) for each successfully contacted peer
    let mut contacted: Vec<(String, String)> = Vec::new();
    // Placeholder entries that turned out to point at ourselves — remove them.
    let mut self_placeholder_ids: Vec<String> = Vec::new();

    for peer in &peers {
        if peer.url.is_empty() {
            continue;
        }
        // Skip our own advertise URL to avoid dialing ourselves
        if !advertise_url.is_empty() && peer.url == advertise_url {
            self_placeholder_ids.push(peer.machine_id.clone());
            continue;
        }
        match dial_peer(&machine_id, &advertise_url, &peers, &peer.url).await {
            Ok((responder_id, responder_version, received_peers)) => {
                // If the peer responded with our own machine_id it's a self-dial
                // (e.g. stale --join seed pointing at our own IP or localhost).
                if responder_id == machine_id {
                    debug!("gossip: {} ({}) resolved to ourselves — removing placeholder", peer.machine_id, peer.url);
                    self_placeholder_ids.push(peer.machine_id.clone());
                    continue;
                }
                contacted.push((responder_id.clone(), responder_version));
                for rp in received_peers {
                    // Don't add ourselves, and skip blank URLs
                    if rp.machine_id != machine_id && !rp.url.is_empty() {
                        newly_learned.push(rp);
                    }
                }
            }
            Err(e) => {
                debug!("gossip dial failed for {} ({}): {e}", peer.machine_id, peer.url);
            }
        }
    }

    // Merge results + update last_seen + store versions + prune stale + remove self-placeholders
    {
        let mut s = state.write().await;
        for (mid, ver) in &contacted {
            s.touch_peer(mid);
            if let Some(p) = s.peers.iter_mut().find(|p| &p.machine_id == mid) {
                p.version = ver.clone();
            }
        }
        s.merge_peers(newly_learned);
        // Remove any entries we discovered were pointing at ourselves
        for mid in &self_placeholder_ids {
            s.peers.retain(|p| &p.machine_id != mid);
        }
        s.prune_stale(STALE_AFTER_SECS);
        s.save(state_path);
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let contacted_ids: Vec<String> = contacted.iter().map(|(mid, _)| mid.clone()).collect();
    if !contacted_ids.is_empty() {
        info!("gossip round complete: contacted [{}]", contacted_ids.join(", "));
    }

    // Log version mismatches and optionally push upgrades to older peers.
    let our_ver = parse_version(current_version);
    for (mid, ver) in &contacted {
        if ver.is_empty() {
            continue;
        }
        let peer_ver = parse_version(ver);
        if peer_ver != our_ver {
            info!("peer {mid} running v{ver} (we are v{current_version})");
        }
        if auto_upgrade && peer_ver < our_ver {
            // Only push if no upgrade is already in flight for this peer.
            let mut in_flight = upgrading.lock().await;
            if in_flight.contains(mid.as_str()) {
                debug!("upgrade for {mid} already in progress — skipping");
                continue;
            }

            // Look up the peer's URL and our own machine_id from state.
            let (our_machine_id, peer_url) = {
                let s = state.read().await;
                let url = s.peers.iter()
                    .find(|p| &p.machine_id == mid)
                    .map(|p| p.url.clone());
                (s.machine_id.clone(), url)
            };
            let Some(dest_url) = peer_url else { continue };

            in_flight.insert(mid.clone());
            drop(in_flight); // release lock before spawning

            info!("auto-upgrade: pushing v{current_version} to {mid}");

            let mid_clone = mid.clone();
            let upgrading2 = Arc::clone(&upgrading);
            let tx2 = tx.clone();
            let sp = state_path.clone();
            tokio::spawn(async move {
                peer_upgrade::send_upgrade(
                    dest_url, mid_clone.clone(), our_machine_id, sp, tx2,
                ).await;
                // Remove from in-flight set whether it succeeded or failed.
                upgrading2.lock().await.remove(&mid_clone);
            });
        }
    }
}

/// Open a temporary WebSocket to `url`, send `peer_hello`, read the `peer_list`
/// response, close, and return `(responder_machine_id, responder_version, received_peers)`.
async fn dial_peer(
    my_machine_id: &str,
    my_url: &str,
    my_peers: &[PeerInfo],
    peer_url: &str,
) -> Result<(String, String, Vec<PeerInfo>), String> {
    let hello = serde_json::json!({
        "type": "peer_hello",
        "machine_id": my_machine_id,
        "url": my_url,
        "peers": my_peers,
        "version": env!("CARGO_PKG_VERSION"),
    });

    let connector = make_tls_connector();
    let connect_fut = connect_async_tls_with_config(peer_url, None, false, Some(connector));
    let (mut ws, _) = tokio::time::timeout(DIAL_TIMEOUT, connect_fut)
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| format!("ws connect error: {e}"))?;

    // Send peer_hello
    ws.send(Message::Text(hello.to_string().into()))
        .await
        .map_err(|e| format!("send error: {e}"))?;

    // Read one response (peer_list)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let response_fut = ws.next();
    let msg = tokio::time::timeout(DIAL_TIMEOUT, response_fut)
        .await
        .map_err(|_| "response timeout".to_string())?
        .ok_or("stream ended")?
        .map_err(|e| format!("recv error: {e}"))?;

    let _ = ws.close(None).await;

    let text = match msg {
        Message::Text(t) => t.to_string(),
        _ => return Err("unexpected non-text response".to_string()),
    };

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("json parse: {e}"))?;

    if value.get("type").and_then(|v| v.as_str()) != Some("peer_list") {
        return Err(format!("expected peer_list, got: {text}"));
    }

    let responder_id = value
        .get("machine_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let responder_version = value
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let peers: Vec<PeerInfo> = value
        .get("peers")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Stamp last_seen = now on all received peers (they were presumably just alive)
    let peers = peers
        .into_iter()
        .map(|mut p| {
            if p.last_seen == 0 {
                p.last_seen = now;
            }
            p
        })
        .collect();

    Ok((responder_id, responder_version, peers))
}
