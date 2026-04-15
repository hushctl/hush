// PtyBus — module-level event emitter routing pty_data / pty_scrollback
// messages from the store dispatch into the right TerminalPane instance.
//
// Why not zustand state? xterm.Terminal is imperative — we want bytes to flow
// straight into terminal.write() without re-rendering React. The store would
// also need to grow per-worktree byte buffers that we never read from JS.

type Listener = (payload: PtyPayload) => void;

export interface PtyPayload {
  kind: "data" | "scrollback" | "exit";
  data?: Uint8Array;
  code?: number | null;
}

const listeners = new Map<string, Set<Listener>>();

export const ptyBus = {
  subscribe(worktreeId: string, fn: Listener): () => void {
    let set = listeners.get(worktreeId);
    if (!set) {
      set = new Set();
      listeners.set(worktreeId, set);
    }
    set.add(fn);
    return () => {
      set!.delete(fn);
      if (set!.size === 0) listeners.delete(worktreeId);
    };
  },

  emit(worktreeId: string, payload: PtyPayload): void {
    const set = listeners.get(worktreeId);
    if (!set) return;
    for (const fn of set) fn(payload);
  },
};

/** Decode a base64 string to a Uint8Array (browser-native, no Buffer). */
export function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ─── ANSI strip + last-line extraction ──────────────────────────────────────

const ANSI_RE = /\x1b\[[\?0-9;]*[a-zA-Z]|\x1b\].*?(?:\x07|\x1b\\)|\x1b[()][0-9A-B]|\r/g;

/** Strip ANSI escape sequences and carriage returns from terminal text. */
export function stripAnsi(s: string): string {
  return s.replace(ANSI_RE, "");
}

/**
 * Extract the last non-empty line from a chunk of terminal output.
 * Returns null if nothing meaningful is found.
 */
export function extractLastLine(data: Uint8Array): string | null {
  // Decode bytes to string (terminal output is typically UTF-8)
  const text = new TextDecoder().decode(data);
  const clean = stripAnsi(text);
  // Split on newlines, find the last non-empty line
  const lines = clean.split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (line.length > 0) return line.slice(0, 120);
  }
  return null;
}
