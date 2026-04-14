import type { ProjectInfo, WorktreeInfo, ClientMessage, PeerInfo } from '@/lib/protocol'
import type { ModelStatus } from '@/lib/gemma/bridge'

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
  /** When true, opening/closing panels auto-reflows the layout. Flipped off by any manual move/resize. */
  autoTidy: boolean
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

export interface TransferState {
  transferId: string
  phase: 'killing_pty' | 'archiving' | 'archiving_history' | 'dialing' | 'offering' | 'awaiting_ack' | 'streaming' | 'streaming_history' | 'awaiting_commit' | 'extracting' | 'installing_history' | 'spawning_pty' | 'complete' | 'failed'
  bytesSent: number
  totalBytes: number
  sourceMachineId: string
  destMachineId: string
  /** Namespaced worktree key at the source — used to find the source dot in the grid */
  sourceWorktreeKey: string
  projectName: string
  branch: string
  errorMessage?: string
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
  /** Measured size of the canvas container (excludes TopBar + CommandBar). Not persisted. */
  canvasSize: { w: number; h: number }
  /** Recency list of open terminal worktree IDs — used by cmd+P and status tracking */
  activePanes: string[]
  selectedWorktreeId: string | null
  selectedProjectId: string | null
  tileMode: '1-up' | '2-up'

  // Transient error message from daemon (cleared after display)
  daemonError: string | null
  // Set when the daemon reports the project path doesn't exist — triggers the "create dir?" prompt
  pendingCreate: { path: string; name: string; machineId: string } | null
  // Per-machine memory pressure alerts — keyed by machine_id, cleared on level: "normal"
  memoryAlerts: Record<string, { level: 'warning' | 'critical'; availableBytes: number; totalBytes: number }>
  // Daemon detail panel — which daemon is currently shown (null = closed)
  selectedDaemonId: string | null
  // Per-machine memory sample ring (cap 30) — fed by memory_pressure messages, used by sparkline
  memorySamples: Record<string, Array<{ t: number; ratio: number }>>
  // Gemma 4 model status — not persisted
  modelStatus: ModelStatus
  modelProgress: number   // 0–100
  modelProgressFile: string

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

  // ── Active transfers (keyed by transfer_id) ───────────────────────────────
  /** Outbound transfers in progress from this browser's perspective */
  transfers: Record<string, TransferState>

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
  setAutoTidy: (enabled: boolean) => void
  setCanvasSize: (w: number, h: number) => void
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
  /** Open the daemon detail panel for the given machine_id */
  openDaemonDetail: (machineId: string) => void
  /** Close the daemon detail panel */
  closeDaemonDetail: () => void
  /** Update Gemma model load status */
  setModelStatus: (status: ModelStatus) => void
  /** Update Gemma model download progress */
  setModelProgress: (progress: number, file: string) => void
  /** Manually dismiss a transfer card (used for failed transfers) */
  dismissTransfer: (transferId: string) => void
}

export type { ModelStatus }
