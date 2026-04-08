use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::protocol::{ClientMessage, ServerMessage};
use crate::pty::PtyManager;
use crate::state::{DaemonState, PeerInfo};

pub async fn handle_socket(
    socket: WebSocket,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
) {
    let mut rx = tx.subscribe();
    let (mut sink, mut stream) = socket.split();

    // Spawn writer task: forward broadcast events to this WebSocket client
    let writer = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let json = match serde_json::to_string(&msg) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Failed to serialize server message: {e}");
                            continue;
                        }
                    };
                    if sink.send(Message::Text(json.into())).await.is_err() {
                        break; // Client disconnected
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket client lagged, skipped {n} messages");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Reader loop: receive ClientMessage from WebSocket, dispatch
    while let Some(msg) = stream.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => continue, // ignore binary/ping/pong
        };

        debug!("ws recv: {text}");

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to parse client message: {e}");
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Invalid message: {e}"),
                    worktree_id: None,
                });
                continue;
            }
        };

        handle_client_message(
            client_msg,
            Arc::clone(&state),
            state_path.clone(),
            tx.clone(),
            pty_manager.clone(),
        )
        .await;
    }

    info!("WebSocket client disconnected");
    writer.abort();
}

async fn handle_client_message(
    msg: ClientMessage,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
) {
    match msg {
        ClientMessage::RegisterProject { path, name } => {
            let path_buf = PathBuf::from(&path);
            if !path_buf.is_dir() {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::PathNotFound { machine_id, path, name });
                return;
            }
            let info = {
                let mut s = state.write().await;
                let info = s.register_project(path_buf, name);
                s.save(&state_path);
                info
            };
            info!("Registered project: {} ({})", info.name, info.id);
            let (machine_id, projects) = {
                let s = state.read().await;
                (s.machine_id.clone(), s.project_list())
            };
            let _ = tx.send(ServerMessage::ProjectList { machine_id, projects });
        }

        ClientMessage::CreateAndRegisterProject { path, name } => {
            let path_buf = PathBuf::from(&path);
            if let Err(e) = std::fs::create_dir_all(&path_buf) {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Failed to create directory: {e}"),
                    worktree_id: None,
                });
                return;
            }
            let git_out = tokio::process::Command::new("git")
                .args(["init"])
                .current_dir(&path_buf)
                .output()
                .await;
            if let Err(e) = git_out {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Failed to run git init: {e}"),
                    worktree_id: None,
                });
                return;
            }
            let info = {
                let mut s = state.write().await;
                let info = s.register_project(path_buf, name);
                s.save(&state_path);
                info
            };
            info!("Created and registered project: {} ({})", info.name, info.id);
            let (machine_id, projects) = {
                let s = state.read().await;
                (s.machine_id.clone(), s.project_list())
            };
            let _ = tx.send(ServerMessage::ProjectList { machine_id, projects });
        }

        ClientMessage::CreateWorktree {
            project_id,
            branch,
            permission_mode,
        } => {
            // Look up project path
            let project_path = {
                let s = state.read().await;
                s.projects
                    .iter()
                    .find(|p| p.id == project_id)
                    .map(|p| p.path.clone())
            };

            let project_path = match project_path {
                Some(p) => p,
                None => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!("Project {project_id} not found"),
                        worktree_id: None,
                    });
                    return;
                }
            };

            // Resolve the working directory for this branch
            match resolve_worktree_dir(&project_path, &branch).await {
                Ok(working_dir) => {
                    let result = {
                        let mut s = state.write().await;
                        let result =
                            s.add_worktree(&project_id, branch, working_dir, permission_mode);
                        if result.is_ok() {
                            s.save(&state_path);
                        }
                        result
                    };
                    match result {
                        Ok(info) => {
                            info!(
                                "Created worktree: {} on branch {} at {}",
                                info.id, info.branch, info.working_dir
                            );
                            let (machine_id, worktrees) = {
                                let s = state.read().await;
                                (s.machine_id.clone(), s.worktree_list())
                            };
                            let _ = tx.send(ServerMessage::WorktreeList { machine_id, worktrees });
                        }
                        Err(e) => {
                            let machine_id = state.read().await.machine_id.clone();
                            let _ = tx.send(ServerMessage::Error {
                                machine_id,
                                message: e,
                                worktree_id: None,
                            });
                        }
                    }
                }
                Err(e) => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!("Failed to resolve worktree directory: {e}"),
                        worktree_id: None,
                    });
                }
            }
        }

        ClientMessage::PtyAttach {
            worktree_id,
            cols,
            rows,
        } => {
            // Look up the worktree's working dir + permission mode
            let info = {
                let s = state.read().await;
                s.find_worktree(&worktree_id)
                    .map(|w| (w.working_dir.clone(), w.permission_mode.clone(), w.session_id.is_some()))
            };
            let (working_dir, permission_mode, has_session) = match info {
                Some(t) => t,
                None => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!("Worktree {worktree_id} not found"),
                        worktree_id: Some(worktree_id),
                    });
                    return;
                }
            };

            // Spawn pty if needed
            if !pty_manager.exists(&worktree_id).await {
                if let Err(e) = pty_manager
                    .spawn(
                        worktree_id.clone(),
                        &working_dir,
                        &permission_mode,
                        has_session,
                        cols,
                        rows,
                    )
                    .await
                {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: e,
                        worktree_id: Some(worktree_id),
                    });
                    return;
                }
            } else {
                // Already running — just resize to the new client's geometry
                let _ = pty_manager.resize(&worktree_id, cols, rows).await;
            }

            // Replay scrollback
            if let Some(scrollback) = pty_manager.scrollback(&worktree_id).await {
                let machine_id = state.read().await.machine_id.clone();
                let encoded = BASE64.encode(&scrollback);
                let _ = tx.send(ServerMessage::PtyScrollback {
                    machine_id,
                    worktree_id,
                    data: encoded,
                });
            }
        }

        ClientMessage::PtyDetach { worktree_id: _ } => {
            // No-op for v1 — pty keeps running, broadcast keeps flowing.
        }

        ClientMessage::PtyInput { worktree_id, data } => {
            if let Err(e) = pty_manager.write(&worktree_id, data.as_bytes()).await {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: e,
                    worktree_id: Some(worktree_id),
                });
            }
        }

        ClientMessage::PtyResize {
            worktree_id,
            cols,
            rows,
        } => {
            if let Err(e) = pty_manager.resize(&worktree_id, cols, rows).await {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: e,
                    worktree_id: Some(worktree_id),
                });
            }
        }

        ClientMessage::PtyKill { worktree_id } => {
            pty_manager.kill(&worktree_id).await;
        }

        ClientMessage::ListProjects => {
            let (machine_id, projects) = {
                let s = state.read().await;
                (s.machine_id.clone(), s.project_list())
            };
            let _ = tx.send(ServerMessage::ProjectList { machine_id, projects });
        }

        ClientMessage::ListWorktrees => {
            let (machine_id, worktrees) = {
                let s = state.read().await;
                (s.machine_id.clone(), s.worktree_list())
            };
            let _ = tx.send(ServerMessage::WorktreeList { machine_id, worktrees });
        }

        ClientMessage::ListPeers => {
            let (machine_id, peers) = {
                let s = state.read().await;
                (s.machine_id.clone(), s.known_peers())
            };
            let _ = tx.send(ServerMessage::PeerList { machine_id, peers });
        }

        ClientMessage::PeerHello {
            machine_id: sender_id,
            url: sender_url,
            peers: sender_peers,
        } => {
            // Merge sender + their known peers into our state
            {
                let mut s = state.write().await;
                // Add the sender itself as a peer
                s.merge_peer(PeerInfo {
                    machine_id: sender_id.clone(),
                    url: sender_url,
                    last_seen: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                });
                s.merge_peers(sender_peers);
                s.save(&state_path);
            }
            // Reply with our peer list
            let (machine_id, peers) = {
                let s = state.read().await;
                (s.machine_id.clone(), s.known_peers())
            };
            let _ = tx.send(ServerMessage::PeerList { machine_id, peers });
        }
    }
}

/// Determine the filesystem directory for a given branch within a project.
async fn resolve_worktree_dir(project_path: &Path, branch: &str) -> Result<PathBuf, String> {
    // Get current HEAD branch — may fail on empty repos (no commits yet).
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {e}"))?;

    // If git failed or returned "HEAD" (empty/unborn repo), use the project path.
    let current_branch = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        String::new()
    };

    if branch == current_branch || current_branch == "HEAD" || current_branch.is_empty() {
        return Ok(project_path.to_path_buf());
    }

    // Create a git worktree for non-current branches
    let project_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    let worktree_base = project_path
        .parent()
        .ok_or("Project path has no parent")?
        .join(format!("{}-worktrees", project_name));

    std::fs::create_dir_all(&worktree_base)
        .map_err(|e| format!("Failed to create worktrees dir: {e}"))?;

    let worktree_path = worktree_base.join(branch);

    // If it already exists (prior run), just return it
    if worktree_path.exists() {
        return Ok(worktree_path);
    }

    let output = tokio::process::Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg(&worktree_path)
        .arg(branch)
        .current_dir(project_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git worktree add: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(worktree_path)
}
