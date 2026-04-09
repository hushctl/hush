//! Peer gossip — each daemon periodically dials all known peers, sends a
//! `peer_hello`, receives a `peer_list`, and merges the results. New peers
//! learned this way are persisted to state.json so the mesh is remembered
//! across restarts. Stale peers (no contact in 24h) are pruned.
//!
//! The gossip loop also handles the initial `--join` seed: those URLs are
//! already in `state.peers` by the time this task starts (inserted in main.rs),
//! so the first tick dials them automatically.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::RwLock;
use tokio::time::{interval, MissedTickBehavior};
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;
use tracing::{debug, info};

use crate::state::{DaemonState, PeerInfo};

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

pub fn spawn_gossip(state: Arc<RwLock<DaemonState>>, state_path: PathBuf) {
    tokio::spawn(async move {
        let mut ticker = interval(GOSSIP_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            run_gossip_round(Arc::clone(&state), &state_path).await;
        }
    });
    info!("spawned gossip task (interval={}s)", GOSSIP_INTERVAL.as_secs());
}

async fn run_gossip_round(state: Arc<RwLock<DaemonState>>, state_path: &PathBuf) {
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
    let mut contacted: Vec<String> = Vec::new();
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
            Ok((responder_id, received_peers)) => {
                // If the peer responded with our own machine_id it's a self-dial
                // (e.g. stale --join seed pointing at our own IP or localhost).
                if responder_id == machine_id {
                    debug!("gossip: {} ({}) resolved to ourselves — removing placeholder", peer.machine_id, peer.url);
                    self_placeholder_ids.push(peer.machine_id.clone());
                    continue;
                }
                contacted.push(peer.machine_id.clone());
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

    // Merge results + update last_seen + prune stale + remove self-placeholders
    {
        let mut s = state.write().await;
        for mid in &contacted {
            s.touch_peer(mid);
        }
        s.merge_peers(newly_learned);
        // Remove any entries we discovered were pointing at ourselves
        for mid in &self_placeholder_ids {
            s.peers.retain(|p| &p.machine_id != mid);
        }
        s.prune_stale(STALE_AFTER_SECS);
        s.save(state_path);
    }

    if !contacted.is_empty() {
        info!("gossip round complete: contacted [{}]", contacted.join(", "));
    }
}

/// Open a temporary WebSocket to `url`, send `peer_hello`, read the `peer_list`
/// response, close, and return `(responder_machine_id, received_peers)`.
async fn dial_peer(
    my_machine_id: &str,
    my_url: &str,
    my_peers: &[PeerInfo],
    peer_url: &str,
) -> Result<(String, Vec<PeerInfo>), String> {
    let hello = serde_json::json!({
        "type": "peer_hello",
        "machine_id": my_machine_id,
        "url": my_url,
        "peers": my_peers,
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

    Ok((responder_id, peers))
}
