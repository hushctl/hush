// ─── Types mirroring daemon protocol (daemon/src/protocol.rs) ────────────────

export interface ProjectInfo {
  id: string
  name: string
  path: string
  worktree_count: number
  machine_id: string
}

export interface WorktreeInfo {
  id: string
  project_id: string
  branch: string
  working_dir: string
  status: WorktreeStatus
  last_task: string | null
  session_id: string | null
  machine_id: string
}

export type WorktreeStatus = 'idle' | 'running' | 'needs_you' | `failed: ${string}`

export interface PeerInfo {
  machine_id: string
  url: string
  last_seen: number
}

// ─── Client → Daemon ──────────────────────────────────────────────────────────

export type ClientMessage =
  | { type: 'register_project'; path: string; name: string }
  | { type: 'create_worktree'; project_id: string; branch: string; permission_mode?: string }
  | { type: 'delete_worktree'; worktree_id: string; cleanup_git: boolean }
  | { type: 'pty_attach'; worktree_id: string; cols: number; rows: number }
  | { type: 'pty_detach'; worktree_id: string }
  | { type: 'pty_input'; worktree_id: string; data: string }
  | { type: 'pty_resize'; worktree_id: string; cols: number; rows: number }
  | { type: 'pty_kill'; worktree_id: string }
  | { type: 'list_projects' }
  | { type: 'list_worktrees' }
  | { type: 'list_peers' }
  | { type: 'peer_hello'; machine_id: string; url: string; peers: PeerInfo[] }

// ─── Daemon → Client ──────────────────────────────────────────────────────────

export type ServerMessage =
  | { type: 'status_change'; machine_id: string; worktree_id: string; status: string }
  | { type: 'project_list'; machine_id: string; projects: ProjectInfo[] }
  | { type: 'worktree_list'; machine_id: string; worktrees: WorktreeInfo[] }
  | { type: 'session_ended'; machine_id: string; worktree_id: string; exit_code: number | null }
  | { type: 'pty_data'; machine_id: string; worktree_id: string; data: string }
  | { type: 'pty_scrollback'; machine_id: string; worktree_id: string; data: string }
  | { type: 'pty_exit'; machine_id: string; worktree_id: string; code: number | null }
  | { type: 'error'; machine_id: string; message: string; worktree_id: string | null }
  | { type: 'peer_list'; machine_id: string; peers: PeerInfo[] }
