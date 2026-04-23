//! mDNS service advertisement and peer discovery.
//!
//! Advertises this daemon as `_hush._tcp.local.` on the LAN and browses for
//! peer daemons. Discovered peers are merged into `DaemonState.peers` so the
//! gossip loop can dial them automatically.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::state::{DaemonState, PeerInfo};

const SERVICE_TYPE: &str = "_hush._tcp.local.";

/// Returns the first non-loopback IPv4 address found on the system, if any.
fn local_ipv4() -> Option<std::net::Ipv4Addr> {
    if_addrs::get_if_addrs().ok()?.into_iter().find_map(|iface| {
        if iface.is_loopback() {
            return None;
        }
        match iface.addr.ip() {
            std::net::IpAddr::V4(v4) => Some(v4),
            _ => None,
        }
    })
}

/// Start mDNS advertisement and discovery.
///
/// Returns the `ServiceDaemon` handle — drop it to unregister on shutdown.
pub fn spawn_mdns(
    state: Arc<RwLock<DaemonState>>,
    machine_id: String,
    advertise_url: String,
    port: u16,
) -> ServiceDaemon {
    let daemon = ServiceDaemon::new().expect("failed to create mDNS service daemon");

    // ── Advertise ─────────────────────────────────────────────────────────────
    // Build the URL we'll put in the TXT record.
    let our_url = if !advertise_url.is_empty() {
        advertise_url.clone()
    } else if let Some(ip) = local_ipv4() {
        format!("wss://{}:{}/ws", ip, port)
    } else {
        String::new()
    };

    if !our_url.is_empty() {
        // Sanitise the instance name: mDNS instance names must not contain dots.
        let instance_name = machine_id.replace('.', "-");
        let host = format!("{}.local.", instance_name);

        let mut txt: HashMap<String, String> = HashMap::new();
        txt.insert("machine_id".to_string(), machine_id.clone());
        txt.insert("url".to_string(), our_url.clone());

        // Use enable_addr_auto() so mdns-sd fills in addresses from local
        // interfaces rather than requiring us to specify an IP.
        match ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name, // instance name (no dots allowed)
            &host,          // host name for A records
            "",             // IP — supply empty str and use addr_auto below
            port,
            txt,
        ) {
            Ok(info) => {
                let info = info.enable_addr_auto();
                if let Err(e) = daemon.register(info) {
                    warn!("mDNS register failed: {e}");
                } else {
                    info!("mDNS: advertising {} at {}", machine_id, our_url);
                }
            }
            Err(e) => warn!("mDNS ServiceInfo construction failed: {e}"),
        }
    } else {
        warn!("mDNS: no advertise URL and no LAN IP found — skipping advertisement");
    }

    // ── Browse ────────────────────────────────────────────────────────────────
    let browse_receiver = match daemon.browse(SERVICE_TYPE) {
        Ok(r) => r,
        Err(e) => {
            warn!("mDNS browse failed: {e}");
            return daemon;
        }
    };

    let state_clone = Arc::clone(&state);
    let my_machine_id = machine_id.clone();

    // mdns-sd is synchronous / channel-based; wrap in a tokio blocking task.
    tokio::task::spawn_blocking(move || {
        info!("mDNS: browsing for peers on LAN");
        while let Ok(event) = browse_receiver.recv() {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    let peer_machine_id =
                        match info.get_property_val_str("machine_id") {
                            Some(v) => v.to_string(),
                            None => continue,
                        };
                    let peer_url = match info.get_property_val_str("url") {
                        Some(v) => v.to_string(),
                        None => continue,
                    };

                    if peer_machine_id == my_machine_id {
                        continue; // skip ourselves
                    }

                    let last_seen = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    let peer = PeerInfo {
                        machine_id: peer_machine_id.clone(),
                        url: peer_url.clone(),
                        last_seen,
                        version: String::new(),
                    };

                    // We are inside a spawn_blocking task which always has a
                    // tokio runtime handle available.
                    let handle = tokio::runtime::Handle::current();
                    handle.block_on(async {
                        let mut s = state_clone.write().await;
                        s.merge_peer(peer);
                    });
                    info!("mDNS: discovered peer {} at {}", peer_machine_id, peer_url);
                }
                ServiceEvent::SearchStarted(_)
                | ServiceEvent::ServiceFound(_, _)
                | ServiceEvent::ServiceRemoved(_, _)
                | ServiceEvent::SearchStopped(_) => {
                    // Don't remove — let gossip stale-pruning handle it.
                }
            }
        }
    });

    daemon
}
