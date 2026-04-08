use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{broadcast, Mutex};
use tracing::debug;

use crate::protocol::ServerMessage;

/// Polls `git status` every 3 seconds for open worktrees and broadcasts
/// `ServerMessage::GitStatus` when the output changes.
#[derive(Clone)]
pub struct GitWatcher {
    active: Arc<Mutex<HashSet<String>>>,
    tx: broadcast::Sender<ServerMessage>,
    machine_id: String,
}

impl GitWatcher {
    pub fn new(tx: broadcast::Sender<ServerMessage>, machine_id: String) -> Self {
        Self {
            active: Arc::new(Mutex::new(HashSet::new())),
            tx,
            machine_id,
        }
    }

    /// Start polling git status for `worktree_id` every 3s.
    /// No-op if already watching that worktree.
    pub async fn start_watching(&self, worktree_id: String, working_dir: PathBuf) {
        {
            let mut active = self.active.lock().await;
            if active.contains(&worktree_id) {
                return; // already watching
            }
            active.insert(worktree_id.clone());
        }

        let active = Arc::clone(&self.active);
        let tx = self.tx.clone();
        let machine_id = self.machine_id.clone();

        tokio::spawn(async move {
            let mut prev: Option<(Vec<String>, Vec<String>, Vec<String>)> = None;
            loop {
                // Exit if we've been removed from the active set
                {
                    let active = active.lock().await;
                    if !active.contains(&worktree_id) {
                        debug!("GitWatcher: stopping poller for {worktree_id}");
                        break;
                    }
                }

                match run_git_status(&working_dir).await {
                    Ok((staged, modified, untracked)) => {
                        let changed = prev.as_ref().map_or(true, |(s, m, u)| {
                            s != &staged || m != &modified || u != &untracked
                        });
                        if changed {
                            let _ = tx.send(ServerMessage::GitStatus {
                                machine_id: machine_id.clone(),
                                worktree_id: worktree_id.clone(),
                                staged: staged.clone(),
                                modified: modified.clone(),
                                untracked: untracked.clone(),
                            });
                            prev = Some((staged, modified, untracked));
                        }
                    }
                    Err(e) => {
                        debug!("GitWatcher: git status error for {worktree_id}: {e}");
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            }
        });
    }

    /// Stop polling git status for `worktree_id`. The running task will exit
    /// at its next loop iteration.
    pub async fn stop_watching(&self, worktree_id: &str) {
        let mut active = self.active.lock().await;
        active.remove(worktree_id);
    }
}

/// Run `git status --porcelain=v1 -z` and bucket the results.
pub async fn run_git_status(
    working_dir: &PathBuf,
) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
    let output = tokio::process::Command::new("git")
        .args(["status", "--porcelain=v1", "-z"])
        .current_dir(working_dir)
        .output()
        .await
        .map_err(|e| format!("git status failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "git status exited non-zero: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();

    // porcelain=v1 -z: entries separated by NUL.
    // Each valid entry: "XY path" where XY are 2 status chars and path starts at index 3.
    // Rename entries produce a second NUL-separated token (orig path) which we skip
    // by checking that the 3rd char is a space.
    for record in stdout.split('\0') {
        let bytes = record.as_bytes();
        if bytes.len() < 4 || bytes[2] != b' ' {
            continue; // skip empty records and orig-path tokens from renames
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let path = record[3..].to_string();
        if path.is_empty() {
            continue;
        }

        if x == '?' && y == '?' {
            untracked.push(path);
        } else {
            if x != ' ' {
                staged.push(path.clone());
            }
            if y != ' ' {
                modified.push(path);
            }
        }
    }

    Ok((staged, modified, untracked))
}
