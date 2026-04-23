mod claude_history;
mod git_watcher;
mod gossip;
mod hooks;
mod mdns;
mod join;
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
use axum::routing::{get, post};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use crate::tls::PeerCertPresent;
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
    /// Generate a short-lived join token for enrolling a new machine into the mesh.
    /// Run this on the CA-origin machine, then pass the token to `hush --join-token`
    /// on the machine you want to add.
    Invite,
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
    #[arg(long, default_value = "127.0.0.1")]
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

    /// Join token received from `hush invite` on an existing mesh member.
    /// When provided alongside --join, the daemon will POST to the peer's /join
    /// endpoint, receive a signed leaf cert + CA cert, write them to
    /// ~/.hush/tls/, and then start normally.
    #[arg(long)]
    join_token: Option<String>,

    /// Disable mDNS peer discovery on LAN.
    #[arg(long, default_value_t = false)]
    no_mdns: bool,
}

/// State for the /join endpoint (subset of full AppState).
#[derive(Clone)]
pub struct JoinHandlerState {
    pub hush_dir: PathBuf,
}

#[derive(Clone)]
struct AppState {
    daemon_state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    auth_token: String,

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
        Some(SubCommand::Invite) => {
            match join::generate_token(&hush_dir) {
                Ok(token) => {
                    println!("{token}");
                    println!();
                    println!("Token expires in 10 minutes. On the joining machine, run:");
                    println!("  hush --join wss://<this-machine>:9111/peer --join-token {token}");
                }
                Err(e) => {
                    eprintln!("Error generating invite token: {e}");
                    std::process::exit(1);
                }
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

    // If a join token was provided, POST to the first --join peer to receive
    // a signed leaf cert + CA cert before the daemon starts serving.
    if let Some(join_token) = &args.join_token {
        let peer_url = args.join.first().cloned().unwrap_or_default();
        if peer_url.is_empty() {
            eprintln!("Error: --join-token requires --join <peer-url>");
            std::process::exit(1);
        }
        info!("Performing mesh join via {peer_url}...");
        let tls_hush_dir = args
            .tls_dir
            .as_deref()
            .unwrap_or_else(|| state_path.parent().unwrap_or(std::path::Path::new(".")))
            .to_path_buf();
        if let Err(e) = join::perform_join(
            &peer_url,
            join_token,
            &daemon_state.machine_id,
            &tls_hush_dir,
        )
        .await
        {
            eprintln!("Error: mesh join failed: {e}");
            std::process::exit(1);
        }
    }

    // Load or generate auth token
    let auth_token = load_or_generate_auth_token(
        state_path.parent().unwrap_or(std::path::Path::new(".")),
    );

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
        pty_manager.clone(),
    );

    // Gossip task — runs every 30s, dials known peers
    gossip::spawn_gossip(
        Arc::clone(&daemon_state),
        state_path.clone(),
        tx.clone(),
        args.auto_upgrade,
    );

    // mDNS peer discovery — advertise on LAN and browse for peers.
    let _mdns_handle = if !args.no_mdns {
        Some(mdns::spawn_mdns(
            Arc::clone(&daemon_state),
            machine_id.clone(),
            args.advertise_url.clone(),
            args.port,
        ))
    } else {
        None
    };

    // Memory pressure monitor — polls system memory every 15s, alerts on transitions
    memory_monitor::spawn(machine_id.clone(), tx.clone());

    // Clean up any stale transfer temp files from a previous crash
    transfer::clean_transfers_dir(&state_path);

    let app_state = AppState {
        daemon_state,
        state_path: state_path.clone(),
        auth_token,
        tx,
        pty_manager,
        git_watcher,
        inbound_transfers: new_inbound_transfers(),
        inbound_upgrades: new_inbound_upgrades(),
    };

    let join_state = JoinHandlerState {
        hush_dir: hush_dir.clone(),
    };

    // The /join endpoint uses a separate state (JoinHandlerState) from the main
    // AppState to avoid circular dependencies. Merge via nested routers.
    let join_router: Router = Router::new()
        .route("/join", post(join::join_handler))
        .with_state(join_state);

    // Allow any origin — the daemon serves a local-only trusted interface and
    // the dev server (localhost:5173) needs to reach it cross-origin.
    let cors = tower_http::cors::CorsLayer::permissive();

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/peer", get(peer_ws_handler))
        .route("/health", get(|| async { "ok" }))
        .route("/config", get(config_handler))
        .route("/config/local", get(config_local_handler))
        .route("/config/peers", get(config_peers_handler))
        .with_state(app_state)
        .merge(join_router)
        .layer(cors);

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
    // Skipped when HUSH_SKIP_CA_TRUST=1 (used in CI where keychain prompts block).
    let skip_trust = std::env::var("HUSH_SKIP_CA_TRUST").as_deref() == Ok("1");
    if !skip_trust && !trust::is_trusted(tls_hush_dir) {
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

    // Build rustls ServerConfig with optional client cert verification.
    // When a mesh CA is present, the server accepts but does not require client
    // certs on /ws (browsers). The /peer handler enforces cert presence for
    // daemon-to-daemon mTLS authentication.
    let (ca_cert_pem_opt, _) = tls::read_ca_pems_from_state(&state_path);
    let server_config = tls::build_server_config(
        &tls_material.cert_pem,
        &tls_material.key_pem,
        ca_cert_pem_opt.as_deref(),
    )
    .expect("Failed to build TLS server config");
    let rustls_config = RustlsConfig::from_config(std::sync::Arc::new(server_config));

    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        info!("Shutting down...");
        shutdown_handle.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
    });

    // Use a custom acceptor that injects PeerCertPresent into every request's
    // extensions after the TLS handshake. The /peer handler uses this to enforce
    // that daemon peers presented a valid CA-signed TLS client certificate.
    let acceptor = tls::MtlsAcceptor::new(rustls_config);
    axum_server::Server::bind(addr.parse().expect("Invalid bind address"))
        .acceptor(acceptor)
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
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

fn load_or_generate_auth_token(hush_dir: &std::path::Path) -> String {
    let token_path = hush_dir.join("auth_token");
    if let Ok(token) = std::fs::read_to_string(&token_path) {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return token;
        }
    }
    // Generate new token
    use ring::rand::SecureRandom;
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .expect("Failed to generate random bytes");
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&token_path, &token).expect("Failed to write auth_token");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            &token_path,
            std::fs::Permissions::from_mode(0o600),
        );
    }
    info!("Generated new auth token → {}", token_path.display());
    token
}

async fn config_handler(
    AxumState(app_state): AxumState<AppState>,
) -> impl IntoResponse {
    let machine_id = app_state.daemon_state.read().await.machine_id.clone();
    axum::Json(serde_json::json!({
        "machine_id": machine_id,
    }))
}

/// Same as /config but also returns the auth token — only served to localhost clients.
async fn config_local_handler(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    AxumState(app_state): AxumState<AppState>,
) -> axum::response::Response {
    let ip = addr.ip();
    if !ip.is_loopback() {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }
    let machine_id = app_state.daemon_state.read().await.machine_id.clone();
    axum::Json(serde_json::json!({
        "token": app_state.auth_token,
        "machine_id": machine_id,
    }))
    .into_response()
}

/// Returns auth tokens for all known peers — loopback only. The browser uses
/// these to open authenticated WebSocket connections to remote daemons.
async fn config_peers_handler(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    AxumState(app_state): AxumState<AppState>,
) -> axum::response::Response {
    if !addr.ip().is_loopback() {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }
    let tokens = app_state.daemon_state.read().await.peer_tokens_snapshot();
    axum::Json(tokens).into_response()
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    AxumState(app_state): AxumState<AppState>,
) -> axum::response::Response {
    // Validate auth token
    let provided = params.get("token").map(|s| s.as_str()).unwrap_or("");
    if provided != app_state.auth_token {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    info!("New WebSocket connection (authenticated)");
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
    .into_response()
}

/// Daemon-to-daemon WebSocket endpoint. Requires a valid CA-signed TLS client
/// certificate — enforced via the [`tls::MtlsAcceptor`] which injects
/// [`PeerCertPresent`] into every request after the TLS handshake. Connections
/// without a client cert (e.g. browsers, unauthenticated scanners) are rejected
/// with 403 Forbidden before the WebSocket upgrade occurs.
async fn peer_ws_handler(
    ws: WebSocketUpgrade,
    axum::extract::Extension(peer_cert): axum::extract::Extension<PeerCertPresent>,
    AxumState(app_state): AxumState<AppState>,
) -> axum::response::Response {
    if !peer_cert.0 {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Peer mTLS certificate required. Use `hush invite` + `hush --join-token` to enroll this machine.",
        )
            .into_response();
    }
    info!("New peer WebSocket connection (mTLS authenticated)");
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
    .into_response()
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut sigterm = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::terminate(),
    )
    .expect("Failed to install SIGTERM handler");

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm.recv() => {},
    }
}
