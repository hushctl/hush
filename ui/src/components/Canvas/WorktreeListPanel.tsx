import { useStore } from '@/store'
import { statusColor } from '@/lib/status'
import { StatusPill } from '@/components/ProjectCard/StatusPill'

interface Props {
  projectId: string
}

export function WorktreeListPanel({ projectId }: Props) {
  const projects = useStore(s => s.projects)
  const worktrees = useStore(s => s.worktrees)
  const openPanel = useStore(s => s.openPanel)
  const send = useStore(s => s.send)

  const project = projects[projectId]
  const projectWorktrees = Object.values(worktrees).filter(w => w.project_id === projectId)

  function openTerminal(worktreeId: string) {
    openPanel({ kind: 'terminal', targetId: worktreeId })
  }

  function openShell(worktreeId: string) {
    openPanel({ kind: 'shell', targetId: worktreeId })
  }

  function openFileRail(worktreeId: string) {
    openPanel({ kind: 'file_rail', targetId: worktreeId })
  }

  if (!project) {
    return (
      <div className="flex items-center justify-center h-full text-xs text-muted-foreground">
        Project not found.
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="px-3 py-2 border-b border-border shrink-0">
        <span className="text-xs font-mono uppercase tracking-wider text-foreground">{project.name}</span>
        <span className="ml-2 text-xs font-mono text-muted-foreground">
          {projectWorktrees.length} {projectWorktrees.length === 1 ? 'worktree' : 'worktrees'}
        </span>
      </div>

      <div className="flex-1 overflow-y-auto py-2">
        {projectWorktrees.length === 0 && (
          <div className="px-3 py-4 text-xs text-muted-foreground">No worktrees yet.</div>
        )}
        {projectWorktrees.map(wt => (
          <div
            key={wt.id}
            className="flex items-start gap-2 px-3 py-2 hover:bg-muted/50 transition-colors"
          >
            <span
              className="shrink-0 mt-1 w-2 h-2 inline-block"
              style={{ backgroundColor: statusColor(wt.status) }}
            />
            <div className="min-w-0 flex-1">
              <div className="text-xs font-mono text-foreground truncate">{wt.branch}</div>
              <StatusPill status={wt.status} />
              {wt.last_task && (
                <div className="text-xs text-muted-foreground truncate mt-0.5">{wt.last_task}</div>
              )}
            </div>
            <div className="flex gap-1 shrink-0">
              <button
                className="text-xs font-mono text-muted-foreground hover:text-foreground px-1"
                title="Open Claude terminal"
                onClick={() => openTerminal(wt.id)}
              >
                ▶
              </button>
              <button
                className="text-xs font-mono text-muted-foreground hover:text-foreground px-1"
                title="Open shell"
                onClick={() => openShell(wt.id)}
              >
                $
              </button>
              <button
                className="text-xs font-mono text-muted-foreground hover:text-foreground px-1"
                title="Open file viewer"
                onClick={() => openFileRail(wt.id)}
              >
                ◫
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
