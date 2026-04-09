import { useStore } from '@/store'
import { statusColor } from '@/lib/status'
import { StatusPill } from './StatusPill'
import { Button } from '@/components/ui/button'
import type { WorktreeInfo } from '@/lib/protocol'

interface Props {
  worktree: WorktreeInfo
  compact?: boolean
  onOpen?: () => void
}

export function ProjectCard({ worktree, compact, onOpen }: Props) {
  const project = useStore(s => s.projects[worktree.project_id])
  const send = useStore(s => s.send)
  const openDaemonDetail = useStore(s => s.openDaemonDetail)
  const borderColor = statusColor(worktree.status)
  const isNeedsYou = worktree.status === 'needs_you'
  const isFailed = worktree.status.startsWith('failed')

  const borderClass = isNeedsYou
    ? 'border-amber-400'
    : isFailed
      ? 'border-red-400'
      : 'border-border'

  if (compact) {
    return (
      <div
        data-testid="project-card"
        data-status={worktree.status}
        className={`flex items-center gap-2 px-2 py-1.5 border ${borderClass} cursor-pointer hover:bg-muted transition-colors`}
        onClick={onOpen}
        style={{ borderLeftColor: borderColor, borderLeftWidth: 2 }}
      >
        <span className="inline-block w-2 h-2 shrink-0" style={{ backgroundColor: borderColor }} />
        <span className="text-xs font-mono truncate flex-1">{project?.name ?? worktree.project_id}</span>
        <span className="text-xs text-muted-foreground font-mono">{worktree.branch}</span>
        <StatusPill status={worktree.status} />
      </div>
    )
  }

  return (
    <div
      data-testid="project-card"
      data-status={worktree.status}
      className={`border ${borderClass} bg-card`}
      style={{ borderLeftColor: borderColor, borderLeftWidth: 2 }}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border">
        <div className="flex items-center gap-2 min-w-0">
          <span className="inline-block w-2 h-2 shrink-0" style={{ backgroundColor: borderColor }} />
          <span className="text-sm font-normal truncate">{project?.name ?? worktree.project_id}</span>
          <span className="text-xs font-mono text-muted-foreground truncate">{worktree.branch}</span>
        </div>
        <StatusPill status={worktree.status} />
      </div>

      {/* Body */}
      <div className="px-3 py-2 space-y-1">
        <div className="text-xs text-muted-foreground uppercase tracking-wide font-mono">
          {worktree.status === 'running' ? 'current task' :
           isNeedsYou ? 'waiting for approval' :
           isFailed ? 'error' : 'last session'}
        </div>
        {worktree.last_task && (
          <div className="text-sm font-normal truncate">{worktree.last_task}</div>
        )}
        <div className="text-xs text-muted-foreground font-mono truncate">{worktree.working_dir}</div>
        {worktree.machine_id && (
          <button
            className="text-xs font-mono border border-border text-muted-foreground px-1.5 py-0.5 hover:border-foreground hover:text-foreground transition-colors self-start"
            onClick={e => { e.stopPropagation(); openDaemonDetail(worktree.machine_id) }}
          >
            {worktree.machine_id}
          </button>
        )}
      </div>

      {/* Actions */}
      <div className="flex items-center gap-2 px-3 py-2 border-t border-border">
        <Button
          data-testid="open-chat-btn"
          variant="outline"
          size="sm"
          className="rounded-none shadow-none font-normal h-7 text-xs"
          onClick={onOpen}
        >
          Open chat
        </Button>
        {isNeedsYou && (
          <Button
            data-testid="approve-btn"
            variant="outline"
            size="sm"
            className="rounded-none shadow-none font-normal h-7 text-xs border-amber-400 text-amber-600 hover:bg-amber-50"
            onClick={() => send({ type: 'pty_input', worktree_id: worktree.id, data: 'yes, proceed\r' })}
          >
            Approve
          </Button>
        )}
        {isFailed && (
          <Button
            data-testid="retry-btn"
            variant="outline"
            size="sm"
            className="rounded-none shadow-none font-normal h-7 text-xs border-red-400 text-red-600 hover:bg-red-50"
            onClick={() => send({ type: 'pty_input', worktree_id: worktree.id, data: 'please retry\r' })}
          >
            Retry
          </Button>
        )}
      </div>
    </div>
  )
}
