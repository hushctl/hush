use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::git_watcher::{run_git_status, GitWatcher};
use crate::protocol::{ClientMessage, ServerMessage};
use crate::pty::PtyManager;
use crate::state::{DaemonState, PeerInfo};
use crate::transfer::{self, InboundTransfer, InboundTransfers};

pub async fn handle_socket(
    socket: WebSocket,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
    git_watcher: GitWatcher,
    inbound_transfers: InboundTransfers,
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

    // Per-connection active transfer ID — binary frames go to this transfer's file.
    // At most one in-flight inbound transfer per WS connection (each source
    // opens a dedicated WS for every transfer it initiates).
    let mut active_transfer_id: Option<String> = None;

    // Reader loop: receive ClientMessage from WebSocket, dispatch
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Binary(bytes)) => {
                // Raw tar bytes from source — append to the active transfer's temp file.
                if let Some(ref tid) = active_transfer_id {
                    let mut map = inbound_transfers.lock().await;
                    if let Some(t) = map.get_mut(tid.as_str()) {
                        t.write_bytes(&bytes);
                    } else {
                        warn!("Binary frame for unknown transfer {tid}");
                    }
                } else {
                    warn!("Binary frame with no active transfer — ignoring");
                }
                continue;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(Message::Text(t)) => {
                // Peek at the transfer_id before full dispatch so we can track
                // which transfer is active for binary-frame routing.
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    match v.get("type").and_then(|t| t.as_str()) {
                        Some("transfer_offer") => {
                            active_transfer_id = v
                                .get("transfer_id")
                                .and_then(|id| id.as_str())
                                .map(String::from);
                        }
                        Some("transfer_commit") | Some("transfer_abort") => {
                            // Clear after the message is handled (apply spawned).
                            // The source closes the WS after commit anyway.
                            active_transfer_id = None;
                        }
                        _ => {}
                    }
                }

                debug!("ws recv: {t}");

                let client_msg: ClientMessage = match serde_json::from_str(&t) {
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
                    git_watcher.clone(),
                    Arc::clone(&inbound_transfers),
                )
                .await;
            }
            Ok(_) => continue, // ping/pong
        }
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
    git_watcher: GitWatcher,
    inbound_transfers: InboundTransfers,
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
                    .map(|w| (w.working_dir.clone(), w.permission_mode.clone(), w.session_id.clone()))
            };
            let (working_dir, permission_mode, session_id) = match info {
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

            // Spawn pty if needed — use --continue (not --resume) so a missing
            // session file doesn't break reconnects or daemon restarts.
            if !pty_manager.exists(&worktree_id).await {
                let has_session = session_id.is_some();
                if let Err(e) = pty_manager
                    .spawn(
                        worktree_id.clone(),
                        &working_dir,
                        &permission_mode,
                        None, // --continue, not --resume
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
                    worktree_id: worktree_id.clone(),
                    data: encoded,
                });
            }

            // Start git status polling for this worktree
            git_watcher.start_watching(worktree_id, PathBuf::from(working_dir)).await;
        }

        ClientMessage::PtyDetach { worktree_id } => {
            // Stop git status polling when the pane detaches
            git_watcher.stop_watching(&worktree_id).await;
        }

        ClientMessage::PtyInput { worktree_id, data } => {
            // Auto-spawn if no pty is running (e.g. after daemon restart or pty crash).
            if !pty_manager.exists(&worktree_id).await {
                let info = {
                    let s = state.read().await;
                    s.find_worktree(&worktree_id)
                        .map(|w| (w.working_dir.clone(), w.permission_mode.clone(), w.session_id.clone()))
                };
                if let Some((working_dir, permission_mode, session_id)) = info {
                    let has_session = session_id.is_some();
                    let _ = pty_manager
                        .spawn(worktree_id.clone(), &working_dir, &permission_mode, None, has_session, 80, 24)
                        .await;
                }
            }
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

        ClientMessage::GitStatus { worktree_id } => {
            let working_dir = {
                let s = state.read().await;
                s.find_worktree(&worktree_id)
                    .map(|w| PathBuf::from(&w.working_dir))
            };
            let Some(working_dir) = working_dir else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Worktree {worktree_id} not found"),
                    worktree_id: Some(worktree_id),
                });
                return;
            };
            match run_git_status(&working_dir).await {
                Ok((staged, modified, untracked)) => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::GitStatus {
                        machine_id,
                        worktree_id,
                        staged,
                        modified,
                        untracked,
                    });
                }
                Err(e) => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!("git status failed: {e}"),
                        worktree_id: Some(worktree_id),
                    });
                }
            }
        }

        ClientMessage::ListFiles { worktree_id } => {
            let working_dir = {
                let s = state.read().await;
                s.find_worktree(&worktree_id)
                    .map(|w| PathBuf::from(&w.working_dir))
            };
            let Some(working_dir) = working_dir else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Worktree {worktree_id} not found"),
                    worktree_id: Some(worktree_id),
                });
                return;
            };
            let output = tokio::process::Command::new("git")
                .args(["ls-files", "--cached", "--others", "--exclude-standard"])
                .current_dir(&working_dir)
                .output()
                .await;
            match output {
                Ok(o) if o.status.success() => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let files: Vec<String> = stdout
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| l.to_string())
                        .collect();
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::FileList {
                        machine_id,
                        worktree_id,
                        files,
                    });
                }
                Ok(o) => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!(
                            "git ls-files failed: {}",
                            String::from_utf8_lossy(&o.stderr)
                        ),
                        worktree_id: Some(worktree_id),
                    });
                }
                Err(e) => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!("git ls-files failed: {e}"),
                        worktree_id: Some(worktree_id),
                    });
                }
            }
        }

        ClientMessage::ReadFile { worktree_id, path } => {
            let working_dir = {
                let s = state.read().await;
                s.find_worktree(&worktree_id)
                    .map(|w| PathBuf::from(&w.working_dir))
            };
            let Some(working_dir) = working_dir else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Worktree {worktree_id} not found"),
                    worktree_id: Some(worktree_id),
                });
                return;
            };

            // Resolve and validate path — prevent directory traversal
            let requested = working_dir.join(&path);
            let (canonical_file, canonical_base) = match (
                requested.canonicalize(),
                working_dir.canonicalize(),
            ) {
                (Ok(f), Ok(b)) => (f, b),
                _ => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!("File not found: {path}"),
                        worktree_id: Some(worktree_id),
                    });
                    return;
                }
            };
            if !canonical_file.starts_with(&canonical_base) {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: "Access denied: path outside worktree".to_string(),
                    worktree_id: Some(worktree_id),
                });
                return;
            }

            if canonical_file.is_dir() {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Cannot open: {path} is a directory"),
                    worktree_id: Some(worktree_id),
                });
                return;
            }

            const MAX_BYTES: usize = 256 * 1024;
            match tokio::fs::read(&canonical_file).await {
                Ok(bytes) => {
                    let truncated = bytes.len() > MAX_BYTES;
                    let slice = if truncated { &bytes[..MAX_BYTES] } else { &bytes[..] };
                    match String::from_utf8(slice.to_vec()) {
                        Ok(content) => {
                            let machine_id = state.read().await.machine_id.clone();
                            let _ = tx.send(ServerMessage::FileContent {
                                machine_id,
                                worktree_id,
                                path,
                                content,
                                truncated,
                            });
                        }
                        Err(_) => {
                            let machine_id = state.read().await.machine_id.clone();
                            let _ = tx.send(ServerMessage::Error {
                                machine_id,
                                message: format!("File is not valid UTF-8: {path}"),
                                worktree_id: Some(worktree_id),
                            });
                        }
                    }
                }
                Err(e) => {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: format!("Failed to read file: {e}"),
                        worktree_id: Some(worktree_id),
                    });
                }
            }
        }

        ClientMessage::ShellAttach { worktree_id, cols, rows } => {
            let working_dir = {
                let s = state.read().await;
                s.find_worktree(&worktree_id).map(|w| PathBuf::from(&w.working_dir))
            };
            let Some(working_dir) = working_dir else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Worktree {worktree_id} not found"),
                    worktree_id: Some(worktree_id),
                });
                return;
            };

            let shell_key = format!("shell:{worktree_id}");
            if !pty_manager.exists(&shell_key).await {
                if let Err(e) = pty_manager.spawn_shell(worktree_id.clone(), &working_dir, cols, rows).await {
                    let machine_id = state.read().await.machine_id.clone();
                    let _ = tx.send(ServerMessage::Error {
                        machine_id,
                        message: e,
                        worktree_id: Some(worktree_id),
                    });
                    return;
                }
            } else {
                let _ = pty_manager.resize(&shell_key, cols, rows).await;
            }

            if let Some(scrollback) = pty_manager.scrollback(&shell_key).await {
                let machine_id = state.read().await.machine_id.clone();
                let encoded = BASE64.encode(&scrollback);
                let _ = tx.send(ServerMessage::ShellScrollback {
                    machine_id,
                    worktree_id,
                    data: encoded,
                });
            }
        }

        ClientMessage::ShellInput { worktree_id, data } => {
            let shell_key = format!("shell:{worktree_id}");
            if let Err(e) = pty_manager.write(&shell_key, data.as_bytes()).await {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: e,
                    worktree_id: Some(worktree_id),
                });
            }
        }

        ClientMessage::ShellResize { worktree_id, cols, rows } => {
            let shell_key = format!("shell:{worktree_id}");
            if let Err(e) = pty_manager.resize(&shell_key, cols, rows).await {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: e,
                    worktree_id: Some(worktree_id),
                });
            }
        }

        ClientMessage::ShellKill { worktree_id } => {
            let shell_key = format!("shell:{worktree_id}");
            pty_manager.kill(&shell_key).await;
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

        // ── Worktree removal ─────────────────────────────────────────────────

        ClientMessage::RemoveWorktree { worktree_id } => {
            let wt_info = {
                let s = state.read().await;
                s.find_worktree(&worktree_id)
                    .map(|w| (w.working_dir.clone(), w.project_id.clone()))
            };
            let Some((working_dir, project_id)) = wt_info else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Worktree {worktree_id} not found"),
                    worktree_id: Some(worktree_id),
                });
                return;
            };

            // Kill pty
            pty_manager.kill(&worktree_id).await;

            // Remove record
            {
                let mut s = state.write().await;
                s.remove_worktree(&worktree_id);
                s.remove_project_if_empty(&project_id);
                s.save(&state_path);
            }

            // git worktree remove --force (only if linked worktree, not main)
            let git_file = working_dir.join(".git");
            if git_file.is_file() {
                if let Some(parent) = working_dir.parent() {
                    let _ = tokio::process::Command::new("git")
                        .args(["worktree", "remove", "--force"])
                        .arg(&working_dir)
                        .current_dir(parent)
                        .output()
                        .await;
                }
            }

            let (machine_id, worktrees) = {
                let s = state.read().await;
                (s.machine_id.clone(), s.worktree_list())
            };
            let _ = tx.send(ServerMessage::WorktreeList { machine_id, worktrees });
        }

        // ── Transfer: browser → source daemon ─────────────────────────────────

        ClientMessage::TransferWorktree { worktree_id, dest_machine_id } => {
            let peer_url = transfer::peer_url_for(&state, &dest_machine_id).await;
            let Some(peer_url) = peer_url else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Unknown peer machine '{dest_machine_id}' — not in gossip mesh"),
                    worktree_id: Some(worktree_id),
                });
                return;
            };

            let wt_info = {
                let s = state.read().await;
                s.find_worktree(&worktree_id).cloned().zip(
                    s.projects.iter().find(|p| {
                        p.worktrees.iter().any(|w| w.id == worktree_id)
                    }).map(|p| (p.name.clone(), p.path.clone()))
                )
            };
            let Some((worktree, (project_name, project_path))) = wt_info else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Worktree {worktree_id} not found"),
                    worktree_id: Some(worktree_id),
                });
                return;
            };

            let machine_id = state.read().await.machine_id.clone();
            info!("Initiating transfer of {worktree_id} to {dest_machine_id}");
            tokio::spawn(transfer::send_worktree(
                worktree,
                project_name,
                project_path,
                peer_url,
                dest_machine_id,
                machine_id,
                Arc::clone(&state),
                state_path,
                tx,
                pty_manager,
            ));
        }

        ClientMessage::TransferProject { project_id, dest_machine_id } => {
            let peer_url = transfer::peer_url_for(&state, &dest_machine_id).await;
            let Some(peer_url) = peer_url else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Unknown peer machine '{dest_machine_id}'"),
                    worktree_id: None,
                });
                return;
            };

            let proj_info = {
                let s = state.read().await;
                s.projects.iter()
                    .find(|p| p.id == project_id)
                    .map(|p| (p.name.clone(), p.path.clone(), p.worktrees.clone()))
            };
            let Some((project_name, project_path, worktrees)) = proj_info else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::Error {
                    machine_id,
                    message: format!("Project {project_id} not found"),
                    worktree_id: None,
                });
                return;
            };

            let machine_id = state.read().await.machine_id.clone();
            for wt in worktrees {
                info!("Initiating transfer of worktree {} (project {project_id}) to {dest_machine_id}", wt.id);
                tokio::spawn(transfer::send_worktree(
                    wt,
                    project_name.clone(),
                    project_path.clone(),
                    peer_url.clone(),
                    dest_machine_id.clone(),
                    machine_id.clone(),
                    Arc::clone(&state),
                    state_path.clone(),
                    tx.clone(),
                    pty_manager.clone(),
                ));
            }
        }

        // ── Transfer: destination daemon (source dials us) ────────────────────

        ClientMessage::TransferOffer {
            transfer_id,
            from_machine_id,
            project_name,
            project_path_hint,
            branch,
            permission_mode,
            session_id,
            last_task,
            has_history,
            total_bytes,
        } => {
            let machine_id = state.read().await.machine_id.clone();

            let hint = PathBuf::from(&project_path_hint);
            let dest_path = match transfer::resolve_dest_path(&hint, &branch) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(ServerMessage::TransferError {
                        machine_id,
                        transfer_id: transfer_id.clone(),
                        message: format!("Cannot resolve dest path: {e}"),
                    });
                    return;
                }
            };

            if dest_path.exists() {
                let _ = tx.send(ServerMessage::TransferError {
                    machine_id,
                    transfer_id: transfer_id.clone(),
                    message: format!("Destination path already exists: {}", dest_path.display()),
                });
                return;
            }

            let dest_path_str = dest_path.to_string_lossy().to_string();
            let xfer_dir = transfer::transfers_dir(&state_path);

            // Open temp files for streaming writes
            let wd_path = xfer_dir.join(format!("{transfer_id}.tar.gz"));
            let hist_path = xfer_dir.join(format!("{transfer_id}.history.tar"));

            let wd_file = match std::fs::OpenOptions::new()
                .write(true).create(true).truncate(true)
                .open(&wd_path)
            {
                Ok(f) => Some(f),
                Err(e) => {
                    let _ = tx.send(ServerMessage::TransferError {
                        machine_id,
                        transfer_id: transfer_id.clone(),
                        message: format!("Cannot open transfer temp file: {e}"),
                    });
                    return;
                }
            };

            {
                let mut map = inbound_transfers.lock().await;
                map.insert(transfer_id.clone(), InboundTransfer {
                    transfer_id: transfer_id.clone(),
                    dest_path,
                    project_name,
                    project_path_hint: hint,
                    branch,
                    permission_mode,
                    session_id,
                    last_task,
                    from_machine_id,
                    has_history,
                    total_bytes,
                    bytes_received: 0,
                    current_kind: "working_dir".to_string(),
                    working_dir_file: wd_file,
                    working_dir_path: wd_path,
                    history_file: None,
                    history_path: hist_path,
                });
            }

            let _ = tx.send(ServerMessage::TransferAck {
                machine_id,
                transfer_id,
                dest_path: dest_path_str,
            });
        }

        ClientMessage::TransferKindSwitch { transfer_id, kind } => {
            let mut map = inbound_transfers.lock().await;
            let Some(t) = map.get_mut(&transfer_id) else {
                warn!("TransferKindSwitch for unknown transfer {transfer_id}");
                return;
            };
            if kind == "history" && t.history_file.is_none() {
                // Open the history temp file on demand
                match std::fs::OpenOptions::new()
                    .write(true).create(true).truncate(true)
                    .open(&t.history_path)
                {
                    Ok(f) => { t.history_file = Some(f); }
                    Err(e) => { warn!("Transfer {transfer_id}: cannot open history temp file: {e}"); return; }
                }
            }
            t.current_kind = kind;
        }

        ClientMessage::TransferCommit { transfer_id } => {
            let t = {
                let mut map = inbound_transfers.lock().await;
                map.remove(&transfer_id)
            };
            let Some(t) = t else {
                let machine_id = state.read().await.machine_id.clone();
                let _ = tx.send(ServerMessage::TransferError {
                    machine_id,
                    transfer_id,
                    message: "No matching inbound transfer".to_string(),
                });
                return;
            };

            let tx2 = tx.clone();
            let state2 = Arc::clone(&state);
            let pty2 = pty_manager.clone();
            tokio::spawn(async move {
                let machine_id = state2.read().await.machine_id.clone();
                match transfer::apply_transfer(t, state2, state_path, tx2.clone(), pty2).await {
                    Ok(new_wt_id) => {
                        let _ = tx2.send(ServerMessage::TransferComplete {
                            machine_id,
                            transfer_id,
                            new_worktree_id: new_wt_id,
                        });
                    }
                    Err(e) => {
                        warn!("apply_transfer failed: {e}");
                        let _ = tx2.send(ServerMessage::TransferError {
                            machine_id,
                            transfer_id,
                            message: e,
                        });
                    }
                }
            });
        }

        ClientMessage::TransferAbort { transfer_id, reason } => {
            let removed = {
                let mut map = inbound_transfers.lock().await;
                map.remove(&transfer_id)
            };
            if removed.is_some() {
                info!("Transfer {transfer_id} aborted by source: {reason}");
            }
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
