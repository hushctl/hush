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
 * Plain shell terminal — runs $SHELL (bash/zsh) in the worktree's directory.
 * Separate from the Claude Code terminal so commands don't pollute the AI session.
 */
export function ShellPane({ worktreeId }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<Terminal | null>(null)
  const send = useStore(s => s.send)

  const [machineId, rawWorktreeId] = splitKey(worktreeId)
  // ptyBus channel for shell events is prefixed with 'shell:'
  const busChannel = `shell:${worktreeId}`

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

    const cols = term.cols
    const rows = term.rows

    send(machineId, { type: 'shell_attach', worktree_id: rawWorktreeId, cols, rows })

    const dataDispose = term.onData(data => {
      send(machineId, { type: 'shell_input', worktree_id: rawWorktreeId, data })
    })

    const unsub = ptyBus.subscribe(busChannel, (payload: PtyPayload) => {
      if (payload.kind === 'data' || payload.kind === 'scrollback') {
        if (payload.data) term.write(payload.data)
      } else if (payload.kind === 'exit') {
        term.write('\r\n\x1b[31m[shell exited]\x1b[0m\r\n')
      }
    })

    const ro = new ResizeObserver(() => {
      try {
        fit.fit()
        if (termRef.current) {
          send(machineId, {
            type: 'shell_resize',
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
      term.dispose()
      termRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [worktreeId])

  return (
    <div
      ref={containerRef}
      data-testid={`shell-pane-${worktreeId}`}
      className="w-full h-full"
      style={{ background: '#0a0a0a' }}
    />
  )
}
