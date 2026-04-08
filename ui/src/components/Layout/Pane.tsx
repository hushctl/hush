import { useStore } from '@/store'
import { statusColor } from '@/lib/status'
import { StatusPill } from '@/components/ProjectCard/StatusPill'
import { TerminalPane } from '@/components/Terminal/TerminalPane'
import { Button } from '@/components/ui/button'
import { X } from 'lucide-react'

interface Props {
  worktreeId: string
}

export function Pane({ worktreeId }: Props) {
  const worktree = useStore(s => s.worktrees[worktreeId])
  const project = useStore(s => s.projects[s.worktrees[worktreeId]?.project_id ?? ''])
  const closePane = useStore(s => s.closePane)
  const color = statusColor(worktree?.status ?? 'idle')

  if (!worktree) {
    return (
      <div
        data-testid="pane"
        className="flex items-center justify-center h-full text-sm text-muted-foreground border border-border"
      >
        Worktree not found
      </div>
    )
  }

  return (
    <div data-testid="pane" data-worktree-id={worktreeId} className="flex flex-col h-full border border-border overflow-hidden">
      {/* Header */}
      <div
        data-testid="pane-header"
        className="flex items-center justify-between px-3 py-2 border-b border-border shrink-0"
        style={{ borderLeftColor: color, borderLeftWidth: 2 }}
      >
        <div className="flex items-center gap-2 min-w-0">
          <span className="inline-block w-2 h-2 shrink-0" style={{ backgroundColor: color }} />
          <span className="text-sm font-normal truncate">{project?.name ?? worktree.project_id}</span>
          <span className="text-xs font-mono text-muted-foreground">{worktree.branch}</span>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <StatusPill status={worktree.status} />
          <Button
            data-testid="close-pane"
            variant="ghost"
            size="icon"
            className="rounded-none shadow-none h-6 w-6"
            onClick={() => closePane(worktreeId)}
          >
            <X className="h-3 w-3" />
          </Button>
        </div>
      </div>

      {/* Embedded Claude Code terminal */}
      <div className="flex-1 overflow-hidden">
        <TerminalPane worktreeId={worktreeId} />
      </div>
    </div>
  )
}
