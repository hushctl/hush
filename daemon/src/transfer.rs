//! Worktree transfer between daemons.
//!
//! # Wire model (v2 — binary frames + file streaming)
//!
//! Control messages travel as JSON text frames. File bytes travel as raw binary
//! WS frames (no base64, no JSON wrapper) written directly to temp files on the
//! destination. This eliminates the ~33 % base64 overhead and the multi-GB
//! in-memory buffer from v1.
//!
//! ## Message order (source → destination)
//!
//! ```text
//! text:  transfer_offer     { transfer_id, ... }
//! text:  transfer_ack       { transfer_id, dest_path }          ← destination replies
//! binary: <raw tar.gz bytes of working_dir>  (N frames, 256 KB each)
//! text:  transfer_kind_switch  { transfer_id, kind: "history" }  (if has_history)
//! binary: <raw tar bytes of history>          (M frames)          (if has_history)
//! text:  transfer_commit    { transfer_id }
//! text:  transfer_complete  { transfer_id, new_worktree_id }    ← destination replies
//! ```
//!
//! During the destination's apply phase (unpack → history install → pty spawn),
//! it broadcasts `transfer_progress` heartbeats every 10 s so the source's idle
//! watchdog stays alive even for very large worktrees.

use std::collections::HashMap;
use std::io::{Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::io;

use flate2::write::GzEncoder;
use flate2::Compression;
use futures_util::{SinkExt, StreamExt};
use tar::Builder as TarBuilder;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;
use tracing::{info, warn};

use crate::claude_history;
use crate::protocol::ServerMessage;
use crate::pty::PtyManager;
use crate::state::{DaemonState, Worktree};

const BINARY_CHUNK_SIZE: usize = 256 * 1024; // 256 KB per binary WS frame
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const ACK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the source waits without *any* message before giving up.
/// Heartbeats from the destination reset this every 10 s during apply.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

// ─── Temp directory ───────────────────────────────────────────────────────────

/// Returns `<state_dir>/transfers/` — temp home for in-flight tar files.
pub fn transfers_dir(state_path: &Path) -> PathBuf {
    state_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("transfers")
}

// ─── Destination-side per-transfer state ─────────────────────────────────────

pub struct InboundTransfer {
    pub transfer_id: String,
    pub dest_path: PathBuf,
    pub project_name: String,
    pub project_path_hint: PathBuf,
    pub branch: String,
    pub permission_mode: String,
    pub session_id: Option<String>,
    pub last_task: Option<String>,
    pub from_machine_id: String,
    pub has_history: bool,
    pub total_bytes: u64,
    pub bytes_received: u64,
    /// Which stream is currently being received: "working_dir" or "history"
    pub current_kind: String,

    /// Open write handle for the working_dir tar.gz temp file.
    pub working_dir_file: Option<std::fs::File>,
    pub working_dir_path: PathBuf,
    /// Open write handle for the history tar temp file.
    pub history_file: Option<std::fs::File>,
    pub history_path: PathBuf,
}

impl InboundTransfer {
    /// Append raw bytes to the currently active temp file.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let file = match self.current_kind.as_str() {
            "working_dir" => &mut self.working_dir_file,
            "history" => &mut self.history_file,
            _ => return,
        };
        if let Some(ref mut f) = file {
            if let Err(e) = f.write_all(bytes) {
                warn!("Transfer {}: write_bytes failed: {e}", self.transfer_id);
            }
            self.bytes_received += bytes.len() as u64;
        }
    }

    /// Flush + close both file handles before apply.
    pub fn close_files(&mut self) {
        drop(self.working_dir_file.take());
        drop(self.history_file.take());
    }
}

/// Shared map of in-progress inbound transfers on the destination daemon.
pub type InboundTransfers = Arc<Mutex<HashMap<String, InboundTransfer>>>;

pub fn new_inbound_transfers() -> InboundTransfers {
    Arc::new(Mutex::new(HashMap::new()))
}

// ─── TLS connector ───────────────────────────────────────────────────────────

fn make_tls_connector() -> Connector {
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("Failed to build TLS connector");
    Connector::NativeTls(tls.into())
}

/// Generate a unique transfer ID.
pub fn new_transfer_id(machine_id: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("tx-{machine_id}-{ts}")
}

// ─── Source side ─────────────────────────────────────────────────────────────

/// Orchestrate sending a worktree to a destination daemon.
/// Spawned as a tokio task from ws.rs.
pub async fn send_worktree(
    worktree: Worktree,
    project_name: String,
    project_path: PathBuf,
    peer_url: String,
    dest_machine_id: String,
    machine_id: String,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
) {
    let transfer_id = new_transfer_id(&machine_id);
    info!("Transfer {transfer_id}: starting — {} → {peer_url}", worktree.id);

    let source_wt_id = worktree.id.clone();
    let branch = worktree.branch.clone();
    let proj_name = project_name.clone();
    let dest_mid = dest_machine_id.clone();

    let progress = |phase: &str, bytes_sent: u64, total_bytes: u64| {
        let _ = tx.send(ServerMessage::TransferProgress {
            machine_id: machine_id.clone(),
            transfer_id: transfer_id.clone(),
            phase: phase.to_string(),
            bytes_sent,
            total_bytes,
            source_worktree_id: source_wt_id.clone(),
            project_name: proj_name.clone(),
            branch: branch.clone(),
            dest_machine_id: dest_mid.clone(),
        });
    };

    let fail = |msg: String| {
        warn!("Transfer {transfer_id}: failed — {msg}");
        let _ = tx.send(ServerMessage::TransferError {
            machine_id: machine_id.clone(),
            transfer_id: transfer_id.clone(),
            message: msg,
        });
    };

    // 1. Kill the source pty so Claude flushes its state
    progress("killing_pty", 0, 0);
    pty_manager.kill(&worktree.id).await;

    // 2. Collect history files (small, archive to a temp file before dialing)
    let history_files: Vec<PathBuf> = worktree.session_id.as_deref()
        .map(|id| claude_history::session_files_to_transfer(id))
        .unwrap_or_default();

    let xfer_dir = transfers_dir(&state_path);
    let hist_path = xfer_dir.join(format!("{transfer_id}.history.tar"));
    let (hist_size, has_history) = if !history_files.is_empty() {
        progress("archiving_history", 0, 0);
        match build_history_tar(&history_files, &hist_path) {
            Ok(sz) => (sz, true),
            Err(e) => {
                warn!("Transfer {transfer_id}: failed to tar history ({e}), continuing without it");
                (0, false)
            }
        }
    } else {
        (0, false)
    };

    // 3. Connect to destination (working_dir size is unknown until we stream it)
    progress("dialing", 0, hist_size);
    let connector = make_tls_connector();
    let connect_result = tokio::time::timeout(
        DIAL_TIMEOUT,
        connect_async_tls_with_config(&peer_url, None, false, Some(connector)),
    )
    .await;

    let (mut ws, _) = match connect_result {
        Ok(Ok(conn)) => conn,
        Ok(Err(e)) => {
            fail(format!("WS connect failed: {e}"));
            respawn_local(&pty_manager, &worktree, &tx, &machine_id).await;
            let _ = std::fs::remove_file(&hist_path);
            return;
        }
        Err(_) => {
            fail("WS connect timeout".to_string());
            respawn_local(&pty_manager, &worktree, &tx, &machine_id).await;
            let _ = std::fs::remove_file(&hist_path);
            return;
        }
    };

    // 4. Send TransferOffer (total_bytes = 0 = unknown; we stream working_dir live)
    progress("offering", 0, 0);
    let offer = serde_json::json!({
        "type": "transfer_offer",
        "transfer_id": &transfer_id,
        "from_machine_id": &machine_id,
        "project_name": &project_name,
        "project_path_hint": project_path.to_string_lossy(),
        "branch": &worktree.branch,
        "permission_mode": &worktree.permission_mode,
        "session_id": worktree.session_id,
        "last_task": worktree.last_task,
        "has_history": has_history,
        "total_bytes": 0u64,
    });
    if ws.send(Message::Text(offer.to_string().into())).await.is_err() {
        fail("Failed to send TransferOffer".to_string());
        respawn_local(&pty_manager, &worktree, &tx, &machine_id).await;
        let _ = std::fs::remove_file(&hist_path);
        return;
    }

    // 5. Wait for TransferAck
    progress("awaiting_ack", 0, 0);
    let dest_path = match wait_for_ack(&mut ws, &transfer_id).await {
        Ok(p) => p,
        Err(e) => {
            fail(format!("No TransferAck: {e}"));
            respawn_local(&pty_manager, &worktree, &tx, &machine_id).await;
            let _ = std::fs::remove_file(&hist_path);
            return;
        }
    };
    info!("Transfer {transfer_id}: ack'd, dest_path={dest_path}");

    // 6. Stream working_dir tar.gz directly into binary WS frames (no temp file)
    progress("streaming", 0, 0);
    let mut bytes_sent = 0u64;
    let working_dir = worktree.working_dir.clone();
    if let Err(e) = stream_working_dir_tar(
        &mut ws, &working_dir, &mut bytes_sent,
        &tx, &machine_id, &transfer_id, &worktree.id, &project_name, &worktree.branch, &dest_machine_id,
    ).await {
        fail(format!("Failed to stream working_dir: {e}"));
        respawn_local(&pty_manager, &worktree, &tx, &machine_id).await;
        let _ = std::fs::remove_file(&hist_path);
        return;
    }

    // 7. Stream history (already archived to disk; usually tiny)
    if has_history {
        progress("streaming_history", bytes_sent, bytes_sent + hist_size);
        let switch = serde_json::json!({
            "type": "transfer_kind_switch",
            "transfer_id": &transfer_id,
            "kind": "history",
        });
        if ws.send(Message::Text(switch.to_string().into())).await.is_err() {
            warn!("Transfer {transfer_id}: failed to send KindSwitch, continuing without history");
        } else {
            let total_with_hist = bytes_sent + hist_size;
            if let Err(e) = send_file_as_binary(
                &mut ws, &hist_path, &mut bytes_sent, total_with_hist,
                &tx, &machine_id, &transfer_id, &worktree.id, &project_name, &worktree.branch, &dest_machine_id,
            ).await {
                warn!("Transfer {transfer_id}: history stream failed ({e}), continuing");
            }
        }
    }
    let _ = std::fs::remove_file(&hist_path);

    // 8. Send TransferCommit
    progress("awaiting_commit", bytes_sent, bytes_sent);
    let commit = serde_json::json!({ "type": "transfer_commit", "transfer_id": &transfer_id });
    if ws.send(Message::Text(commit.to_string().into())).await.is_err() {
        fail("Failed to send TransferCommit".to_string());
        return;
    }

    // 9. Wait for TransferComplete — idle watchdog, no hard total timeout
    let new_wt_id = match wait_for_complete_idle(&mut ws, &transfer_id).await {
        Ok(id) => id,
        Err(e) => {
            fail(format!("TransferComplete not received: {e}"));
            return;
        }
    };
    let _ = ws.close(None).await;

    info!("Transfer {transfer_id}: complete, new_wt_id={new_wt_id}");

    // 11. Local cleanup: remove worktree record + git worktree remove
    let project_id = worktree.project_id.clone();
    {
        let mut s = state.write().await;
        s.remove_worktree(&worktree.id);
        s.remove_project_if_empty(&project_id);
        s.save(&state_path);
    }

    let wd = worktree.working_dir.clone();
    let git_file = wd.join(".git");
    if git_file.is_file() {
        // Linked worktree — safe to remove via git
        let _ = tokio::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&wd)
            .current_dir(&project_path)
            .output()
            .await;
    }

    // Broadcast updated list + completion
    let (mid, worktrees) = {
        let s = state.read().await;
        (s.machine_id.clone(), s.worktree_list())
    };
    let _ = tx.send(ServerMessage::WorktreeList { machine_id: mid.clone(), worktrees });
    let _ = tx.send(ServerMessage::TransferComplete {
        machine_id: mid,
        transfer_id: transfer_id.clone(),
        new_worktree_id: new_wt_id,
    });

    progress("complete", bytes_sent, bytes_sent);
}

// ─── Source helpers ───────────────────────────────────────────────────────────

/// A `Write` impl that chunks bytes into a tokio mpsc channel.
/// Each chunk is exactly `BINARY_CHUNK_SIZE` bytes except the last.
struct ChannelWriter {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    buf: Vec<u8>,
}

impl IoWrite for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        while self.buf.len() >= BINARY_CHUNK_SIZE {
            let chunk: Vec<u8> = self.buf.drain(..BINARY_CHUNK_SIZE).collect();
            self.tx.blocking_send(chunk)
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "ws receiver closed"))?;
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

impl Drop for ChannelWriter {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            let chunk = std::mem::take(&mut self.buf);
            // Best-effort flush of the last partial chunk; channel may already be gone.
            let _ = self.tx.blocking_send(chunk);
        }
    }
}

/// Tar + gzip the working directory and stream it directly to the WebSocket
/// as binary frames, without writing a temp file on disk.
/// Returns the number of compressed bytes sent.
#[allow(clippy::too_many_arguments)]
async fn stream_working_dir_tar(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    working_dir: &Path,
    bytes_sent: &mut u64,
    tx: &broadcast::Sender<ServerMessage>,
    machine_id: &str,
    transfer_id: &str,
    source_worktree_id: &str,
    project_name: &str,
    branch: &str,
    dest_machine_id: &str,
) -> Result<(), String> {
    // Channel capacity = 8 × 256 KB = 2 MB in-flight buffer
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);

    // Spawn a background thread to build the tar; it never touches the async runtime.
    let wd = working_dir.to_path_buf();
    let tar_thread = std::thread::spawn(move || -> Result<(), String> {
        let writer = ChannelWriter { tx: chunk_tx, buf: Vec::new() };
        let gz = GzEncoder::new(writer, Compression::fast());
        let mut tar = TarBuilder::new(gz);
        tar.follow_symlinks(false);
        append_dir_recursive(&mut tar, &wd, &wd)?;
        tar.into_inner()
            .map_err(|e| format!("tar finish: {e}"))?
            .finish()
            .map_err(|e| format!("gz finish: {e}"))?;
        Ok(())
    });

    // Drain chunks as they arrive and forward to the WebSocket.
    while let Some(chunk) = chunk_rx.recv().await {
        *bytes_sent += chunk.len() as u64;
        ws.send(Message::Binary(chunk.into()))
            .await
            .map_err(|e| format!("send binary frame: {e}"))?;

        // Broadcast progress roughly every 1 MB
        if *bytes_sent % (1024 * 1024) < BINARY_CHUNK_SIZE as u64 {
            let _ = tx.send(ServerMessage::TransferProgress {
                machine_id: machine_id.to_string(),
                transfer_id: transfer_id.to_string(),
                phase: "streaming".to_string(),
                bytes_sent: *bytes_sent,
                total_bytes: 0,  // unknown — show counter without denominator
                source_worktree_id: source_worktree_id.to_string(),
                project_name: project_name.to_string(),
                branch: branch.to_string(),
                dest_machine_id: dest_machine_id.to_string(),
            });
        }
    }

    // Collect the tar thread result (channel is now closed, thread has exited).
    tar_thread
        .join()
        .map_err(|_| "tar thread panicked".to_string())?
}


fn append_dir_recursive<W: IoWrite>(
    tar: &mut TarBuilder<W>,
    base: &Path,
    dir: &Path,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {e}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip directories that contain only regenerable artifacts
        const SKIP_DIRS: &[&str] = &[
            ".hush",        // daemon-local state
            "target",       // Rust build output
            "node_modules", // Node.js dependencies
            ".venv",        // Python virtualenv
            "venv",
            "__pycache__",  // Python bytecode
            ".cache",       // generic cache dir
            "dist",         // generic build output
            ".tox",         // Python tox environments
        ];
        if path.is_dir() && SKIP_DIRS.contains(&name) { continue; }

        let rel = path.strip_prefix(base)
            .map_err(|e| format!("strip_prefix: {e}"))?;

        if path.is_symlink() || path.is_file() {
            tar.append_path_with_name(&path, rel)
                .map_err(|e| format!("tar append {}: {e}", path.display()))?;
        } else if path.is_dir() {
            tar.append_dir(rel, &path)
                .map_err(|e| format!("tar append_dir {}: {e}", path.display()))?;
            append_dir_recursive(tar, base, &path)?;
        }
    }
    Ok(())
}

/// Build a plain tar of history files into `out_path`. Returns file size.
fn build_history_tar(files: &[PathBuf], out_path: &Path) -> Result<u64, String> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir transfers (hist): {e}"))?;
    }
    let file = std::fs::File::create(out_path)
        .map_err(|e| format!("create history tar: {e}"))?;
    {
        let mut tar = TarBuilder::new(&file);
        for f in files {
            if let Some(name) = f.file_name() {
                tar.append_path_with_name(f, name)
                    .map_err(|e| format!("tar history {}: {e}", f.display()))?;
            }
        }
        tar.into_inner()
            .map_err(|e| format!("tar history finish: {e}"))?;
    }
    let size = std::fs::metadata(out_path)
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(size)
}

/// Read a file and send its bytes as binary WS frames of `BINARY_CHUNK_SIZE`.
#[allow(clippy::too_many_arguments)]
async fn send_file_as_binary(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    path: &Path,
    bytes_sent: &mut u64,
    total_bytes: u64,
    tx: &broadcast::Sender<ServerMessage>,
    machine_id: &str,
    transfer_id: &str,
    source_worktree_id: &str,
    project_name: &str,
    branch: &str,
    dest_machine_id: &str,
) -> Result<(), String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = vec![0u8; BINARY_CHUNK_SIZE];

    loop {
        let n = reader.read(&mut buf)
            .map_err(|e| format!("read chunk: {e}"))?;
        if n == 0 { break; }

        ws.send(Message::Binary(buf[..n].to_vec().into()))
            .await
            .map_err(|e| format!("send binary frame: {e}"))?;

        *bytes_sent += n as u64;

        // Broadcast progress roughly every 1 MB
        if *bytes_sent % (1024 * 1024) < BINARY_CHUNK_SIZE as u64 {
            let _ = tx.send(ServerMessage::TransferProgress {
                machine_id: machine_id.to_string(),
                transfer_id: transfer_id.to_string(),
                phase: "streaming".to_string(),
                bytes_sent: *bytes_sent,
                total_bytes,
                source_worktree_id: source_worktree_id.to_string(),
                project_name: project_name.to_string(),
                branch: branch.to_string(),
                dest_machine_id: dest_machine_id.to_string(),
            });
        }
    }
    Ok(())
}

/// Wait for `transfer_ack`, returning `dest_path`.
async fn wait_for_ack(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    transfer_id: &str,
) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + ACK_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, ws.next())
            .await
            .map_err(|_| "timeout waiting for transfer_ack")?
            .ok_or("stream ended")?
            .map_err(|e| format!("ws recv: {e}"))?;

        if let Message::Text(text) = msg {
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("json: {e}"))?;
            if v.get("type").and_then(|t| t.as_str()) == Some("transfer_ack")
                && v.get("transfer_id").and_then(|t| t.as_str()) == Some(transfer_id)
            {
                return v.get("dest_path")
                    .and_then(|p| p.as_str())
                    .map(String::from)
                    .ok_or_else(|| "transfer_ack missing dest_path".to_string());
            }
        }
    }
}

/// Wait for `transfer_complete` with an idle watchdog (no hard total timeout).
/// Any inbound message (heartbeat, binary frame, etc.) resets the idle timer.
async fn wait_for_complete_idle(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    transfer_id: &str,
) -> Result<String, String> {
    loop {
        let msg = tokio::time::timeout(IDLE_TIMEOUT, ws.next())
            .await
            .map_err(|_| format!("idle timeout ({} s) — destination may have crashed", IDLE_TIMEOUT.as_secs()))?
            .ok_or("stream ended before transfer_complete")?
            .map_err(|e| format!("ws recv: {e}"))?;

        // Any message resets the idle timer (we just go back to the top of the loop).
        if let Message::Text(text) = msg {
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let type_ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let id = v.get("transfer_id").and_then(|t| t.as_str()).unwrap_or("");

            if type_ == "transfer_complete" && id == transfer_id {
                return v.get("new_worktree_id")
                    .and_then(|id| id.as_str())
                    .map(String::from)
                    .ok_or_else(|| "transfer_complete missing new_worktree_id".to_string());
            }
            if type_ == "transfer_error" && id == transfer_id {
                let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("unknown");
                return Err(format!("destination error: {msg}"));
            }
        }
        // Binary, ping, pong — all reset the idle timer via the loop.
    }
}


/// Re-spawn the local pty after a failed transfer.
async fn respawn_local(pty_manager: &PtyManager, worktree: &Worktree, tx: &broadcast::Sender<ServerMessage>, machine_id: &str) {
    if let Err(e) = pty_manager
        .spawn(
            worktree.id.clone(),
            &worktree.working_dir,
            &worktree.permission_mode,
            worktree.session_id.as_deref(),
            worktree.session_id.is_some(),
            220,
            50,
        )
        .await
    {
        warn!("Failed to respawn local pty after transfer failure: {e}");
        let _ = tx.send(ServerMessage::Error {
            machine_id: machine_id.to_string(),
            message: format!("Transfer failed AND could not respawn local session: {e}"),
            worktree_id: Some(worktree.id.clone()),
        });
    }
}

// ─── Destination side ─────────────────────────────────────────────────────────

/// Resolve the destination path for an inbound transfer.
///
/// Prefers placing the worktree next to the hinted project path (same convention
/// as a local `git worktree add`). Falls back to `{hush_dir}/worktrees/{project}/{branch}`
/// when the hinted parent directory doesn't exist or isn't writable — which is the
/// common case when transferring to a machine with a different directory layout.
pub fn resolve_dest_path(project_path_hint: &Path, branch: &str, hush_dir: &Path) -> Result<PathBuf, String> {
    let project_name = project_path_hint
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    if let Some(parent) = project_path_hint.parent() {
        if parent.exists() && is_writable(parent) {
            let worktree_base = parent.join(format!("{project_name}-worktrees"));
            return Ok(worktree_base.join(branch));
        }
    }

    // Hinted parent doesn't exist or isn't writable on this machine — use the
    // hush worktrees directory as a safe fallback.
    Ok(hush_dir.join("worktrees").join(&project_name).join(branch))
}

fn is_writable(path: &Path) -> bool {
    // A quick probe: try creating (and immediately removing) a temp file.
    let probe = path.join(".hush_write_probe");
    match std::fs::File::create(&probe) {
        Ok(_) => { let _ = std::fs::remove_file(&probe); true }
        Err(_) => false,
    }
}

/// Apply an inbound transfer: extract tars, install history, register worktree, spawn pty.
/// Broadcasts `TransferProgress` heartbeats every 10 s to keep the source's idle watchdog alive.
pub async fn apply_transfer(
    mut transfer: InboundTransfer,
    state: Arc<RwLock<DaemonState>>,
    state_path: PathBuf,
    tx: broadcast::Sender<ServerMessage>,
    pty_manager: PtyManager,
) -> Result<String, String> {
    let dest_path = transfer.dest_path.clone();
    info!("Transfer {}: applying to {}", transfer.transfer_id, dest_path.display());

    // Close write handles so we can reopen for reading
    transfer.close_files();

    let machine_id = state.read().await.machine_id.clone();

    // Spawn a heartbeat task to keep the source's idle watchdog alive
    let (hb_stop_tx, hb_stop_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let hb_tx = tx.clone();
        let hb_machine_id = machine_id.clone();
        let hb_tid = transfer.transfer_id.clone();
        let hb_total = transfer.total_bytes;
        let hb_branch = transfer.branch.clone();
        let hb_project = transfer.project_name.clone();
        let hb_from = transfer.from_machine_id.clone();
        tokio::spawn(async move {
            let mut stop = hb_stop_rx;
            loop {
                tokio::select! {
                    _ = &mut stop => break,
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {
                        let _ = hb_tx.send(ServerMessage::TransferProgress {
                            machine_id: hb_machine_id.clone(),
                            transfer_id: hb_tid.clone(),
                            phase: "extracting".to_string(),
                            bytes_sent: hb_total,
                            total_bytes: hb_total,
                            source_worktree_id: String::new(), // unknown on dest side
                            project_name: hb_project.clone(),
                            branch: hb_branch.clone(),
                            dest_machine_id: hb_machine_id.clone(),
                        });
                        // Also send "still alive" back toward source (dest→source direction)
                        // through the src WS — but since this goes via broadcast it reaches
                        // any browser on the destination too, which is fine.
                    }
                }
            }
        });
    }

    // Extract working_dir tar.gz
    let wd_path = transfer.working_dir_path.clone();
    if wd_path.exists() {
        std::fs::create_dir_all(&dest_path)
            .map_err(|e| format!("mkdir dest: {e}"))?;

        let dest = dest_path.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let file = std::fs::File::open(&wd_path)
                .map_err(|e| format!("open working_dir tar: {e}"))?;
            let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
            let mut archive = tar::Archive::new(gz);
            archive.set_preserve_permissions(false);
            archive.set_unpack_xattrs(false);
            archive.set_overwrite(true);
            archive.unpack(&dest)
                .map_err(|e| format!("tar unpack: {e}"))?;
            // Clean up temp file
            let _ = std::fs::remove_file(&wd_path);
            Ok(())
        })
        .await
        .map_err(|e| format!("spawn_blocking: {e}"))?
        .map_err(|e| e)?;
    }

    // Extract and install history
    let hist_path = transfer.history_path.clone();
    if transfer.has_history && hist_path.exists() {
        let _ = tx.send(ServerMessage::TransferProgress {
            machine_id: machine_id.clone(),
            transfer_id: transfer.transfer_id.clone(),
            phase: "installing_history".to_string(),
            bytes_sent: transfer.total_bytes,
            total_bytes: transfer.total_bytes,
            source_worktree_id: String::new(),
            project_name: transfer.project_name.clone(),
            branch: transfer.branch.clone(),
            dest_machine_id: machine_id.clone(),
        });

        let dest = dest_path.clone();
        let installed = tokio::task::spawn_blocking(move || -> Result<usize, String> {
            let tmp_dir = std::env::temp_dir().join(format!("hush-hist-{}", uuid_like()));
            std::fs::create_dir_all(&tmp_dir)
                .map_err(|e| format!("mkdir hist tmp: {e}"))?;

            let file = std::fs::File::open(&hist_path)
                .map_err(|e| format!("open history tar: {e}"))?;
            let mut archive = tar::Archive::new(file);
            archive.unpack(&tmp_dir)
                .map_err(|e| format!("history tar unpack: {e}"))?;

            let files: Vec<PathBuf> = std::fs::read_dir(&tmp_dir)
                .map_err(|e| format!("read hist tmp: {e}"))?
                .flatten()
                .map(|e| e.path())
                .collect();

            let n = claude_history::install_history_files(&files, &dest)?;
            let _ = std::fs::remove_dir_all(&tmp_dir);
            let _ = std::fs::remove_file(&hist_path);
            Ok(n)
        })
        .await
        .map_err(|e| format!("spawn_blocking hist: {e}"))?;

        match installed {
            Ok(n) => info!("Transfer {}: installed {n} history file(s)", transfer.transfer_id),
            Err(e) => warn!("Transfer {}: history install failed ({e}), session will start fresh", transfer.transfer_id),
        }
    }

    // Stop heartbeat
    let _ = hb_stop_tx.send(());

    // Send "spawning_pty" progress so source knows we're in the final phase
    let _ = tx.send(ServerMessage::TransferProgress {
        machine_id: machine_id.clone(),
        transfer_id: transfer.transfer_id.clone(),
        phase: "spawning_pty".to_string(),
        bytes_sent: transfer.total_bytes,
        total_bytes: transfer.total_bytes,
        source_worktree_id: String::new(),
        project_name: transfer.project_name.clone(),
        branch: transfer.branch.clone(),
        dest_machine_id: machine_id.clone(),
    });

    // Register project + worktree in state
    let new_wt_id = {
        let mut s = state.write().await;
        let project_id = s.upsert_project_for_transfer(
            &transfer.project_name,
            transfer.dest_path.parent()
                .and_then(|p| p.parent())
                .unwrap_or(&transfer.dest_path)
                .to_path_buf(),
        );
        let info = s.add_worktree_transferred(
            &project_id,
            transfer.branch.clone(),
            dest_path.clone(),
            transfer.permission_mode.clone(),
            transfer.session_id.clone(),
            transfer.last_task.clone(),
            transfer.from_machine_id.clone(),
        )?;
        s.save(&state_path);
        info.id
    };

    // Spawn pty with --resume <session_id> if available
    pty_manager
        .spawn(
            new_wt_id.clone(),
            &dest_path,
            &transfer.permission_mode,
            transfer.session_id.as_deref(),
            transfer.session_id.is_some(),
            220,
            50,
        )
        .await?;

    // Broadcast updated worktree list
    let worktrees = {
        let s = state.read().await;
        s.worktree_list()
    };
    let _ = tx.send(ServerMessage::WorktreeList { machine_id: machine_id.clone(), worktrees });

    Ok(new_wt_id)
}

/// Simple timestamp-based ID for temp directories.
fn uuid_like() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Return the peer URL for a machine_id from the state.
pub async fn peer_url_for(state: &Arc<RwLock<DaemonState>>, dest_machine_id: &str) -> Option<String> {
    let s = state.read().await;
    s.peers
        .iter()
        .find(|p| p.machine_id == dest_machine_id)
        .map(|p| p.url.clone())
        .filter(|u| !u.is_empty())
}

/// On daemon startup, create the transfers directory and remove any stale
/// temp files from a previous crashed run.
pub fn clean_transfers_dir(state_path: &Path) {
    let dir = transfers_dir(state_path);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("Could not create transfers dir {}: {e}", dir.display());
        return;
    }
    // Remove any .tar.gz / .history.tar left from a crash
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "gz" || e == "tar").unwrap_or(false) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}
