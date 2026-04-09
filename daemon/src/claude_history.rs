//! Claude Code conversation history utilities.
//!
//! Claude Code stores conversation history at:
//!   ~/.claude/projects/<slug>/<session_id>.jsonl
//!
//! The slug is derived from the cwd by replacing every '/' with '-', so:
//!   /Users/admin/work/project → -Users-admin-work-project
//!
//! When a worktree moves to a different absolute path on a different machine,
//! we must install the jsonl under the *new* slug so Claude Code's
//! `--resume <session_id>` can find it.

use std::path::{Path, PathBuf};

/// Return the slug Claude Code uses for a given working directory path.
/// Example: /Users/admin/work/project → -Users-admin-work-project
pub fn slug_for(working_dir: &Path) -> String {
    working_dir.to_string_lossy().replace('/', "-")
}

/// Return the ~/.claude/projects/<slug>/ directory for a working dir, or None
/// if the home directory cannot be determined.
pub fn history_dir_for(working_dir: &Path) -> Option<PathBuf> {
    let base = dirs::home_dir()?.join(".claude").join("projects");
    Some(base.join(slug_for(working_dir)))
}

/// Find the jsonl file for a specific session_id by scanning all project dirs
/// under ~/.claude/projects/. Returns the path if found.
pub fn find_session_jsonl(session_id: &str) -> Option<PathBuf> {
    let base = dirs::home_dir()?.join(".claude").join("projects");
    let entries = std::fs::read_dir(&base).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(format!("{session_id}.jsonl"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Collect files that should be transferred for a session:
/// - The session's .jsonl file
/// - Any *.json summary/sidecar files in the same directory
pub fn session_files_to_transfer(session_id: &str) -> Vec<PathBuf> {
    let Some(jsonl) = find_session_jsonl(session_id) else { return vec![] };
    let dir = match jsonl.parent() {
        Some(d) => d.to_path_buf(),
        None => return vec![jsonl],
    };

    let mut files = vec![jsonl];
    // Include small sidecar json files (summaries etc.)
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") && path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Install history files into the slug directory for dest_working_dir,
/// creating the directory if needed. Returns the number of files installed.
pub fn install_history_files(files: &[PathBuf], dest_working_dir: &Path) -> Result<usize, String> {
    let Some(dest_dir) = history_dir_for(dest_working_dir) else {
        return Err("Cannot determine home directory".to_string());
    };
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create history dir {}: {e}", dest_dir.display()))?;

    let mut count = 0;
    for src in files {
        if let Some(name) = src.file_name() {
            let dst = dest_dir.join(name);
            std::fs::copy(src, &dst)
                .map_err(|e| format!("Failed to copy {} → {}: {e}", src.display(), dst.display()))?;
            count += 1;
        }
    }
    Ok(count)
}
