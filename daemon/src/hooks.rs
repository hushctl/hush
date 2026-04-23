//! Claude Code hooks listener.
//!
//! Listens on a Unix socket; the `hush-hook` shim binary connects per hook
//! invocation and writes one JSON line. We parse the line, look up the
//! worktree, transition its status, persist, and broadcast.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use crate::protocol::ServerMessage;
use crate::pty::PtyManager;
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
    pty_manager: PtyManager,
) {
    tokio::spawn(async move {
        if let Err(e) = run(socket_path.clone(), state, state_path, tx, pty_manager).await {
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
    pty_manager: PtyManager,
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
        let pty_manager = pty_manager.clone();
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
            handle_line(&line, state, state_path, tx, pty_manager).await;
        });
    }
}

async fn handle_line(
    line: &str,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
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

        // Auto-dispatch the next queued task when the worktree becomes idle.
        if matches!(new_status, Some(WorktreeStatus::Idle)) {
            let next = {
                let mut s = state.write().await;
                if let Some(w) = s.find_worktree_mut(&parsed.worktree_id) {
                    if !w.queued_tasks.is_empty() {
                        Some(w.queued_tasks.remove(0))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(prompt) = next {
                {
                    let s = state.read().await;
                    s.save(&state_path);
                    let machine_id = s.machine_id.clone();
                    let queued = s.find_worktree(&parsed.worktree_id)
                        .map(|w| w.queued_tasks.clone())
                        .unwrap_or_default();
                    let _ = tx.send(ServerMessage::QueueUpdate {
                        machine_id,
                        worktree_id: parsed.worktree_id.clone(),
                        queued_tasks: queued,
                    });
                }
                // Give Claude Code a moment to return to the prompt before injecting input.
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let data = format!("{}\r", prompt);
                if let Err(e) = pty_manager.write(&parsed.worktree_id, data.as_bytes()).await {
                    warn!("failed to dispatch queued task: {e}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Build a minimal DaemonState with one project and one worktree.
    /// Returns `(state, tmp_dir, worktree_id)`.
    fn make_state() -> (Arc<RwLock<DaemonState>>, TempDir, String) {
        let tmp = TempDir::new().unwrap();
        let mut ds = DaemonState::default();
        ds.machine_id = "test-machine".to_string();
        let proj = ds.register_project(PathBuf::from("/tmp/proj"), "TestProject".to_string());
        let wt = ds
            .add_worktree(&proj.id, "main".to_string(), PathBuf::from("/tmp/proj"), "default".to_string())
            .unwrap();
        let wt_id = wt.id.clone();
        (Arc::new(RwLock::new(ds)), tmp, wt_id)
    }

    fn make_pty_manager(tx: broadcast::Sender<ServerMessage>) -> PtyManager {
        use std::path::PathBuf;
        PtyManager::new(
            tx,
            "test-machine".to_string(),
            PathBuf::from("/tmp/test-hooks.sock"),
            PathBuf::from("/usr/local/bin/hush-hook"),
        )
    }

    /// Process a hook line and return the resulting worktree status.
    async fn process(line: &str) -> (Arc<RwLock<DaemonState>>, TempDir, String) {
        let (state, tmp, wt_id) = make_state();
        let (tx, _rx) = broadcast::channel(16);
        let pty_manager = make_pty_manager(tx.clone());
        let hook_line = line.replace("{{WID}}", &wt_id);
        handle_line(
            &hook_line,
            Arc::clone(&state),
            tmp.path().join("state.json"),
            tx,
            pty_manager,
        )
        .await;
        (state, tmp, wt_id)
    }

    async fn get_status(state: &Arc<RwLock<DaemonState>>, wt_id: &str) -> WorktreeStatus {
        state.read().await.find_worktree(wt_id).unwrap().status.clone()
    }

    #[tokio::test]
    async fn session_start_transitions_to_running() {
        let (state, _tmp, wt_id) = process(r#"{"event":"session_start","worktree_id":"{{WID}}"}"#).await;
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Running);
    }

    #[tokio::test]
    async fn user_prompt_transitions_to_running() {
        let (state, _tmp, wt_id) = process(
            r#"{"event":"user_prompt","worktree_id":"{{WID}}","payload":{"prompt":"do something"}}"#,
        )
        .await;
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Running);
    }

    #[tokio::test]
    async fn user_prompt_sets_last_task() {
        let (state, _tmp, wt_id) = process(
            r#"{"event":"user_prompt","worktree_id":"{{WID}}","payload":{"prompt":"write tests"}}"#,
        )
        .await;
        let last_task = state.read().await.find_worktree(&wt_id).unwrap().last_task.clone();
        assert_eq!(last_task, Some("write tests".to_string()));
    }

    #[tokio::test]
    async fn pre_tool_use_transitions_to_running() {
        let (state, _tmp, wt_id) =
            process(r#"{"event":"pre_tool_use","worktree_id":"{{WID}}"}"#).await;
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Running);
    }

    #[tokio::test]
    async fn notification_transitions_to_needs_you() {
        let (state, _tmp, wt_id) =
            process(r#"{"event":"notification","worktree_id":"{{WID}}"}"#).await;
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::NeedsYou);
    }

    #[tokio::test]
    async fn stop_transitions_to_idle() {
        let (state, tmp, wt_id) = make_state();
        // Put it in Running first so the transition back to Idle is observable
        {
            let mut s = state.write().await;
            s.find_worktree_mut(&wt_id).unwrap().status = WorktreeStatus::Running;
        }
        let (tx, _rx) = broadcast::channel(16);
        let pty_manager = make_pty_manager(tx.clone());
        handle_line(
            &format!(r#"{{"event":"stop","worktree_id":"{wt_id}"}}"#),
            Arc::clone(&state),
            tmp.path().join("state.json"),
            tx,
            pty_manager,
        )
        .await;
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Idle);
    }

    #[tokio::test]
    async fn session_end_transitions_to_idle() {
        let (state, tmp, wt_id) = make_state();
        {
            let mut s = state.write().await;
            s.find_worktree_mut(&wt_id).unwrap().status = WorktreeStatus::Running;
        }
        let (tx, _rx) = broadcast::channel(16);
        let pty_manager = make_pty_manager(tx.clone());
        handle_line(
            &format!(r#"{{"event":"session_end","worktree_id":"{wt_id}"}}"#),
            Arc::clone(&state),
            tmp.path().join("state.json"),
            tx,
            pty_manager,
        )
        .await;
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Idle);
    }

    #[tokio::test]
    async fn unknown_event_does_not_change_status() {
        let (state, _tmp, wt_id) =
            process(r#"{"event":"some_future_event","worktree_id":"{{WID}}"}"#).await;
        // Status stays Idle (initial value)
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Idle);
    }

    #[tokio::test]
    async fn malformed_json_does_not_panic() {
        let (state, _tmp, wt_id) = process("not valid json at all").await;
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Idle);
    }

    #[tokio::test]
    async fn unknown_worktree_does_not_panic() {
        let (state, tmp, wt_id) = make_state();
        let (tx, _rx) = broadcast::channel(16);
        let pty_manager = make_pty_manager(tx.clone());
        // wt_unknown doesn't exist — should log a warning and return without panic
        handle_line(
            r#"{"event":"session_start","worktree_id":"wt_unknown_xyz"}"#,
            Arc::clone(&state),
            tmp.path().join("state.json"),
            tx,
            pty_manager,
        )
        .await;
        // The real worktree is unchanged
        assert_eq!(get_status(&state, &wt_id).await, WorktreeStatus::Idle);
    }

    #[tokio::test]
    async fn status_change_broadcast_is_sent() {
        let (state, tmp, wt_id) = make_state();
        let (tx, mut rx) = broadcast::channel(16);
        let pty_manager = make_pty_manager(tx.clone());
        handle_line(
            &format!(r#"{{"event":"session_start","worktree_id":"{wt_id}"}}"#),
            Arc::clone(&state),
            tmp.path().join("state.json"),
            tx,
            pty_manager,
        )
        .await;
        // The first broadcast should be StatusChange(running)
        let msg = rx.try_recv().expect("expected a broadcast message");
        assert!(
            matches!(&msg, ServerMessage::StatusChange { status, .. } if status == "running"),
            "expected StatusChange(running), got {msg:?}"
        );
    }
}
