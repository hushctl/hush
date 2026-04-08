import { useState } from 'react'
import { useStore, splitKey } from '@/store'
import { TerminalPane } from '@/components/Terminal/TerminalPane'
import { StatusPill } from '@/components/ProjectCard/StatusPill'
import { statusColor } from '@/lib/status'

export function ProjectTree() {
  const selectedProjectId = useStore(s => s.selectedProjectId)
  const projects = useStore(s => s.projects)
  const worktrees = useStore(s => s.worktrees)
  const switchToGrid = useStore(s => s.switchToGrid)
  const send = useStore(s => s.send)

  const project = selectedProjectId ? projects[selectedProjectId] : null
  const projectWorktrees = Object.values(worktrees).filter(w => w.project_id === selectedProjectId)

  const [selectedWtId, setSelectedWtId] = useState<string | null>(projectWorktrees[0]?.id ?? null)
  const [showNewWorktree, setShowNewWorktree] = useState(false)
  const [newBranch, setNewBranch] = useState('')

  function createWorktree() {
    if (!selectedProjectId || !newBranch.trim()) return
    // selectedProjectId is namespaced — split to get machineId + raw project ID
    const [machineId, rawProjectId] = splitKey(selectedProjectId)
    send(machineId, { type: 'create_worktree', project_id: rawProjectId, branch: newBranch.trim(), permission_mode: 'plan' })
    setShowNewWorktree(false)
    setNewBranch('')
  }

  if (!project) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        No project selected.
      </div>
    )
  }

  return (
    <div data-testid="project-tree" className="flex flex-col h-full">
      {/* Header */}
      <div
        data-testid="tree-header"
        className="flex items-center gap-3 px-4 py-2 border-b border-border shrink-0 bg-background"
      >
        <button
          data-testid="grid-btn-tree"
          className="text-xs font-mono text-muted-foreground hover:text-foreground"
          onClick={switchToGrid}
        >
          ← grid
        </button>
        <span className="text-xs font-mono uppercase tracking-wider text-foreground">
          {project.name}
        </span>
        <span className="text-xs font-mono text-muted-foreground">
          {projectWorktrees.length} {projectWorktrees.length === 1 ? 'worktree' : 'worktrees'}
        </span>
      </div>

      {/* Split: tree left, chat right */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left: worktree tree */}
        <div
          data-testid="tree-panel"
          className="w-64 border-r border-border flex flex-col overflow-hidden shrink-0"
        >
          <div className="flex-1 overflow-y-auto py-2">
            <div className="relative">
              {/* Vertical connecting line */}
              {projectWorktrees.length > 1 && (
                <div
                  className="absolute bg-border"
                  style={{ left: 28, top: 20, bottom: 20, width: 1 }}
                />
              )}

              {projectWorktrees.map(wt => {
                const isSelected = selectedWtId === wt.id

                return (
                  <button
                    key={wt.id}
                    data-testid={`tree-node-${wt.id}`}
                    data-status={wt.status}
                    className={`relative flex items-start gap-3 w-full text-left px-4 py-2 transition-colors ${isSelected ? 'bg-muted' : 'hover:bg-muted/50'}`}
                    onClick={() => setSelectedWtId(wt.id)}
                  >
                    {/* Node dot */}
                    <span
                      className="shrink-0 mt-1 w-2.5 h-2.5 inline-block"
                      style={{ backgroundColor: statusColor(wt.status) }}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="text-xs font-mono text-foreground truncate">{wt.branch}</div>
                      <StatusPill status={wt.status} />
                      {wt.last_task && (
                        <div className="text-xs text-muted-foreground truncate mt-0.5">
                          {wt.last_task}
                        </div>
                      )}
                    </div>
                  </button>
                )
              })}

              {projectWorktrees.length === 0 && (
                <div className="px-4 py-4 text-xs text-muted-foreground">
                  No worktrees yet.
                </div>
              )}
            </div>
          </div>

          {/* + new worktree */}
          <div className="shrink-0 border-t border-border p-3">
            {showNewWorktree ? (
              <div className="flex flex-col gap-2">
                <input
                  data-testid="new-worktree-branch-input"
                  className="bg-background border border-border px-2 py-1 text-xs font-mono outline-none w-full"
                  placeholder="branch name"
                  value={newBranch}
                  onChange={e => setNewBranch(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === 'Enter') createWorktree()
                    if (e.key === 'Escape') setShowNewWorktree(false)
                  }}
                  autoFocus
                />
                <div className="flex gap-1">
                  <button
                    data-testid="new-worktree-create-btn"
                    className="text-xs font-mono border border-border px-2 py-1 hover:bg-muted"
                    onClick={createWorktree}
                  >
                    Create
                  </button>
                  <button
                    className="text-xs font-mono text-muted-foreground px-2 py-1 hover:bg-muted"
                    onClick={() => setShowNewWorktree(false)}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <button
                data-testid="new-worktree-btn"
                className="text-xs font-mono text-muted-foreground hover:text-foreground w-full text-left"
                onClick={() => setShowNewWorktree(true)}
              >
                + new worktree
              </button>
            )}
          </div>
        </div>

        {/* Right: embedded Claude Code terminal */}
        <div className="flex-1 overflow-hidden">
          {selectedWtId ? (
            <TerminalPane worktreeId={selectedWtId} />
          ) : (
            <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
              Select a worktree to view its terminal.
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
