import { useStore } from '@/store'
import { statusColor } from '@/lib/status'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { LayoutGrid } from 'lucide-react'

export function TopBar() {
  const worktrees = useStore(s => s.worktrees)
  const projects = useStore(s => s.projects)
  const activePanes = useStore(s => s.activePanes)
  const openPane = useStore(s => s.openPane)
  const switchToGrid = useStore(s => s.switchToGrid)
  const tileMode = useStore(s => s.tileMode)
  const setTileMode = useStore(s => s.setTileMode)
  const daemons = useStore(s => s.daemons)
  const daemonList = Object.values(daemons)
  const connectedCount = daemonList.filter(d => d.connected).length
  const totalCount = daemonList.length
  const connected = connectedCount > 0

  return (
    <div data-testid="top-bar" className="flex items-center gap-2 px-3 h-10 border-b border-border bg-background shrink-0">
      {/* Back to grid */}
      <Button
        data-testid="grid-btn"
        variant="ghost"
        size="sm"
        className="rounded-none shadow-none font-normal h-7 text-xs px-2 gap-1"
        onClick={switchToGrid}
      >
        <LayoutGrid className="h-3 w-3" />
        grid
      </Button>

      <div className="w-px h-4 bg-border" />

      {/* All worktree dots */}
      <div className="flex items-center gap-3 overflow-x-auto flex-1">
        {Object.values(worktrees).map(wt => {
          const project = projects[wt.project_id]
          const isActive = activePanes.includes(wt.id)
          const color = statusColor(wt.status)
          return (
            <Tooltip key={wt.id}>
              <TooltipTrigger
                data-testid={`top-bar-wt-${wt.id}`}
                className="flex items-center gap-1.5 shrink-0 hover:opacity-70 transition-opacity"
                onClick={() => openPane(wt.id)}
              >
                <span
                  className="inline-block w-2 h-2"
                  style={{
                    backgroundColor: color,
                    outline: isActive ? `2px solid ${color}` : 'none',
                    outlineOffset: 2,
                  }}
                />
                <span className="text-xs font-mono text-muted-foreground">
                  {project?.name ?? wt.project_id} / {wt.branch}
                </span>
              </TooltipTrigger>
              <TooltipContent className="rounded-none text-xs font-mono">
                {wt.status} · {wt.working_dir}
              </TooltipContent>
            </Tooltip>
          )
        })}
      </div>

      <div className="flex items-center gap-1">
        {/* Tile mode toggle */}
        <button
          data-testid="tile-1"
          className={`px-2 py-1 text-xs font-mono border ${tileMode === '1-up' ? 'border-foreground' : 'border-border text-muted-foreground'} hover:border-foreground transition-colors`}
          onClick={() => setTileMode('1-up')}
          title="Single pane"
        >
          1
        </button>
        <button
          data-testid="tile-2"
          className={`px-2 py-1 text-xs font-mono border ${tileMode === '2-up' ? 'border-foreground' : 'border-border text-muted-foreground'} hover:border-foreground transition-colors`}
          onClick={() => setTileMode('2-up')}
          title="Two panes"
        >
          2
        </button>

        {/* Connection status */}
        <Tooltip>
          <TooltipTrigger
            data-testid="connection-status"
            data-connected={connected}
            className="ml-2 inline-flex items-center gap-1.5"
            style={{ background: 'none', border: 'none', padding: 0, cursor: 'default' }}
          >
            <span
              className="inline-block w-1.5 h-1.5"
              style={{ backgroundColor: connected ? '#22c55e' : '#ef4444' }}
            />
            <span className="text-xs font-mono text-muted-foreground">
              {connectedCount}/{totalCount}
            </span>
          </TooltipTrigger>
          <TooltipContent className="rounded-none text-xs font-mono">
            {connectedCount}/{totalCount} {totalCount === 1 ? 'daemon' : 'daemons'} connected
            {daemonList.map(d => (
              <div key={d.id} className="mt-0.5">
                <span style={{ color: d.connected ? '#22c55e' : '#ef4444' }}>■</span> {d.name}
              </div>
            ))}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}
