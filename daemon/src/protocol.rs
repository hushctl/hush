use serde::{Deserialize, Serialize};

use crate::state::PeerInfo;

/// Messages sent from browser/wscat → daemon
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    RegisterProject {
        path: String,
        name: String,
    },
    CreateWorktree {
        project_id: String,
        branch: String,
        #[serde(default = "default_permission_mode")]
        permission_mode: String,
    },
    /// Attach to a worktree's pty. Spawns the pty if not already running.
    /// Daemon responds by replaying scrollback and streaming live output.
    PtyAttach {
        worktree_id: String,
        cols: u16,
        rows: u16,
    },
    /// Detach from streaming. Pty keeps running.
    PtyDetach {
        worktree_id: String,
    },
    /// Forward keystrokes / utf-8 input to the pty stdin.
    PtyInput {
        worktree_id: String,
        data: String,
    },
    /// Resize the pty.
    PtyResize {
        worktree_id: String,
        cols: u16,
        rows: u16,
    },
    /// Kill the pty (close session).
    PtyKill {
        worktree_id: String,
    },
    /// Browser pastes an image into the terminal. Daemon writes the bytes to
    /// `~/.hush/paste/<filename>` and injects the absolute path (with a
    /// trailing space) into the pty's stdin so Claude Code picks it up the
    /// same way it handles drag-and-drop file paths.
    PasteImage {
        worktree_id: String,
        /// Base64-encoded image bytes (PNG, JPEG, etc).
        data: String,
        /// Optional filename hint; if missing, a timestamp-based name is used.
        #[serde(default)]
        filename: Option<String>,
    },
    /// Confirm creation of a missing directory + git init, then register it.
    CreateAndRegisterProject {
        path: String,
        name: String,
    },
    /// One-shot git status snapshot for a worktree.
    GitStatus {
        worktree_id: String,
    },
    /// List all non-gitignored files in a worktree (for cmd+P).
    ListFiles {
        worktree_id: String,
    },
    /// Read a file from a worktree's working dir (relative path).
    ReadFile {
        worktree_id: String,
        path: String,
    },
    /// Attach to a worktree's shell pty (plain bash/zsh, not claude).
    /// Spawns the shell if not already running.
    ShellAttach {
        worktree_id: String,
        #[serde(default)]
        shell_id: String,
        cols: u16,
        rows: u16,
    },
    /// Forward keystrokes to a worktree's shell pty.
    ShellInput {
        worktree_id: String,
        #[serde(default)]
        shell_id: String,
        data: String,
    },
    /// Resize a worktree's shell pty.
    ShellResize {
        worktree_id: String,
        #[serde(default)]
        shell_id: String,
        cols: u16,
        rows: u16,
    },
    /// Kill a worktree's shell pty.
    ShellKill {
        worktree_id: String,
        #[serde(default)]
        shell_id: String,
    },
    ListProjects,
    ListWorktrees,
    /// Browser asks a daemon for its known peers.
    ListPeers,
    /// Daemon-to-daemon gossip greeting. Also accepted from browsers for symmetry.
    PeerHello {
        machine_id: String,
        url: String,
        peers: Vec<PeerInfo>,
        /// Sender's daemon version (e.g. "0.9.1"). Empty for pre-version peers.
        #[serde(default)]
        version: String,
        /// CA cert PEM — sent so joining machines can adopt the mesh CA.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ca_cert_pem: Option<String>,
        /// CA private key PEM — sent so joining machines can sign their own leaf certs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ca_key_pem: Option<String>,
    },

    // ── Transfer: browser → source daemon ────────────────────────────────────
    /// Move a single worktree to another daemon (browser → source daemon).
    TransferWorktree {
        worktree_id: String,
        dest_machine_id: String,
    },
    /// Move an entire project (all worktrees) to another daemon.
    TransferProject {
        project_id: String,
        dest_machine_id: String,
    },
    /// Remove a worktree record, kill its pty, and run `git worktree remove`.
    RemoveWorktree {
        worktree_id: String,
    },

    // ── Transfer: source daemon → destination daemon ──────────────────────────
    /// First message: describes what is about to be transferred.
    TransferOffer {
        transfer_id: String,
        from_machine_id: String,
        project_name: String,
        /// Absolute path of the project root on the source (hint for dest layout).
        project_path_hint: String,
        branch: String,
        permission_mode: String,
        session_id: Option<String>,
        last_task: Option<String>,
        /// Whether a history tar follows the working_dir tar.
        has_history: bool,
        /// Combined expected bytes (working_dir tar.gz + history tar).
        total_bytes: u64,
    },
    /// Switch the binary-frame stream to a different payload kind.
    /// Sent between the working_dir stream and the history stream.
    TransferKindSwitch {
        transfer_id: String,
        /// "working_dir" | "history"
        kind: String,
    },
    /// Source signals that all bytes have been sent; destination should apply.
    TransferCommit {
        transfer_id: String,
    },
    /// Either side can abort; destination should discard temp state.
    TransferAbort {
        transfer_id: String,
        reason: String,
    },

    // ── Peer upgrade: browser → source daemon ────────────────────────────────
    /// Browser asks this daemon to push its binary to an older peer.
    PeerUpgrade {
        dest_machine_id: String,
    },

    // ── Peer upgrade: source daemon → destination daemon ─────────────────────
    /// Source daemon offers its binary to the destination.
    UpgradeOffer {
        upgrade_id: String,
        from_machine_id: String,
        /// Version being offered (e.g. "0.9.2").
        version: String,
        /// Platform identifier (e.g. "darwin-aarch64"). Dest rejects mismatches.
        platform: String,
        /// Total compressed bytes that will follow as binary frames.
        total_bytes: u64,
    },
    /// Source signals that all binary frames have been sent; destination should apply.
    UpgradeCommit {
        upgrade_id: String,
    },
}

fn default_permission_mode() -> String {
    "dangerously-skip-permissions".to_string()
}

/// Messages sent from daemon → browser/wscat
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    StatusChange {
        machine_id: String,
        worktree_id: String,
        status: String,
    },
    ProjectList {
        machine_id: String,
        projects: Vec<ProjectInfo>,
    },
    WorktreeList {
        machine_id: String,
        worktrees: Vec<WorktreeInfo>,
    },
    SessionEnded {
        machine_id: String,
        worktree_id: String,
        exit_code: Option<i32>,
    },
    /// Base64-encoded raw bytes from a pty's stdout/stderr.
    PtyData {
        machine_id: String,
        worktree_id: String,
        data: String,
    },
    /// Pty's child process has exited.
    PtyExit {
        machine_id: String,
        worktree_id: String,
        code: Option<i32>,
    },
    /// Initial scrollback replay sent in response to a PtyAttach.
    PtyScrollback {
        machine_id: String,
        worktree_id: String,
        data: String,
    },
    Error {
        machine_id: String,
        message: String,
        worktree_id: Option<String>,
    },
    /// System memory pressure level changed. Only sent on transitions between levels.
    MemoryPressure {
        machine_id: String,
        /// "normal" | "warning" | "critical"
        level: String,
        available_bytes: u64,
        total_bytes: u64,
    },
    /// Sent when RegisterProject path does not exist on this machine.
    /// Browser should ask the user if they want to create it.
    PathNotFound {
        machine_id: String,
        path: String,
        name: String,
    },
    /// Response to ListPeers / PeerHello — also used for daemon-to-daemon replies.
    PeerList {
        machine_id: String,
        peers: Vec<PeerInfo>,
        /// Sender's daemon version (e.g. "0.9.1").
        #[serde(default)]
        version: String,
        /// CA cert PEM — sent so joining machines can adopt the mesh CA.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ca_cert_pem: Option<String>,
        /// CA private key PEM — sent so joining machines can sign their own leaf certs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ca_key_pem: Option<String>,
    },

    // ── Peer upgrade responses (destination → source) ─────────────────────────
    /// Destination is ready to receive upgrade binary frames.
    UpgradeAck {
        machine_id: String,
        upgrade_id: String,
    },
    /// Streamed bytes progress (source → browser, dest → browser).
    UpgradeProgress {
        machine_id: String,
        upgrade_id: String,
        dest_machine_id: String,
        bytes_sent: u64,
        total_bytes: u64,
    },
    /// Upgrade applied; destination is restarting.
    UpgradeComplete {
        machine_id: String,
        upgrade_id: String,
        dest_machine_id: String,
        version: String,
    },
    /// Upgrade failed at source or destination.
    UpgradeError {
        machine_id: String,
        upgrade_id: String,
        message: String,
    },
    /// Live git status for a worktree (from poller or one-shot request).
    GitStatus {
        machine_id: String,
        worktree_id: String,
        staged: Vec<String>,
        modified: Vec<String>,
        untracked: Vec<String>,
    },
    /// All non-gitignored files in a worktree (response to ListFiles).
    FileList {
        machine_id: String,
        worktree_id: String,
        files: Vec<String>,
    },
    /// Contents of a file in a worktree (response to ReadFile).
    FileContent {
        machine_id: String,
        worktree_id: String,
        path: String,
        content: String,
        truncated: bool,
    },
    /// Base64-encoded output from a worktree's shell pty.
    ShellData {
        machine_id: String,
        worktree_id: String,
        shell_id: String,
        data: String,
    },
    /// Scrollback replay sent in response to ShellAttach.
    ShellScrollback {
        machine_id: String,
        worktree_id: String,
        shell_id: String,
        data: String,
    },
    /// Shell pty process has exited.
    ShellExit {
        machine_id: String,
        worktree_id: String,
        shell_id: String,
        code: Option<i32>,
    },

    // ── Transfer responses ────────────────────────────────────────────────────
    /// Destination accepted the offer and reserved dest_path.
    TransferAck {
        machine_id: String,
        transfer_id: String,
        dest_path: String,
    },
    /// Destination applied the transfer and spawned the pty.
    TransferComplete {
        machine_id: String,
        transfer_id: String,
        new_worktree_id: String,
    },
    /// Transfer failed (either side).
    TransferError {
        machine_id: String,
        transfer_id: String,
        message: String,
    },
    /// Progress update broadcast to browsers on the source daemon.
    /// Carries enough context for the UI overlay without a separate lookup.
    TransferProgress {
        machine_id: String,
        transfer_id: String,
        /// "starting" | "streaming" | "extracting" | "installing_history" | "spawning_pty" | "complete" | "failed"
        phase: String,
        bytes_sent: u64,
        total_bytes: u64,
        /// Raw (un-namespaced) worktree id on the source daemon.
        source_worktree_id: String,
        project_name: String,
        branch: String,
        dest_machine_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub worktree_count: usize,
    pub machine_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub id: String,
    pub project_id: String,
    pub branch: String,
    pub working_dir: String,
    pub status: String,
    pub last_task: Option<String>,
    pub session_id: Option<String>,
    pub machine_id: String,
    /// Whether a shell pty is currently alive for this worktree.
    #[serde(default)]
    pub shell_alive: bool,
}
