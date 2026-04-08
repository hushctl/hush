import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { AppState, DaemonConfig } from './types'
import type { ServerMessage, ProjectInfo, WorktreeInfo, ClientMessage, PeerInfo } from '@/lib/protocol'
import { ptyBus, decodeBase64 } from '@/lib/ptyBus'

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
  url: 'ws://localhost:9111/ws',
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
      activePanes: [],
      selectedWorktreeId: null,
      selectedProjectId: null,
      tileMode: '1-up',
      daemonError: null,
      pendingCreate: null,

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
        }
      },

      selectWorktree: (id) => set({ selectedWorktreeId: id }),

      openPane: (worktreeId) => {
        const { activePanes, tileMode } = get()
        const maxPanes = tileMode === '2-up' ? 2 : 1
        if (activePanes.includes(worktreeId)) {
          set({ layoutMode: 'panes', selectedWorktreeId: worktreeId })
          return
        }
        const newPanes = [...activePanes, worktreeId].slice(-maxPanes)
        set({ activePanes: newPanes, layoutMode: 'panes', selectedWorktreeId: worktreeId })
      },

      closePane: (worktreeId) => {
        const { activePanes } = get()
        const newPanes = activePanes.filter(id => id !== worktreeId)
        set({
          activePanes: newPanes,
          layoutMode: newPanes.length === 0 ? 'grid' : 'panes',
          selectedWorktreeId: newPanes[newPanes.length - 1] ?? null,
        })
      },

      setTileMode: (mode) => {
        const { activePanes } = get()
        const maxPanes = mode === '2-up' ? 2 : 1
        set({ tileMode: mode, activePanes: activePanes.slice(-maxPanes) })
      },

      switchToGrid: () => set({ layoutMode: 'grid' }),
      switchToPanes: () => set({ layoutMode: 'panes' }),

      openProjectTree: (projectId) => set({ layoutMode: 'tree', selectedProjectId: projectId }),
    }),
    {
      name: 'mc-ui-prefs',
      // Persist layout prefs + daemon registry; data comes from daemon on every connect
      partialize: (state) => ({
        layoutMode: state.layoutMode,
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
