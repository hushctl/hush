//! Claude Code hooks listener.
//!
//! Listens on a Unix socket; the `hush-hook` shim binary connects per hook
//! invocation and writes one JSON line. We parse the line, look up the
//! worktree, transition its status, persist, and broadcast.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use crate::protocol::ServerMessage;
use crate::state::{DaemonState, WorktreeStatus};

#[derive(Debug, Deserialize)]
struct HookLine {
    event: String,
    worktree_id: String,
    #[serde(default)]
    payload: Option<Value>,
}

/// Spawn the listener task. Removes any stale socket file, binds, and runs
/// the accept loop forever.
pub fn spawn_listener(
    socket_path: PathBuf,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
) {
    tokio::spawn(async move {
        if let Err(e) = run(socket_path.clone(), state, state_path, tx).await {
            warn!("hook listener exited: {e}");
        }
    });
    info!("spawned hook listener task");
}

async fn run(
    socket_path: PathBuf,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
) -> std::io::Result<()> {
    // Clean up any stale socket from a previous daemon run
    let _ = std::fs::remove_file(&socket_path);
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let listener = UnixListener::bind(&socket_path)?;
    info!("hook listener bound to {}", socket_path.display());

    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = Arc::clone(&state);
        let state_path = state_path.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => return, // empty connection, ignore
                Ok(_) => {}
                Err(e) => {
                    warn!("hook read error: {e}");
                    return;
                }
            }
            handle_line(&line, state, state_path, tx).await;
        });
    }
}

async fn handle_line(
    line: &str,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
) {
    let parsed: HookLine = match serde_json::from_str(line.trim()) {
        Ok(p) => p,
        Err(e) => {
            warn!("malformed hook line: {e} | {}", line.trim());
            return;
        }
    };

    debug!(
        "hook event={} worktree={}",
        parsed.event, parsed.worktree_id
    );

    // Decide the new status (and possibly extract last_task / session_id).
    let new_status: Option<WorktreeStatus> = match parsed.event.as_str() {
        "session_start" | "user_prompt" | "pre_tool_use" => Some(WorktreeStatus::Running),
        "notification" => Some(WorktreeStatus::NeedsYou),
        "stop" | "session_end" => Some(WorktreeStatus::Idle),
        other => {
            debug!("unknown hook event: {other}");
            None
        }
    };

    // Pull task text from user_prompt payload if present
    let last_task: Option<String> = if parsed.event == "user_prompt" {
        parsed
            .payload
            .as_ref()
            .and_then(|v| v.get("prompt"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };

    // Pull session_id if it's in the payload
    let session_id: Option<String> = parsed
        .payload
        .as_ref()
        .and_then(|v| v.get("session_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut status_changed = false;
    {
        let mut s = state.write().await;
        if let Some(w) = s.find_worktree_mut(&parsed.worktree_id) {
            if let Some(ref ns) = new_status {
                if w.status != *ns {
                    w.status = ns.clone();
                    status_changed = true;
                }
            }
            if let Some(t) = last_task {
                w.last_task = Some(t);
                status_changed = true;
            }
            if let Some(sid) = session_id {
                if w.session_id.as_deref() != Some(&sid) {
                    w.session_id = Some(sid);
                    status_changed = true;
                }
            }
        } else {
            warn!("hook for unknown worktree: {}", parsed.worktree_id);
            return;
        }
        if status_changed {
            s.save(&state_path);
        }
    }

    if status_changed {
        let s = state.read().await;
        let machine_id = s.machine_id.clone();
        let status_str = s
            .find_worktree(&parsed.worktree_id)
            .map(|w| w.status.as_str())
            .unwrap_or_else(|| "idle".to_string());
        let _ = tx.send(ServerMessage::StatusChange {
            machine_id: machine_id.clone(),
            worktree_id: parsed.worktree_id.clone(),
            status: status_str,
        });
        // Also broadcast a fresh worktree_list so any client also gets the
        // new last_task / session_id reflected in card UI.
        let worktrees = s.worktree_list();
        drop(s);
        let _ = tx.send(ServerMessage::WorktreeList {
            machine_id,
            worktrees,
        });
    }
}

/// Standard socket location: ~/.hush/hooks.sock
pub fn default_socket_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| Path::new("/tmp").to_path_buf());
    home.join(".hush").join("hooks.sock")
}
