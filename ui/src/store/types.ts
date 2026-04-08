import type { ProjectInfo, WorktreeInfo, ClientMessage, PeerInfo } from '@/lib/protocol'

export interface DaemonConfig {
  /** Stable machine identity (machine_id from the daemon) */
  id: string
  /** Human-readable label shown in the UI */
  name: string
  /** WebSocket URL, e.g. ws://localhost:9111/ws */
  url: string
  /** Live connection status */
  connected: boolean
}

export interface AppState {
  // ── Per-daemon registry ───────────────────────────────────────────────────
  /** Keyed by machine_id. Persisted to localStorage. */
  daemons: Record<string, DaemonConfig>

  // ── Daemon data (namespaced keys: `${machineId}:${rawId}`) ───────────────
  projects: Record<string, ProjectInfo>
  worktrees: Record<string, WorktreeInfo>

  // ── UI state ─────────────────────────────────────────────────────────────
  layoutMode: 'grid' | 'panes' | 'tree'
  activePanes: string[]         // namespaced worktree IDs open in panes (max 2)
  selectedWorktreeId: string | null
  selectedProjectId: string | null
  tileMode: '1-up' | '2-up'

  // Transient error message from daemon (cleared after display)
  daemonError: string | null
  // Set when the daemon reports the project path doesn't exist — triggers the "create dir?" prompt
  pendingCreate: { path: string; name: string; machineId: string } | null

  // ── WebSocket send function (per machine_id, injected by hook) ───────────
  /**
   * Send a ClientMessage to the specified daemon (by machine_id).
   * For pty_* messages, callers pass the raw (un-namespaced) worktree ID
   * because the daemon never sees namespaced IDs.
   */
  send: (machineId: string, msg: ClientMessage) => void

  // ── Actions ───────────────────────────────────────────────────────────────
  clearDaemonError: () => void
  clearPendingCreate: () => void
  setDaemonConnected: (machineId: string, connected: boolean) => void
  addDaemon: (config: Omit<DaemonConfig, 'connected'>) => void
  removeDaemon: (machineId: string) => void
  /** Called by useDaemonConnections when a machine_id→sendFn mapping is ready */
  setSend: (fn: (machineId: string, msg: ClientMessage) => void) => void
  /** Called on peer_list — auto-registers unknown peers as daemon entries */
  mergeDiscoveredPeers: (peers: PeerInfo[]) => void
  /** Called when the real machine_id is learned — renames the temp URL-keyed entry */
  resolveDaemonId: (tempId: string, realMachineId: string) => void
  handleServerMessage: (raw: string) => void
  selectWorktree: (id: string | null) => void
  openPane: (worktreeId: string) => void
  closePane: (worktreeId: string) => void
  setTileMode: (mode: '1-up' | '2-up') => void
  switchToGrid: () => void
  switchToPanes: () => void
  openProjectTree: (projectId: string) => void
}
