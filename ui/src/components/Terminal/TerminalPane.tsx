import { useEffect, useRef } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { useStore, splitKey } from '@/store'
import { ptyBus, type PtyPayload } from '@/lib/ptyBus'

interface Props {
  /** Namespaced worktree ID: `${machineId}:${rawId}` */
  worktreeId: string
}

/**
 * Embedded Claude Code terminal — xterm.js renders a live pty stream from the
 * daemon. The component owns the xterm.Terminal instance imperatively; bytes
 * arrive via the PtyBus rather than React state to avoid per-byte re-renders.
 */
export function TerminalPane({ worktreeId }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<Terminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const send = useStore(s => s.send)

  // Split namespaced ID into machineId + rawId for daemon messages
  const [machineId, rawWorktreeId] = splitKey(worktreeId)

  useEffect(() => {
    if (!containerRef.current) return

    const term = new Terminal({
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      theme: {
        background: '#0a0a0a',
        foreground: '#e5e5e5',
      },
      allowProposedApi: true,
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(containerRef.current)
    fit.fit()
    termRef.current = term
    fitRef.current = fit

    const cols = term.cols
    const rows = term.rows

    send(machineId, { type: 'pty_attach', worktree_id: rawWorktreeId, cols, rows })

    // Intercept shift+Enter before xterm maps it to \r (same as plain Enter).
    // Claude Code's multi-line input distinguishes soft newlines via \n (0x0a).
    term.attachCustomKeyEventHandler(e => {
      if (e.type === 'keydown' && e.key === 'Enter' && e.shiftKey) {
        send(machineId, { type: 'pty_input', worktree_id: rawWorktreeId, data: '\n' })
        return false // prevent xterm's default \r handling
      }
      return true
    })

    // Forward keystrokes
    const dataDispose = term.onData(data => {
      send(machineId, { type: 'pty_input', worktree_id: rawWorktreeId, data })
    })

    // Subscribe to bytes from the daemon for this worktree
    const unsub = ptyBus.subscribe(worktreeId, (payload: PtyPayload) => {
      if (payload.kind === 'data' || payload.kind === 'scrollback') {
        if (payload.data) term.write(payload.data)
      } else if (payload.kind === 'exit') {
        term.write('\r\n\x1b[31m[session exited]\x1b[0m\r\n')
      }
    })

    // Resize on container resize
    const ro = new ResizeObserver(() => {
      try {
        fit.fit()
        if (termRef.current) {
          send(machineId, {
            type: 'pty_resize',
            worktree_id: rawWorktreeId,
            cols: termRef.current.cols,
            rows: termRef.current.rows,
          })
        }
      } catch {
        // Container not measurable yet — ignore.
      }
    })
    ro.observe(containerRef.current)

    return () => {
      ro.disconnect()
      unsub()
      dataDispose.dispose()
      send(machineId, { type: 'pty_detach', worktree_id: rawWorktreeId })
      term.dispose()
      termRef.current = null
      fitRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [worktreeId])

  return (
    <div
      ref={containerRef}
      data-testid={`terminal-pane-${worktreeId}`}
      className="w-full h-full"
      style={{ background: '#0a0a0a' }}
    />
  )
}
