use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::protocol::{ProjectInfo, WorktreeInfo};

/// A peer daemon in the gossip mesh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerInfo {
    pub machine_id: String,
    /// WebSocket URL the peer should be dialled at (e.g. ws://laptop:9111/ws).
    pub url: String,
    /// Unix timestamp of the last successful contact (seconds).
    #[serde(default)]
    pub last_seen: u64,
    /// Daemon version string (e.g. "0.9.1"). Empty for pre-version peers.
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonState {
    /// Stable identity for this machine (hostname or --machine-name override).
    #[serde(default)]
    pub machine_id: String,
    /// URL at which peers can reach us (set via --advertise-url; empty means unknown).
    #[serde(default)]
    pub advertise_url: String,
    /// Known peers in the gossip mesh.
    #[serde(default)]
    pub peers: Vec<PeerInfo>,
    pub projects: Vec<Project>,
    pub next_project_id: u32,
    pub next_worktree_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub id: String,
    pub project_id: String,
    pub branch: String,
    pub working_dir: PathBuf,
    pub permission_mode: String,
    pub status: WorktreeStatus,
    pub last_task: Option<String>,
    /// Claude Code session_id captured from the init event; used for --resume
    pub session_id: Option<String>,
    /// Set when this worktree was transferred from another machine.
    #[serde(default)]
    pub origin_machine_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeStatus {
    Idle,
    Running,
    NeedsYou,
    Failed(String),
}

impl WorktreeStatus {
    pub fn as_str(&self) -> String {
        match self {
            WorktreeStatus::Idle => "idle".to_string(),
            WorktreeStatus::Running => "running".to_string(),
            WorktreeStatus::NeedsYou => "needs_you".to_string(),
            WorktreeStatus::Failed(msg) => format!("failed: {msg}"),
        }
    }
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            machine_id: String::new(),
            advertise_url: String::new(),
            peers: Vec::new(),
            projects: Vec::new(),
            next_project_id: 1,
            next_worktree_id: 1,
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl DaemonState {
    /// Merge a peer into the local list. Updates `last_seen` if already present.
    pub fn merge_peer(&mut self, peer: PeerInfo) {
        if peer.machine_id == self.machine_id || peer.url.is_empty() {
            return; // never add ourselves or peers without a reachable URL
        }
        if let Some(existing) = self.peers.iter_mut().find(|p| p.machine_id == peer.machine_id) {
            existing.url = peer.url;
            existing.last_seen = peer.last_seen.max(existing.last_seen);
        } else {
            self.peers.push(peer);
        }
    }

    /// Merge a slice of peers (e.g. received in a peer_list message).
    pub fn merge_peers(&mut self, peers: Vec<PeerInfo>) {
        for p in peers {
            self.merge_peer(p);
        }
    }

    /// Remove peers whose last_seen is older than `max_age_secs`.
    pub fn prune_stale(&mut self, max_age_secs: u64) {
        let cutoff = now_secs().saturating_sub(max_age_secs);
        self.peers.retain(|p| p.last_seen >= cutoff || p.last_seen == 0);
    }

    /// Touch a peer's `last_seen` timestamp to now.
    pub fn touch_peer(&mut self, machine_id: &str) {
        if let Some(p) = self.peers.iter_mut().find(|p| p.machine_id == machine_id) {
            p.last_seen = now_secs();
        }
    }

    /// All peers (snapshot clone).
    pub fn known_peers(&self) -> Vec<PeerInfo> {
        self.peers.clone()
    }

    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(state) => state,
                Err(e) => {
                    warn!("Failed to parse state file: {e}. Starting fresh.");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &PathBuf) {
        let tmp = path.with_extension("json.tmp");
        let contents = match serde_json::to_string_pretty(self) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to serialize state: {e}");
                return;
            }
        };
        if let Err(e) = std::fs::write(&tmp, &contents) {
            warn!("Failed to write state tmp file: {e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, path) {
            warn!("Failed to rename state file: {e}");
        }
    }

    pub fn register_project(&mut self, path: PathBuf, name: String) -> ProjectInfo {
        // Return existing if same path
        if let Some(p) = self.projects.iter().find(|p| p.path == path) {
            return ProjectInfo {
                id: p.id.clone(),
                name: p.name.clone(),
                path: p.path.to_string_lossy().to_string(),
                worktree_count: p.worktrees.len(),
                machine_id: self.machine_id.clone(),
            };
        }
        let id = format!("proj_{}", self.next_project_id);
        self.next_project_id += 1;
        let project = Project {
            id: id.clone(),
            name: name.clone(),
            path: path.clone(),
            worktrees: Vec::new(),
        };
        self.projects.push(project);
        ProjectInfo {
            id,
            name,
            path: path.to_string_lossy().to_string(),
            worktree_count: 0,
            machine_id: self.machine_id.clone(),
        }
    }

    pub fn add_worktree(
        &mut self,
        project_id: &str,
        branch: String,
        working_dir: PathBuf,
        permission_mode: String,
    ) -> Result<WorktreeInfo, String> {
        let project = self
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project {project_id} not found"))?;

        let id = format!("wt_{}", self.next_worktree_id);
        self.next_worktree_id += 1;

        let worktree = Worktree {
            id: id.clone(),
            project_id: project_id.to_string(),
            branch: branch.clone(),
            working_dir: working_dir.clone(),
            permission_mode: permission_mode.clone(),
            status: WorktreeStatus::Idle,
            last_task: None,
            session_id: None,
            origin_machine_id: None,
        };

        let info = WorktreeInfo {
            id: id.clone(),
            project_id: project_id.to_string(),
            branch,
            working_dir: working_dir.to_string_lossy().to_string(),
            status: worktree.status.as_str(),
            last_task: None,
            session_id: None,
            machine_id: self.machine_id.clone(),
        };

        project.worktrees.push(worktree);
        Ok(info)
    }

    /// Like add_worktree but also sets session_id, last_task, and origin_machine_id for transfers.
    pub fn add_worktree_transferred(
        &mut self,
        project_id: &str,
        branch: String,
        working_dir: PathBuf,
        permission_mode: String,
        session_id: Option<String>,
        last_task: Option<String>,
        origin_machine_id: String,
    ) -> Result<WorktreeInfo, String> {
        let project = self
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project {project_id} not found"))?;

        let id = format!("wt_{}", self.next_worktree_id);
        self.next_worktree_id += 1;

        let worktree = Worktree {
            id: id.clone(),
            project_id: project_id.to_string(),
            branch: branch.clone(),
            working_dir: working_dir.clone(),
            permission_mode: permission_mode.clone(),
            status: WorktreeStatus::Idle,
            last_task: last_task.clone(),
            session_id: session_id.clone(),
            origin_machine_id: Some(origin_machine_id.clone()),
        };

        let info = WorktreeInfo {
            id: id.clone(),
            project_id: project_id.to_string(),
            branch,
            working_dir: working_dir.to_string_lossy().to_string(),
            status: worktree.status.as_str(),
            last_task,
            session_id,
            machine_id: self.machine_id.clone(),
        };

        project.worktrees.push(worktree);
        Ok(info)
    }

    pub fn find_worktree_mut(&mut self, id: &str) -> Option<&mut Worktree> {
        self.projects
            .iter_mut()
            .flat_map(|p| p.worktrees.iter_mut())
            .find(|w| w.id == id)
    }

    /// Remove a worktree by id, returning the removed record (caller saves state).
    pub fn remove_worktree(&mut self, id: &str) -> Option<Worktree> {
        for project in &mut self.projects {
            if let Some(pos) = project.worktrees.iter().position(|w| w.id == id) {
                return Some(project.worktrees.remove(pos));
            }
        }
        None
    }

    /// Remove a project if it has no worktrees left.
    pub fn remove_project_if_empty(&mut self, project_id: &str) {
        self.projects.retain(|p| !(p.id == project_id && p.worktrees.is_empty()));
    }

    /// Ensure a project is registered under the given name and path, returning its id.
    /// Idempotent: returns the existing id if the path is already known.
    pub fn upsert_project_for_transfer(&mut self, name: &str, path: PathBuf) -> String {
        if let Some(p) = self.projects.iter().find(|p| p.path == path) {
            return p.id.clone();
        }
        let id = format!("proj_{}", self.next_project_id);
        self.next_project_id += 1;
        self.projects.push(Project {
            id: id.clone(),
            name: name.to_string(),
            path,
            worktrees: Vec::new(),
        });
        id
    }

    pub fn find_worktree(&self, id: &str) -> Option<&Worktree> {
        self.projects
            .iter()
            .flat_map(|p| p.worktrees.iter())
            .find(|w| w.id == id)
    }

    pub fn project_list(&self) -> Vec<ProjectInfo> {
        self.projects
            .iter()
            .map(|p| ProjectInfo {
                id: p.id.clone(),
                name: p.name.clone(),
                path: p.path.to_string_lossy().to_string(),
                worktree_count: p.worktrees.len(),
                machine_id: self.machine_id.clone(),
            })
            .collect()
    }

    pub fn worktree_list(&self) -> Vec<WorktreeInfo> {
        self.projects
            .iter()
            .flat_map(|p| {
                p.worktrees.iter().map(|w| WorktreeInfo {
                    id: w.id.clone(),
                    project_id: w.project_id.clone(),
                    branch: w.branch.clone(),
                    working_dir: w.working_dir.to_string_lossy().to_string(),
                    status: w.status.as_str(),
                    last_task: w.last_task.clone(),
                    session_id: w.session_id.clone(),
                    machine_id: self.machine_id.clone(),
                })
            })
            .collect()
    }
}
