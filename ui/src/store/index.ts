import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { AppState, DaemonConfig, Panel, PanelKind } from './types'
import type { ServerMessage, ProjectInfo, WorktreeInfo, ClientMessage, PeerInfo } from '@/lib/protocol'
import { ptyBus, decodeBase64 } from '@/lib/ptyBus'

const PANEL_DEFAULTS: Record<PanelKind, { width: number; height: number }> = {
  terminal: { width: 720, height: 480 },
  shell: { width: 600, height: 400 },
  file_rail: { width: 260, height: 520 },
  worktree_list: { width: 240, height: 400 },
}

/** Namespace a raw daemon ID to a globally-unique key */
export function nsKey(machineId: string, rawId: string): string {
  return `${machineId}:${rawId}`
}

/** Split a namespaced key back into [machineId, rawId] */
export function splitKey(key: string): [string, string] {
  const idx = key.indexOf(':')
  if (idx === -1) return ['', key]
  return [key.slice(0, idx), key.slice(idx + 1)]
}

/** Default localhost daemon — used when no daemons are persisted */
const LOCALHOST_DAEMON: DaemonConfig = {
  id: 'localhost',
  name: 'localhost',
  url: 'wss://localhost:9111/ws',
  connected: false,
}

export const useStore = create<AppState>()(
  persist(
    (set, get) => ({
      // ── Daemon registry ────────────────────────────────────────────────────
      daemons: { localhost: { ...LOCALHOST_DAEMON } },

      // ── Daemon data ────────────────────────────────────────────────────────
      projects: {},
      worktrees: {},

      // ── UI state ───────────────────────────────────────────────────────────
      layoutMode: 'grid',
      canvas: { panels: [], nextZ: 0 },
      canvasSize: { w: 0, h: 0 },
      activePanes: [],
      selectedWorktreeId: null,
      selectedProjectId: null,
      tileMode: '1-up',
      daemonError: null,
      pendingCreate: null,

      // ── File viewer state ──────────────────────────────────────────────────
      gitStatus: {},
      fileList: {},
      fileContents: {},
      cmdPOpen: false,
      cmdPTargetWorktree: null,

      // ── WebSocket send (injected by hook) ──────────────────────────────────
      send: (_machineId: string, _msg: ClientMessage) => {
        console.warn('WebSocket not connected yet')
      },

      // ── Actions ────────────────────────────────────────────────────────────
      clearDaemonError: () => set({ daemonError: null }),
      clearPendingCreate: () => set({ pendingCreate: null }),

      setDaemonConnected: (machineId, connected) =>
        set(state => {
          const daemon = state.daemons[machineId]
          if (!daemon) return {}
          return {
            daemons: {
              ...state.daemons,
              [machineId]: { ...daemon, connected },
            },
          }
        }),

      addDaemon: (config) =>
        set(state => ({
          daemons: {
            ...state.daemons,
            [config.id]: { ...config, connected: false },
          },
        })),

      removeDaemon: (machineId) =>
        set(state => {
          const { [machineId]: _removed, ...rest } = state.daemons
          // Also remove all projects/worktrees from that machine
          const projects = Object.fromEntries(
            Object.entries(state.projects).filter(([k]) => !k.startsWith(machineId + ':'))
          )
          const worktrees = Object.fromEntries(
            Object.entries(state.worktrees).filter(([k]) => !k.startsWith(machineId + ':'))
          )
          return { daemons: rest, projects, worktrees }
        }),

      setSend: (fn) => set({ send: fn }),

      mergeDiscoveredPeers: (peers: PeerInfo[]) => {
        set(state => {
          const updated = { ...state.daemons }
          const knownUrls = new Set(Object.values(updated).map(d => d.url))
          let changed = false
          for (const peer of peers) {
            if (!peer.url) continue
            if (peer.machine_id in updated) continue  // already known by machine_id
            if (knownUrls.has(peer.url)) continue     // already known by URL (temp entry)
            updated[peer.machine_id] = {
              id: peer.machine_id,
              name: peer.machine_id,
              url: peer.url,
              connected: false,
            }
            knownUrls.add(peer.url)
            changed = true
          }
          return changed ? { daemons: updated } : {}
        })
      },

      resolveDaemonId: (tempId: string, realMachineId: string) => {
        set(state => {
          if (tempId === realMachineId) return {}
          if (!(tempId in state.daemons)) return {}
          const existing = state.daemons[tempId]
          const updated = { ...state.daemons }
          delete updated[tempId]
          // If machine_id entry already exists (e.g. from gossip), just remove the temp entry
          if (!(realMachineId in updated)) {
            updated[realMachineId] = { ...existing, id: realMachineId }
          }
          return { daemons: updated }
        })
      },

      handleServerMessage: (raw: string) => {
        let msg: ServerMessage
        try { msg = JSON.parse(raw) as ServerMessage }
        catch { return }

        const mid = msg.machine_id

        switch (msg.type) {
          case 'project_list': {
            // Re-key projects under ${machineId}:${rawId}
            set(state => {
              const projects = { ...state.projects }
              // Remove stale entries for this machine
              for (const k of Object.keys(projects)) {
                if (k.startsWith(mid + ':')) delete projects[k]
              }
              for (const p of msg.projects) {
                const key = nsKey(mid, p.id)
                projects[key] = { ...p, id: key, machine_id: mid }
              }
              return { projects }
            })

            // If the daemon's machine_id differs from our placeholder entry
            // (i.e. we registered it as 'localhost' but the machine reports
            // a different id), update the registry key.
            set(state => {
              const daemons = { ...state.daemons }
              // Find a daemon entry whose url matches the sender and whose id
              // is a placeholder (doesn't equal mid yet).
              // We can't easily do this here without knowing the URL —
              // useDaemonConnections passes machineId as a param instead.
              // This case is handled in useDaemonConnections via onFirstMessage.
              return { daemons }
            })
            break
          }

          case 'worktree_list': {
            set(state => {
              const worktrees = { ...state.worktrees }
              for (const k of Object.keys(worktrees)) {
                if (k.startsWith(mid + ':')) delete worktrees[k]
              }
              for (const w of msg.worktrees) {
                const key = nsKey(mid, w.id)
                worktrees[key] = {
                  ...w,
                  id: key,
                  // Namespace project_id so lookups vs. projects store work
                  project_id: nsKey(mid, w.project_id),
                  machine_id: mid,
                }
              }
              return { worktrees }
            })
            break
          }

          case 'status_change': {
            const nsId = nsKey(mid, msg.worktree_id)
            set(state => {
              const wt = state.worktrees[nsId]
              if (!wt) return {}
              return {
                worktrees: {
                  ...state.worktrees,
                  [nsId]: { ...wt, status: msg.status as WorktreeInfo['status'] },
                }
              }
            })
            break
          }

          case 'session_ended':
            // status already updated via status_change
            break

          case 'pty_data': {
            const nsId = nsKey(mid, msg.worktree_id)
            ptyBus.emit(nsId, { kind: 'data', data: decodeBase64(msg.data) })
            break
          }

          case 'pty_scrollback': {
            const nsId = nsKey(mid, msg.worktree_id)
            ptyBus.emit(nsId, { kind: 'scrollback', data: decodeBase64(msg.data) })
            break
          }

          case 'pty_exit': {
            const nsId = nsKey(mid, msg.worktree_id)
            ptyBus.emit(nsId, { kind: 'exit', code: msg.code })
            break
          }

          case 'error': {
            console.error('[daemon error]', msg.message, msg.worktree_id)
            set({ daemonError: msg.message })
            break
          }

          case 'peer_list': {
            get().mergeDiscoveredPeers(msg.peers)
            break
          }

          case 'path_not_found': {
            set({ pendingCreate: { path: msg.path, name: msg.name, machineId: msg.machine_id } })
            break
          }

          case 'git_status': {
            const nsId = nsKey(msg.machine_id, msg.worktree_id)
            set(state => ({
              gitStatus: {
                ...state.gitStatus,
                [nsId]: { staged: msg.staged, modified: msg.modified, untracked: msg.untracked },
              },
            }))
            break
          }

          case 'file_list': {
            const nsId = nsKey(msg.machine_id, msg.worktree_id)
            set(state => ({
              fileList: { ...state.fileList, [nsId]: msg.files },
            }))
            break
          }

          case 'file_content': {
            const nsId = nsKey(msg.machine_id, msg.worktree_id)
            set(state => ({
              fileContents: {
                ...state.fileContents,
                [nsId]: { path: msg.path, content: msg.content, truncated: msg.truncated },
              },
            }))
            break
          }

          case 'shell_data': {
            const nsId = nsKey(mid, msg.worktree_id)
            ptyBus.emit(`shell:${nsId}`, { kind: 'data', data: decodeBase64(msg.data) })
            break
          }

          case 'shell_scrollback': {
            const nsId = nsKey(mid, msg.worktree_id)
            ptyBus.emit(`shell:${nsId}`, { kind: 'scrollback', data: decodeBase64(msg.data) })
            break
          }

          case 'shell_exit': {
            const nsId = nsKey(mid, msg.worktree_id)
            ptyBus.emit(`shell:${nsId}`, { kind: 'exit', code: msg.code })
            break
          }
        }
      },

      selectWorktree: (id) => set({ selectedWorktreeId: id }),

      openPanel: ({ kind, targetId }) => {
        const state = get()
        // Bring existing panel to front instead of duplicating
        const existing = state.canvas.panels.find(p => p.kind === kind && p.targetId === targetId)
        if (existing) {
          const nextZ = state.canvas.nextZ
          const panels = state.canvas.panels.map(p =>
            p.id === existing.id ? { ...p, z: nextZ } : p
          )
          set({
            canvas: { panels, nextZ: nextZ + 1 },
            layoutMode: 'canvas',
            selectedWorktreeId: kind === 'terminal' ? targetId : state.selectedWorktreeId,
          })
          return
        }
        const n = state.canvas.panels.length
        const { width, height } = PANEL_DEFAULTS[kind]
        const z = state.canvas.nextZ
        const { w: cw, h: ch } = state.canvasSize
        const pw = cw > 0 ? Math.min(width, cw) : width
        const ph = ch > 0 ? Math.min(height, ch) : height
        const rawX = Math.round((40 + 30 * (n % 10)) / 8) * 8
        const rawY = Math.round((40 + 30 * (n % 10)) / 8) * 8
        const newPanel: Panel = {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
          kind,
          targetId,
          x: cw > 0 ? Math.min(rawX, cw - pw) : rawX,
          y: ch > 0 ? Math.min(rawY, ch - ph) : rawY,
          width: pw,
          height: ph,
          z,
        }
        const newActivePanes = kind === 'terminal'
          ? [...state.activePanes.filter(id => id !== targetId), targetId]
          : state.activePanes
        set({
          canvas: { panels: [...state.canvas.panels, newPanel], nextZ: z + 1 },
          layoutMode: 'canvas',
          activePanes: newActivePanes,
          selectedWorktreeId: kind === 'terminal' ? targetId : state.selectedWorktreeId,
        })
      },

      closePanel: (id) => {
        const state = get()
        const panel = state.canvas.panels.find(p => p.id === id)
        const newPanels = state.canvas.panels.filter(p => p.id !== id)
        const newActivePanes = panel?.kind === 'terminal'
          ? state.activePanes.filter(wtId => wtId !== panel.targetId)
          : state.activePanes
        set({
          canvas: { ...state.canvas, panels: newPanels },
          activePanes: newActivePanes,
          layoutMode: newPanels.length === 0 ? 'grid' : state.layoutMode,
        })
      },

      movePanel: (id, x, y) => {
        const state = get()
        const panels = state.canvas.panels.map(p =>
          p.id === id ? { ...p, x: Math.max(0, x), y: Math.max(0, y) } : p
        )
        set({ canvas: { ...state.canvas, panels } })
      },

      resizePanel: (id, width, height) => {
        const state = get()
        const panels = state.canvas.panels.map(p =>
          p.id === id
            ? { ...p, width: Math.max(200, width), height: Math.max(120, height) }
            : p
        )
        set({ canvas: { ...state.canvas, panels } })
      },

      focusPanel: (id) => {
        const state = get()
        const nextZ = state.canvas.nextZ
        const panels = state.canvas.panels.map(p =>
          p.id === id ? { ...p, z: nextZ } : p
        )
        set({ canvas: { panels, nextZ: nextZ + 1 } })
      },

      arrangePanels: (canvasW, canvasH) => {
        const state = get()
        const panels = state.canvas.panels
        if (panels.length === 0) return

        const GAP = 8
        const NARROW_W = 240  // fixed width for file viewers, like an IDE sidebar

        const isNarrow = (kind: string) => kind === 'file_rail' || kind === 'worktree_list'

        const narrows = panels.filter(p => isNarrow(p.kind))
        const wides   = panels.filter(p => !isNarrow(p.kind))

        // Sort wides: group by targetId so same-worktree panels are adjacent
        wides.sort((a, b) => a.targetId.localeCompare(b.targetId))
        // Sort narrows: worktree_list before file_rail
        narrows.sort((a, b) => (a.kind === 'worktree_list' ? -1 : 1) - (b.kind === 'worktree_list' ? -1 : 1))

        const arranged: typeof panels = []

        // ── Narrow panels: fixed-width left column, each capped at half height ──
        if (narrows.length > 0) {
          const maxCellH = Math.floor(canvasH / 2)
          const cellH = Math.min(maxCellH, Math.floor((canvasH - GAP * (narrows.length + 1)) / narrows.length))
          narrows.forEach((p, i) => {
            arranged.push({
              ...p,
              x: GAP,
              y: GAP + i * (cellH + GAP),
              width: NARROW_W,
              height: cellH,
            })
          })
        }

        // ── Wide panels: fill remaining area in a grid ────────────────────────
        if (wides.length > 0) {
          const xOffset = narrows.length > 0 ? GAP + NARROW_W + GAP : 0
          const areaW = canvasW - xOffset
          const n = wides.length

          // Pick cols to make cells closest to 16:9
          let bestCols = 1, bestScore = Infinity
          for (let c = 1; c <= n; c++) {
            const r = Math.ceil(n / c)
            const aspect = (areaW - GAP * (c + 1)) / c / ((canvasH - GAP * (r + 1)) / r)
            const score = Math.abs(aspect - 16 / 9)
            if (score < bestScore) { bestScore = score; bestCols = c }
          }
          const cols = bestCols
          const rows = Math.ceil(n / cols)
          const cellW = Math.floor((areaW - GAP * (cols + 1)) / cols)
          const cellH = Math.floor((canvasH - GAP * (rows + 1)) / rows)

          wides.forEach((p, i) => {
            const col = i % cols
            const row = Math.floor(i / cols)
            arranged.push({
              ...p,
              x: xOffset + GAP + col * (cellW + GAP),
              y: GAP + row * (cellH + GAP),
              width: cellW,
              height: cellH,
            })
          })
        }

        set({ canvas: { ...state.canvas, panels: arranged } })
      },

      setCanvasSize: (w, h) => {
        if (w === 0 || h === 0) return
        // Clamp all persisted panel positions to fit within the actual canvas bounds.
        const panels = get().canvas.panels.map(p => {
          const clampedH = Math.min(p.height, h)
          const clampedW = Math.min(p.width, w)
          const clampedY = Math.min(p.y, h - clampedH)
          const clampedX = Math.min(p.x, w - clampedW)
          return { ...p, x: Math.max(0, clampedX), y: Math.max(0, clampedY), width: clampedW, height: clampedH }
        })
        set({ canvasSize: { w, h }, canvas: { ...get().canvas, panels } })
      },

      openPane: (worktreeId) => {
        get().openPanel({ kind: 'terminal', targetId: worktreeId })
      },

      closePane: (worktreeId) => {
        const state = get()
        const toClose = new Set(
          state.canvas.panels.filter(p => p.targetId === worktreeId).map(p => p.id)
        )
        const newPanels = state.canvas.panels.filter(p => !toClose.has(p.id))
        const newActivePanes = state.activePanes.filter(id => id !== worktreeId)
        set({
          canvas: { ...state.canvas, panels: newPanels },
          activePanes: newActivePanes,
          layoutMode: newPanels.length === 0 ? 'grid' : state.layoutMode,
          selectedWorktreeId: newActivePanes[newActivePanes.length - 1] ?? null,
        })
      },

      setTileMode: (mode) => set({ tileMode: mode }),

      switchToGrid: () => set({ layoutMode: 'grid' }),
      switchToCanvas: () => set({ layoutMode: 'canvas' }),
      switchToPanes: () => set({ layoutMode: 'canvas' }),

      openProjectTree: (projectId) => {
        set({ selectedProjectId: projectId })
        get().openPanel({ kind: 'worktree_list', targetId: projectId })
      },

      openFileContent: (worktreeId, path, content, truncated) =>
        set(state => ({
          fileContents: { ...state.fileContents, [worktreeId]: { path, content, truncated } },
        })),

      clearFileContent: (worktreeId) =>
        set(state => {
          const { [worktreeId]: _removed, ...rest } = state.fileContents
          return { fileContents: rest }
        }),

      openCmdP: (worktreeId) => set({ cmdPOpen: true, cmdPTargetWorktree: worktreeId }),

      closeCmdP: () => set({ cmdPOpen: false, cmdPTargetWorktree: null }),
    }),
    {
      name: 'mc-ui-prefs',
      version: 1,
      // v1: migrate ws:// → wss:// in persisted daemon URLs
      migrate: (persisted: unknown, fromVersion: number) => {
        const s = persisted as Record<string, unknown>
        if (fromVersion < 1 && s?.daemons && typeof s.daemons === 'object') {
          const daemons = s.daemons as Record<string, { url?: string }>
          for (const d of Object.values(daemons)) {
            if (typeof d.url === 'string' && d.url.startsWith('ws://')) {
              d.url = d.url.replace('ws://', 'wss://')
            }
          }
        }
        return s
      },
      // Persist layout prefs + daemon registry; data comes from daemon on every connect
      partialize: (state) => ({
        layoutMode: state.layoutMode,
        canvas: state.canvas,
        activePanes: state.activePanes,
        tileMode: state.tileMode,
        selectedWorktreeId: state.selectedWorktreeId,
        selectedProjectId: state.selectedProjectId,
        // Persist daemons but reset connection status
        daemons: Object.fromEntries(
          Object.entries(state.daemons).map(([k, v]) => [k, { ...v, connected: false }])
        ),
      }),
    }
  )
)
