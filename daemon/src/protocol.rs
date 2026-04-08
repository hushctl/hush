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
    /// Confirm creation of a missing directory + git init, then register it.
    CreateAndRegisterProject {
        path: String,
        name: String,
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
    },
}

fn default_permission_mode() -> String {
    "default".to_string()
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
}
