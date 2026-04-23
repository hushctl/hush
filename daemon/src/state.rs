use std::collections::HashMap;
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
    /// Auth tokens for known peers, keyed by machine_id. Received over the
    /// mTLS-authenticated /peer channel. Not persisted to disk.
    #[serde(skip)]
    pub peer_tokens: HashMap<String, String>,
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
    /// Prompts waiting to be dispatched. When the worktree transitions to idle
    /// and this is non-empty, the daemon auto-dispatches the first entry.
    #[serde(default)]
    pub queued_tasks: Vec<String>,
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
            peer_tokens: HashMap::new(),
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
        if let Some(existing) = self
            .peers
            .iter_mut()
            .find(|p| p.machine_id == peer.machine_id)
        {
            existing.url = peer.url;
            existing.last_seen = peer.last_seen.max(existing.last_seen);
        } else {
            self.peers.push(peer);
        }
    }

    /// Store a peer's auth token (received over mTLS-authenticated channel).
    pub fn store_peer_token(&mut self, machine_id: &str, token: String) {
        self.peer_tokens.insert(machine_id.to_string(), token);
    }

    /// All known peer tokens, for relaying to the browser via /config/peers.
    pub fn peer_tokens_snapshot(&self) -> HashMap<String, String> {
        self.peer_tokens.clone()
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
        self.peers
            .retain(|p| p.last_seen >= cutoff || p.last_seen == 0);
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
            queued_tasks: Vec::new(),
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
            shell_alive: false,
            queued_tasks: Vec::new(),
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
            queued_tasks: Vec::new(),
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
            shell_alive: false,
            queued_tasks: Vec::new(),
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
        self.projects
            .retain(|p| !(p.id == project_id && p.worktrees.is_empty()));
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
                    shell_alive: false, // populated by caller with pty_manager.exists()
                    queued_tasks: w.queued_tasks.clone(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_state(machine_id: &str) -> DaemonState {
        let mut s = DaemonState::default();
        s.machine_id = machine_id.to_string();
        s
    }

    fn make_peer(id: &str, url: &str) -> PeerInfo {
        PeerInfo {
            machine_id: id.to_string(),
            url: url.to_string(),
            last_seen: 1000,
            version: "0.12.0".to_string(),
        }
    }

    #[test]
    fn merge_peer_adds_new() {
        let mut s = make_state("me");
        s.merge_peer(make_peer("other", "wss://other:9111/ws"));
        assert_eq!(s.peers.len(), 1);
        assert_eq!(s.peers[0].machine_id, "other");
    }

    #[test]
    fn merge_peer_updates_existing() {
        let mut s = make_state("me");
        s.merge_peer(make_peer("other", "wss://old:9111/ws"));
        s.merge_peer(PeerInfo {
            machine_id: "other".to_string(),
            url: "wss://new:9111/ws".to_string(),
            last_seen: 2000,
            version: "0.12.1".to_string(),
        });
        assert_eq!(s.peers.len(), 1);
        assert_eq!(s.peers[0].url, "wss://new:9111/ws");
        assert_eq!(s.peers[0].last_seen, 2000);
    }

    #[test]
    fn merge_peer_ignores_self() {
        let mut s = make_state("me");
        s.merge_peer(make_peer("me", "wss://me:9111/ws"));
        assert!(s.peers.is_empty());
    }

    #[test]
    fn merge_peer_ignores_empty_url() {
        let mut s = make_state("me");
        s.merge_peer(make_peer("other", ""));
        assert!(s.peers.is_empty());
    }

    #[test]
    fn prune_stale_removes_old_peers() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut s = make_state("me");
        s.merge_peer(PeerInfo {
            machine_id: "old".to_string(),
            url: "wss://old:9111/ws".to_string(),
            last_seen: 1, // very old
            version: String::new(),
        });
        s.merge_peer(PeerInfo {
            machine_id: "recent".to_string(),
            url: "wss://recent:9111/ws".to_string(),
            last_seen: now, // just now
            version: "0.12.0".to_string(),
        });
        s.prune_stale(500); // anything older than 500s from now
        // "old" should be pruned, "recent" should remain
        assert_eq!(s.peers.len(), 1);
        assert_eq!(s.peers[0].machine_id, "recent");
    }

    #[test]
    fn prune_stale_keeps_zero_last_seen() {
        let mut s = make_state("me");
        s.merge_peer(PeerInfo {
            machine_id: "new".to_string(),
            url: "wss://new:9111/ws".to_string(),
            last_seen: 0, // never contacted yet
            version: String::new(),
        });
        s.prune_stale(1);
        assert_eq!(s.peers.len(), 1);
    }

    #[test]
    fn register_project_is_idempotent() {
        let mut s = make_state("me");
        let p1 = s.register_project(PathBuf::from("/tmp/proj"), "proj".to_string());
        let p2 = s.register_project(PathBuf::from("/tmp/proj"), "proj".to_string());
        assert_eq!(p1.id, p2.id);
        assert_eq!(s.projects.len(), 1);
    }

    #[test]
    fn add_and_find_worktree() {
        let mut s = make_state("me");
        s.register_project(PathBuf::from("/tmp/proj"), "proj".to_string());
        let wt = s
            .add_worktree("proj_1", "main".to_string(), PathBuf::from("/tmp/proj/main"), "default".to_string())
            .unwrap();
        assert!(s.find_worktree(&wt.id).is_some());
        assert_eq!(s.find_worktree(&wt.id).unwrap().branch, "main");
    }

    #[test]
    fn remove_worktree_returns_removed() {
        let mut s = make_state("me");
        s.register_project(PathBuf::from("/tmp/proj"), "proj".to_string());
        let wt = s
            .add_worktree("proj_1", "main".to_string(), PathBuf::from("/tmp/proj/main"), "default".to_string())
            .unwrap();
        let removed = s.remove_worktree(&wt.id);
        assert!(removed.is_some());
        assert!(s.find_worktree(&wt.id).is_none());
    }

    #[test]
    fn remove_project_if_empty_keeps_nonempty() {
        let mut s = make_state("me");
        s.register_project(PathBuf::from("/tmp/proj"), "proj".to_string());
        s.add_worktree("proj_1", "main".to_string(), PathBuf::from("/tmp/proj/main"), "default".to_string())
            .unwrap();
        s.remove_project_if_empty("proj_1");
        assert_eq!(s.projects.len(), 1); // still there
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let mut s = make_state("roundtrip-machine");
        s.register_project(PathBuf::from("/tmp/proj"), "proj".to_string());
        s.merge_peer(make_peer("peer1", "wss://peer1:9111/ws"));
        s.save(&path);

        let loaded = DaemonState::load(&path);
        assert_eq!(loaded.machine_id, "roundtrip-machine");
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.peers.len(), 1);
    }
}
