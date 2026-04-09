import type { ProjectInfo, WorktreeInfo, ClientMessage, PeerInfo } from '@/lib/protocol'

export type PanelKind = 'terminal' | 'shell' | 'file_rail' | 'worktree_list'

export interface Panel {
  id: string
  kind: PanelKind
  /** Namespaced worktree id for terminal/file_rail; namespaced project id for worktree_list */
  targetId: string
  x: number
  y: number
  width: number
  height: number
  /** Stacking order — higher = on top */
  z: number
}

export interface CanvasState {
  panels: Panel[]
  nextZ: number
}

export interface GitFileStatus {
  staged: string[]
  modified: string[]
  untracked: string[]
}

export interface FileContent {
  path: string
  content: string
  truncated: boolean
}

export interface DaemonConfig {
  /** Stable machine identity (machine_id from the daemon) */
  id: string
  /** Human-readable label shown in the UI */
  name: string
  /** WebSocket URL, e.g. wss://localhost:9111/ws */
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
  layoutMode: 'grid' | 'canvas'
  canvas: CanvasState
  /** Recency list of open terminal worktree IDs — used by cmd+P and status tracking */
  activePanes: string[]
  selectedWorktreeId: string | null
  selectedProjectId: string | null
  tileMode: '1-up' | '2-up'

  // Transient error message from daemon (cleared after display)
  daemonError: string | null
  // Set when the daemon reports the project path doesn't exist — triggers the "create dir?" prompt
  pendingCreate: { path: string; name: string; machineId: string } | null

  // ── File viewer state ─────────────────────────────────────────────────────
  /** Live git status per namespaced worktree id */
  gitStatus: Record<string, GitFileStatus>
  /** All non-gitignored files per worktree id (for cmd+P) */
  fileList: Record<string, string[]>
  /** Currently open file content per worktree id */
  fileContents: Record<string, FileContent>
  /** Whether the cmd+P quick-open modal is visible */
  cmdPOpen: boolean
  /** Which worktree the cmd+P modal targets */
  cmdPTargetWorktree: string | null

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
  openPanel: (p: { kind: PanelKind; targetId: string }) => void
  closePanel: (id: string) => void
  movePanel: (id: string, x: number, y: number) => void
  resizePanel: (id: string, width: number, height: number) => void
  focusPanel: (id: string) => void
  arrangePanels: (canvasW: number, canvasH: number) => void
  openPane: (worktreeId: string) => void
  closePane: (worktreeId: string) => void
  setTileMode: (mode: '1-up' | '2-up') => void
  switchToGrid: () => void
  switchToCanvas: () => void
  switchToPanes: () => void
  openProjectTree: (projectId: string) => void
  /** Store a file's content in the viewer slot for a worktree */
  openFileContent: (worktreeId: string, path: string, content: string, truncated: boolean) => void
  /** Clear the file viewer for a worktree (return to changed-files list) */
  clearFileContent: (worktreeId: string) => void
  /** Open the cmd+P modal targeting a specific worktree */
  openCmdP: (worktreeId: string) => void
  /** Close the cmd+P modal */
  closeCmdP: () => void
}
