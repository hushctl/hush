mod gossip;
mod hooks;
mod protocol;
mod pty;
mod state;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State as AxumState;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use clap::Parser;
use tokio::sync::{broadcast, RwLock};
use tracing::info;

use crate::protocol::ServerMessage;
use crate::pty::PtyManager;
use crate::state::{DaemonState, PeerInfo};

#[derive(Parser)]
#[command(name = "mcd", about = "Mission Control Daemon")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 9111)]
    port: u16,

    /// Bind address (use 0.0.0.0 to listen on all interfaces incl. Tailscale)
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Path to state file (default: ~/.mission-control/state.json)
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
}

#[derive(Clone)]
struct AppState {
    daemon_state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let state_path = args.state_file.unwrap_or_else(|| {
        let home = dirs::home_dir().expect("Could not determine home directory");
        home.join(".mission-control").join("state.json")
    });

    // Ensure the directory exists
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create ~/.mission-control/");
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
        });
    }

    daemon_state.save(&state_path);
    info!(
        "Loaded state: {} projects, machine_id={}",
        daemon_state.projects.len(),
        daemon_state.machine_id
    );

    let machine_id = daemon_state.machine_id.clone();
    let daemon_state = Arc::new(RwLock::new(daemon_state));

    // Global broadcast channel — capacity 256 events
    let (tx, _rx) = broadcast::channel::<ServerMessage>(256);

    // Hook listener Unix socket — mc-hook shim writes status events here
    let hook_socket = hooks::default_socket_path();
    let mc_hook_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mc-hook")))
        .unwrap_or_else(|| PathBuf::from("mc-hook"));

    let pty_manager = PtyManager::new(tx.clone(), machine_id, hook_socket.clone(), mc_hook_path);

    hooks::spawn_listener(
        hook_socket,
        Arc::clone(&daemon_state),
        state_path.clone(),
        tx.clone(),
    );

    // Gossip task — runs every 30s, dials known peers
    gossip::spawn_gossip(Arc::clone(&daemon_state), state_path.clone());

    let app_state = AppState {
        daemon_state,
        state_path,
        tx,
        pty_manager,
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(app_state);

    let addr = format!("{}:{}", args.bind, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    info!("Mission Control Daemon listening on ws://{addr}/ws");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");

    info!("Daemon shut down");
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
        )
    })
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
    info!("Shutting down...");
}
