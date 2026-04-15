mod claude_history;
mod git_watcher;
mod gossip;
mod hooks;
mod memory_monitor;
mod peer_upgrade;
mod protocol;
mod pty;
mod state;
mod tls;
mod transfer;
mod trust;
mod upgrade;
mod ws;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Original argv, saved at startup so the upgrade path can exec the new binary
/// with the same arguments after replacing itself.
pub static DAEMON_ARGS: OnceLock<Vec<String>> = OnceLock::new();

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State as AxumState;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use clap::Parser;
use tokio::sync::{broadcast, RwLock};
use tracing::info;

use crate::git_watcher::GitWatcher;
use crate::peer_upgrade::{new_inbound_upgrades, InboundUpgrades};
use crate::protocol::ServerMessage;
use crate::pty::PtyManager;
use crate::state::{DaemonState, PeerInfo};
use crate::transfer::{new_inbound_transfers, InboundTransfers};

#[derive(clap::Subcommand)]
enum SubCommand {
    /// Upgrade hush to the latest GitHub release
    Upgrade,
    /// Manage the local CA used to issue trusted TLS certificates
    Trust {
        #[command(subcommand)]
        action: Option<TrustAction>,
    },
}

#[derive(clap::Subcommand)]
enum TrustAction {
    /// Install the Hush local CA into the OS trust store (default)
    Install,
    /// Print CA paths and an scp command for sharing to other machines
    Export,
    /// Remove the Hush local CA from the OS trust store
    Uninstall,
}

#[derive(Parser)]
#[command(name = "hush", about = "Hush Daemon", version)]
struct Args {
    #[command(subcommand)]
    command: Option<SubCommand>,

    /// Port to listen on
    #[arg(short, long, default_value_t = 9111)]
    port: u16,

    /// Bind address (use 0.0.0.0 to listen on all interfaces incl. Tailscale)
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Path to state file (default: ~/.hush/state.json)
    #[arg(long)]
    state_file: Option<PathBuf>,

    /// Human-readable name for this machine (defaults to hostname).
    /// Used as the stable machine_id in the gossip mesh.
    #[arg(long)]
    machine_name: Option<String>,

    /// WebSocket URL at which peers can reach this daemon
    /// (e.g. ws://laptop.tailnet.ts.net:9111/ws).
    /// Required for other daemons to discover and connect back to this one.
    #[arg(long, default_value = "")]
    advertise_url: String,

    /// Seed peer URL(s) to contact on startup (e.g. ws://other-machine:9111/ws).
    /// Can be specified multiple times. Merged into state.peers once, then persisted.
    #[arg(long)]
    join: Vec<String>,

    /// Directory for TLS CA and leaf cert (default: same dir as state file).
    /// Useful when running two daemons on the same machine — point both to
    /// the same --tls-dir so they share the already-trusted CA.
    #[arg(long)]
    tls_dir: Option<PathBuf>,

    /// Automatically push a newer binary to peers running an older version.
    /// Each peer is upgraded at most once per gossip cycle; launchd restarts
    /// the peer's daemon automatically after the binary is replaced.
    #[arg(long, default_value_t = false)]
    auto_upgrade: bool,
}

#[derive(Clone)]
struct AppState {
    daemon_state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,

    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
    git_watcher: GitWatcher,
    inbound_transfers: InboundTransfers,
    inbound_upgrades: InboundUpgrades,
}

#[tokio::main]
async fn main() {
    // axum-server's TLS (rustls 0.23) requires an explicit crypto provider when
    // multiple backends are in the dependency graph.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Save argv before parsing so the upgrade path can re-exec with the same args.
    let _ = DAEMON_ARGS.set(std::env::args().collect());

    let args = Args::parse();

    // Resolve hush_dir early — needed by trust subcommand before full state load.
    let hush_dir = args
        .state_file
        .as_deref()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| {
            dirs::home_dir()
                .expect("Could not determine home directory")
                .join(".hush")
        });

    match &args.command {
        Some(SubCommand::Upgrade) => {
            upgrade::run().await;
            return;
        }
        Some(SubCommand::Trust { action }) => {
            let action = action.as_ref().unwrap_or(&TrustAction::Install);
            let result = match action {
                TrustAction::Install => trust::install(&hush_dir),
                TrustAction::Export => {
                    trust::export(&hush_dir);
                    Ok(())
                }
                TrustAction::Uninstall => trust::uninstall(&hush_dir),
            };
            if let Err(e) = result {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
            return;
        }
        None => {}
    }

    let state_path = args.state_file.unwrap_or_else(|| {
        let home = dirs::home_dir().expect("Could not determine home directory");
        home.join(".hush").join("state.json")
    });

    // Ensure the directory exists
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create ~/.hush/");
    }

    let mut daemon_state = DaemonState::load(&state_path);

    // Assign machine_id if not already persisted
    if daemon_state.machine_id.is_empty() {
        daemon_state.machine_id = args.machine_name.clone().unwrap_or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        });
    } else if let Some(name) = &args.machine_name {
        // Allow override via CLI even if persisted
        daemon_state.machine_id = name.clone();
    }

    // Update advertise_url if provided
    if !args.advertise_url.is_empty() {
        daemon_state.advertise_url = args.advertise_url.clone();
    }

    // Merge --join seed peers
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for url in &args.join {
        // We don't know the machine_id yet — use the URL as a placeholder key.
        // Once the gossip round dials it, the real machine_id will be learned.
        daemon_state.merge_peer(PeerInfo {
            machine_id: url.clone(), // overwritten once peer responds
            url: url.clone(),
            last_seen: now_secs,
            version: String::new(),
        });
    }

    // Migrate ws:// → wss:// in persisted advertise_url, peer URLs, and placeholder machine_ids
    if daemon_state.advertise_url.starts_with("ws://") {
        let new_url = daemon_state.advertise_url.replacen("ws://", "wss://", 1);
        info!("Migrated advertise_url: ws:// → wss:// ({})", new_url);
        daemon_state.advertise_url = new_url;
    }
    for peer in daemon_state.peers.iter_mut() {
        if peer.url.starts_with("ws://") {
            peer.url = peer.url.replacen("ws://", "wss://", 1);
            info!("Migrated peer URL → wss:// ({})", peer.url);
        }
        // Placeholder machine_ids (the --join URL, before the real ID is learned via gossip)
        if peer.machine_id.starts_with("ws://") {
            peer.machine_id = peer.machine_id.replacen("ws://", "wss://", 1);
        }
    }
    // Deduplicate peers by URL — repeated --join invocations can create duplicate placeholder entries
    {
        let mut seen = std::collections::HashSet::new();
        daemon_state.peers.retain(|p| seen.insert(p.url.clone()));
    }

    daemon_state.save(&state_path);
    info!(
        "Loaded state: {} projects, machine_id={}",
        daemon_state.projects.len(),
        daemon_state.machine_id
    );

    let machine_id = daemon_state.machine_id.clone();
    let advertise_url = daemon_state.advertise_url.clone();
    let daemon_state = Arc::new(RwLock::new(daemon_state));

    // Global broadcast channel — capacity 256 events
    let (tx, _rx) = broadcast::channel::<ServerMessage>(256);

    // Hook listener Unix socket — hush-hook shim writes status events here
    // Derive hook socket from state dir so two daemons on the same machine
    // don't share the same socket.
    let hook_socket = state_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/tmp"))
        .join("hooks.sock");
    let hush_hook_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("hush-hook")))
        .unwrap_or_else(|| PathBuf::from("hush-hook"));

    let pty_manager = PtyManager::new(
        tx.clone(),
        machine_id.clone(),
        hook_socket.clone(),
        hush_hook_path,
    );
    let git_watcher = GitWatcher::new(tx.clone(), machine_id.clone());

    hooks::spawn_listener(
        hook_socket,
        Arc::clone(&daemon_state),
        state_path.clone(),
        tx.clone(),
    );

    // Gossip task — runs every 30s, dials known peers
    gossip::spawn_gossip(
        Arc::clone(&daemon_state),
        state_path.clone(),
        tx.clone(),
        args.auto_upgrade,
    );

    // Memory pressure monitor — polls system memory every 15s, alerts on transitions
    memory_monitor::spawn(machine_id.clone(), tx.clone());

    // Clean up any stale transfer temp files from a previous crash
    transfer::clean_transfers_dir(&state_path);

    let app_state = AppState {
        daemon_state,
        state_path: state_path.clone(),
        tx,
        pty_manager,
        git_watcher,
        inbound_transfers: new_inbound_transfers(),
        inbound_upgrades: new_inbound_upgrades(),
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(app_state);

    // Serve the built UI as a fallback — if a UI dir is found, any request
    // that isn't /ws or /health gets the SPA (with index.html fallback for
    // client-side routing).
    let app = if let Some(ui_dir) = resolve_ui_dir() {
        info!("Serving UI from {}", ui_dir.display());
        let serve_dir = tower_http::services::ServeDir::new(&ui_dir).fallback(
            tower_http::services::ServeFile::new(ui_dir.join("index.html")),
        );
        app.fallback_service(serve_dir)
    } else {
        info!("No UI directory found — run `make build-ui` or set $HUSH_UI_DIR");
        app
    };

    let addr = format!("{}:{}", args.bind, args.port);

    // Load or generate self-signed TLS cert.
    // --tls-dir overrides the default (state file dir) so two daemons on the
    // same machine can share the already-trusted CA.
    let tls_hush_dir = args
        .tls_dir
        .as_deref()
        .unwrap_or_else(|| state_path.parent().unwrap_or(std::path::Path::new(".")));
    let tls_material = tls::load_or_generate(tls_hush_dir, &machine_id)
        .expect("Failed to load/generate TLS certificate");

    // Auto-trust CA on first boot so browsers just work.
    if !trust::is_trusted(tls_hush_dir) {
        let ca_cert_path = tls_hush_dir.join("tls").join("ca.crt");
        if ca_cert_path.exists() {
            info!("First run — installing CA into OS trust store (may prompt for password)...");
            match trust::install_ca(&ca_cert_path) {
                Ok(()) => {
                    trust::write_trusted_marker(tls_hush_dir);
                    info!("✓ CA trusted — browsers will accept Hush certificates");
                }
                Err(e) => {
                    info!("CA trust install failed: {e} — run `hush trust` manually if needed");
                }
            }
        }
    }

    // Derive a useful host for the browser hint — prefer the advertise URL's host over 0.0.0.0
    let hint_host = if !advertise_url.is_empty() {
        advertise_url
            .trim_start_matches("wss://")
            .trim_start_matches("ws://")
            .trim_end_matches("/ws")
            .to_string()
    } else {
        format!("localhost:{}", args.port)
    };
    info!("Hush Daemon listening on wss://{addr}/ws");
    info!("  Cert fingerprint (SHA-256): {}", tls_material.fingerprint);
    info!("  Open https://{hint_host} in your browser");

    let rustls_config = RustlsConfig::from_pem(tls_material.cert_pem, tls_material.key_pem)
        .await
        .expect("Failed to build TLS config");

    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        info!("Shutting down...");
        shutdown_handle.graceful_shutdown(None);
    });

    axum_server::bind_rustls(addr.parse().expect("Invalid bind address"), rustls_config)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .expect("Server error");

    info!("Daemon shut down");
}

/// Find the built UI directory. Checks (in order):
/// 1. `$HUSH_UI_DIR` environment variable
/// 2. `{binary_dir}/../ui/dist/` (dev layout — binary in daemon/target/debug/)
/// 3. `{binary_dir}/ui/` (installed layout — binary + ui side by side)
/// 4. `~/.hush/ui/` (make install target)
fn resolve_ui_dir() -> Option<std::path::PathBuf> {
    // Explicit override
    if let Ok(dir) = std::env::var("HUSH_UI_DIR") {
        let p = std::path::PathBuf::from(dir);
        if p.join("index.html").exists() {
            return Some(p);
        }
    }

    // Relative to binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            // Dev: daemon/target/debug/hush → repo/ui/dist/
            let dev = bin_dir
                .join("..")
                .join("..")
                .join("..")
                .join("ui")
                .join("dist");
            if dev.join("index.html").exists() {
                return Some(dev.canonicalize().unwrap_or(dev));
            }

            // Installed: ~/.local/bin/hush + ~/.local/bin/ui/
            let installed = bin_dir.join("ui");
            if installed.join("index.html").exists() {
                return Some(installed);
            }
        }
    }

    // ~/.hush/ui/
    if let Some(home) = dirs::home_dir() {
        let hush_ui = home.join(".hush").join("ui");
        if hush_ui.join("index.html").exists() {
            return Some(hush_ui);
        }
    }

    None
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    AxumState(app_state): AxumState<AppState>,
) -> impl IntoResponse {
    info!("New WebSocket connection");
    ws.on_upgrade(move |socket| {
        ws::handle_socket(
            socket,
            app_state.daemon_state,
            app_state.state_path,
            app_state.tx,
            app_state.pty_manager,
            app_state.git_watcher,
            app_state.inbound_transfers,
            app_state.inbound_upgrades,
        )
    })
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
}
