// PtyBus — module-level event emitter routing pty_data / pty_scrollback
// messages from the store dispatch into the right TerminalPane instance.
//
// Why not zustand state? xterm.Terminal is imperative — we want bytes to flow
// straight into terminal.write() without re-rendering React. The store would
// also need to grow per-worktree byte buffers that we never read from JS.

type Listener = (payload: PtyPayload) => void

export interface PtyPayload {
  kind: 'data' | 'scrollback' | 'exit'
  data?: Uint8Array
  code?: number | null
}

const listeners = new Map<string, Set<Listener>>()

export const ptyBus = {
  subscribe(worktreeId: string, fn: Listener): () => void {
    let set = listeners.get(worktreeId)
    if (!set) {
      set = new Set()
      listeners.set(worktreeId, set)
    }
    set.add(fn)
    return () => {
      set!.delete(fn)
      if (set!.size === 0) listeners.delete(worktreeId)
    }
  },

  emit(worktreeId: string, payload: PtyPayload): void {
    const set = listeners.get(worktreeId)
    if (!set) return
    for (const fn of set) fn(payload)
  },
}

/** Decode a base64 string to a Uint8Array (browser-native, no Buffer). */
export function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}
