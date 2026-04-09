//! Long-lived pty per worktree.
//!
//! Each worktree gets one `claude` process running inside a pty owned by the
//! daemon. Bytes from the pty stream out to all attached WebSocket clients via
//! the existing broadcast channel as `pty_data` messages. Stdin (keystrokes
//! from the browser) flows back via `pty_input`.
//!
//! Ptys outlive browser connections — close the laptop lid, come back, the
//! session is still alive (tmux model). Ptys do NOT survive a daemon restart;
//! recovery happens by spawning fresh `claude --continue` processes.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info, warn};

use crate::claude_history;
use crate::protocol::ServerMessage;

/// Maximum scrollback retained per pty (bytes). On reattach, the daemon
/// replays this much history so a new browser sees the recent screen state.
const SCROLLBACK_CAP: usize = 256 * 1024; // 256 KB

pub struct PtySession {
    /// Writer side of the pty master — keystrokes go here.
    writer: Box<dyn Write + Send>,
    /// Master pty handle, kept around for resize.
    master: Box<dyn MasterPty + Send>,
    /// In-memory scrollback for late-arriving / reattaching clients.
    scrollback: Vec<u8>,
}

impl PtySession {
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize failed: {e}"))
    }

    pub fn scrollback(&self) -> Vec<u8> {
        self.scrollback.clone()
    }

    fn append_scrollback(&mut self, bytes: &[u8]) {
        self.scrollback.extend_from_slice(bytes);
        if self.scrollback.len() > SCROLLBACK_CAP {
            // Drop the oldest half. Cheap, infrequent, keeps the buffer
            // bounded without copying every write.
            let drop = self.scrollback.len() - SCROLLBACK_CAP;
            self.scrollback.drain(..drop);
        }
    }
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<PtySession>>>>>,
    tx: broadcast::Sender<ServerMessage>,
    /// Stable machine identity for this daemon — stamped onto every broadcast.
    machine_id: Arc<String>,
    /// Absolute path to the daemon's hook Unix socket. Injected as
    /// HUSH_HOOK_SOCKET env var into every spawned claude process so the
    /// `hush-hook` shim knows where to write.
    hook_socket: PathBuf,
    /// Absolute path to the `hush-hook` shim binary. Written into each
    /// worktree's settings.local.json so Claude Code can invoke it.
    hush_hook_path: PathBuf,
}

impl PtyManager {
    pub fn new(
        tx: broadcast::Sender<ServerMessage>,
        machine_id: String,
        hook_socket: PathBuf,
        hush_hook_path: PathBuf,
    ) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            tx,
            machine_id: Arc::new(machine_id),
            hook_socket,
            hush_hook_path,
        }
    }

    /// Returns whether a pty for the given worktree currently exists.
    pub async fn exists(&self, worktree_id: &str) -> bool {
        self.sessions.lock().await.contains_key(worktree_id)
    }

    /// Get the current scrollback for a worktree, if a pty exists.
    pub async fn scrollback(&self, worktree_id: &str) -> Option<Vec<u8>> {
        let sessions = self.sessions.lock().await;
        let s = sessions.get(worktree_id)?;
        let s = s.lock().await;
        Some(s.scrollback())
    }

    /// Write bytes to a worktree's pty stdin.
    pub async fn write(&self, worktree_id: &str, data: &[u8]) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        let s = sessions
            .get(worktree_id)
            .ok_or_else(|| format!("no pty for worktree {worktree_id}"))?;
        let mut s = s.lock().await;
        s.write(data).map_err(|e| format!("pty write failed: {e}"))
    }

    /// Resize a worktree's pty.
    pub async fn resize(&self, worktree_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        let s = sessions
            .get(worktree_id)
            .ok_or_else(|| format!("no pty for worktree {worktree_id}"))?;
        let mut s = s.lock().await;
        s.resize(cols, rows)
    }

    /// Kill and remove a worktree's pty (if any).
    pub async fn kill(&self, worktree_id: &str) {
        let mut sessions = self.sessions.lock().await;
        if sessions.remove(worktree_id).is_some() {
            info!("killed pty for {worktree_id}");
        }
        // Dropping the PtySession drops the master, which closes the slave
        // and causes the child process to receive SIGHUP.
    }

    /// Spawn a plain shell (bash/zsh/$SHELL) pty for a worktree.
    /// The session is stored under key `shell:{worktree_id}`.
    /// Output is broadcast as `ShellData`, scrollback as `ShellScrollback`.
    pub async fn spawn_shell(
        &self,
        worktree_id: String,
        working_dir: &Path,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let shell_key = format!("shell:{worktree_id}");
        {
            let sessions = self.sessions.lock().await;
            if sessions.contains_key(&shell_key) {
                debug!("shell pty for {worktree_id} already exists, skipping spawn");
                return Ok(());
            }
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("openpty failed: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(working_dir);
        for (key, value) in std::env::vars() {
            cmd.env(key, value);
        }

        let _child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn shell failed: {e}"))?;
        drop(pair.slave);

        let writer = pair.master.take_writer().map_err(|e| format!("take_writer failed: {e}"))?;
        let reader = pair.master.try_clone_reader().map_err(|e| format!("clone_reader failed: {e}"))?;

        let session = Arc::new(Mutex::new(PtySession {
            writer,
            master: pair.master,
            scrollback: Vec::new(),
        }));

        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(shell_key.clone(), Arc::clone(&session));
        }

        let (byte_tx, mut byte_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let shell_key_thread = shell_key.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => { debug!("shell pty reader EOF for {shell_key_thread}"); break; }
                    Ok(n) => { if byte_tx.send(buf[..n].to_vec()).is_err() { break; } }
                    Err(e) => { warn!("shell pty read error for {shell_key_thread}: {e}"); break; }
                }
            }
        });

        let session_for_task = Arc::clone(&session);
        let tx = self.tx.clone();
        let sessions_for_cleanup = Arc::clone(&self.sessions);
        let mid = Arc::clone(&self.machine_id);
        let wt_id = worktree_id.clone();
        let sk = shell_key.clone();
        tokio::spawn(async move {
            while let Some(chunk) = byte_rx.recv().await {
                {
                    let mut s = session_for_task.lock().await;
                    s.append_scrollback(&chunk);
                }
                let encoded = BASE64.encode(&chunk);
                let _ = tx.send(crate::protocol::ServerMessage::ShellData {
                    machine_id: (*mid).clone(),
                    worktree_id: wt_id.clone(),
                    data: encoded,
                });
            }
            info!("shell pty stream ended for {sk}");
            sessions_for_cleanup.lock().await.remove(&sk);
            let _ = tx.send(crate::protocol::ServerMessage::ShellExit {
                machine_id: (*mid).clone(),
                worktree_id: wt_id,
                code: None,
            });
        });

        info!("spawned shell pty for worktree {worktree_id} ({cols}x{rows})");
        Ok(())
    }

    /// Spawn a fresh `claude` pty for the given worktree. If one already
    /// exists, this is a no-op (returns Ok).
    ///
    /// `session_id` controls how the session is resumed:
    /// - `Some(id)` → `claude --resume <id>` (exact session by ID)
    /// - `None` with `has_session=true` → `claude --continue` (resume by cwd)
    /// - `None` with `has_session=false` → fresh session
    pub async fn spawn(
        &self,
        worktree_id: String,
        working_dir: &Path,
        permission_mode: &str,
        session_id: Option<&str>,
        has_session: bool,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        {
            let sessions = self.sessions.lock().await;
            if sessions.contains_key(&worktree_id) {
                debug!("pty for {worktree_id} already exists, skipping spawn");
                return Ok(());
            }
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty failed: {e}"))?;

        // Ensure .claude/settings.local.json in the worktree registers the
        // hush-hook shim for the lifecycle events we care about. Idempotent —
        // overwrites unconditionally with the canonical content.
        if let Err(e) = write_hook_settings(working_dir, &self.hush_hook_path) {
            warn!("failed to write .claude/settings.local.json: {e}");
            // Non-fatal — pty still spawns, just no status events.
        }

        let mut cmd = CommandBuilder::new("claude");
        if let Some(id) = session_id {
            cmd.arg("--resume");
            cmd.arg(id);
        } else if has_session {
            // Only pass --continue if Claude actually has history for this cwd.
            // Without this check, a missing/wrong slug causes an immediate exit
            // ("No conversation found to continue") leaving the user with a dead pty.
            let has_history = claude_history::history_dir_for(working_dir)
                .map_or(false, |d| d.exists());
            if has_history {
                cmd.arg("--continue");
            }
        }
        // "dangerously-skip-permissions" maps to the dedicated flag; all other
        // values are passed as --permission-mode <value>.
        if permission_mode == "dangerously-skip-permissions" {
            cmd.arg("--dangerously-skip-permissions");
        } else {
            cmd.arg("--permission-mode");
            cmd.arg(permission_mode);
        }
        cmd.cwd(working_dir);
        // Pass through env so claude finds its config / PATH
        for (key, value) in std::env::vars() {
            cmd.env(key, value);
        }
        // Inject hook env vars — hush-hook reads these to know where to send
        // events and which worktree they belong to.
        cmd.env("HUSH_WORKTREE_ID", &worktree_id);
        cmd.env("HUSH_HOOK_SOCKET", self.hook_socket.to_string_lossy().to_string());

        let _child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn claude failed: {e}"))?;
        // Drop the slave side so the daemon doesn't keep its fd open;
        // child still has its own copy.
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("take_writer failed: {e}"))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("clone_reader failed: {e}"))?;

        let session = Arc::new(Mutex::new(PtySession {
            writer,
            master: pair.master,
            scrollback: Vec::new(),
        }));

        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(worktree_id.clone(), Arc::clone(&session));
        }

        // Spawn the blocking reader on a dedicated OS thread. portable-pty's
        // reader is a sync std::io::Read, so we cannot use it directly from
        // tokio. The thread reads bytes and shovels them into a tokio mpsc.
        let (byte_tx, mut byte_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let worktree_id_thread = worktree_id.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!("pty reader EOF for {worktree_id_thread}");
                        break;
                    }
                    Ok(n) => {
                        if byte_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("pty read error for {worktree_id_thread}: {e}");
                        break;
                    }
                }
            }
        });

        // Tokio task: receive byte chunks, append to scrollback, broadcast.
        let session_for_task = Arc::clone(&session);
        let tx = self.tx.clone();
        let sessions_for_cleanup = Arc::clone(&self.sessions);
        let wid = worktree_id.clone();
        let mid = Arc::clone(&self.machine_id);
        tokio::spawn(async move {
            while let Some(chunk) = byte_rx.recv().await {
                {
                    let mut s = session_for_task.lock().await;
                    s.append_scrollback(&chunk);
                }
                let encoded = BASE64.encode(&chunk);
                let _ = tx.send(ServerMessage::PtyData {
                    machine_id: (*mid).clone(),
                    worktree_id: wid.clone(),
                    data: encoded,
                });
            }
            // Reader thread ended → pty is dead. Remove from session map.
            info!("pty stream ended for {wid}");
            sessions_for_cleanup.lock().await.remove(&wid);
            let _ = tx.send(ServerMessage::PtyExit {
                machine_id: (*mid).clone(),
                worktree_id: wid,
                code: None,
            });
        });

        info!("spawned pty for worktree {worktree_id} ({cols}x{rows})");
        Ok(())
    }
}

/// Write `<working_dir>/.claude/settings.local.json` with the hush-hook
/// registration for the lifecycle events we care about.
///
/// This file is conventionally gitignored by Claude Code projects (see
/// CLAUDE.md / Claude Code docs). The hush-hook shim is env-var-gated, so it's
/// also safe if the file leaks elsewhere — non-daemon claude invocations
/// just don't have HUSH_WORKTREE_ID set and the shim is a no-op.
fn write_hook_settings(working_dir: &Path, hush_hook_path: &Path) -> std::io::Result<()> {
    let claude_dir = working_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir)?;
    let settings_path = claude_dir.join("settings.local.json");
    let hook_cmd = hush_hook_path.to_string_lossy();

    // Build the JSON literally so we don't pull in serde for one helper.
    let body = format!(
        r#"{{
  "hooks": {{
    "SessionStart":     [{{"hooks": [{{"type": "command", "command": "{hook} session_start"}}]}}],
    "UserPromptSubmit": [{{"hooks": [{{"type": "command", "command": "{hook} user_prompt"}}]}}],
    "PreToolUse":       [{{"hooks": [{{"type": "command", "command": "{hook} pre_tool_use"}}]}}],
    "Notification":     [{{"hooks": [{{"type": "command", "command": "{hook} notification"}}]}}],
    "Stop":             [{{"hooks": [{{"type": "command", "command": "{hook} stop"}}]}}],
    "SessionEnd":       [{{"hooks": [{{"type": "command", "command": "{hook} session_end"}}]}}]
  }}
}}
"#,
        hook = hook_cmd
    );
    std::fs::write(&settings_path, body)?;
    Ok(())
}
