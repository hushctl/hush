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
  const openPanel = useStore(s => s.openPanel)
  const arrangePanels = useStore(s => s.arrangePanels)
  const panels = useStore(s => s.canvas.panels)
  const autoTidy = useStore(s => s.canvas.autoTidy)
  const setAutoTidy = useStore(s => s.setAutoTidy)
  const switchToGrid = useStore(s => s.switchToGrid)

  function handleTidy() {
    const canvasEl = document.querySelector('[data-canvas-area]')
    const w = canvasEl ? canvasEl.clientWidth : window.innerWidth
    const h = canvasEl ? canvasEl.clientHeight : window.innerHeight - 40
    arrangePanels(w, h)
    // Re-enable auto-tidy: a manual tidy signals the user wants layout managed again.
    if (!autoTidy) setAutoTidy(true)
  }
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

      {panels.length > 0 && (
        <>
          <Button
            variant="ghost"
            size="sm"
            className="rounded-none shadow-none font-normal h-7 text-xs px-2"
            onClick={handleTidy}
            title="Arrange panels into a tidy grid"
          >
            tidy
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="rounded-none shadow-none font-normal h-7 text-xs px-2 gap-1"
            onClick={() => setAutoTidy(!autoTidy)}
            title={autoTidy ? 'Auto-tidy on — click to disable' : 'Auto-tidy off — click to enable'}
          >
            <span
              className="inline-block w-1.5 h-1.5"
              style={{ backgroundColor: autoTidy ? '#22c55e' : '#6b7280' }}
            />
            auto
          </Button>
        </>
      )}

      <div className="w-px h-4 bg-border" />

      {/* All worktree dots */}
      <div className="flex items-center gap-3 overflow-x-auto flex-1">
        {Object.values(worktrees).map(wt => {
          const project = projects[wt.project_id]
          const isActive = activePanes.includes(wt.id)
          const color = statusColor(wt.status)
          return (
            <Tooltip key={wt.id}>
              <div className="flex items-center gap-1 shrink-0 group">
                <TooltipTrigger
                  data-testid={`top-bar-wt-${wt.id}`}
                  className="flex items-center gap-1.5 hover:opacity-70 transition-opacity"
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
                <button
                  className="text-xs font-mono text-muted-foreground hover:text-foreground opacity-0 group-hover:opacity-100 transition-opacity px-0.5"
                  title="Open shell"
                  onClick={() => openPanel({ kind: 'shell', targetId: wt.id })}
                >
                  $
                </button>
              </div>
              <TooltipContent className="rounded-none text-xs font-mono">
                {wt.status} · {wt.working_dir}
              </TooltipContent>
            </Tooltip>
          )
        })}
      </div>

      <div className="flex items-center gap-1">
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
