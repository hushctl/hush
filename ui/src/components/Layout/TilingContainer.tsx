import { useStore } from '@/store'
import { TopBar } from './TopBar'
import { Pane } from './Pane'

export function TilingContainer() {
  const activePanes = useStore(s => s.activePanes)
  const tileMode = useStore(s => s.tileMode)

  const gridCols = tileMode === '2-up' && activePanes.length === 2
    ? 'grid-cols-2'
    : 'grid-cols-1'

  return (
    <div className="flex flex-col h-full">
      <TopBar />
      <div className={`flex-1 grid ${gridCols} overflow-hidden`} style={{ gap: '1px', background: 'hsl(var(--border))' }}>
        {activePanes.map(id => (
          <div key={id} className="bg-background overflow-hidden">
            <Pane worktreeId={id} />
          </div>
        ))}
        {activePanes.length === 0 && (
          <div className="bg-background flex items-center justify-center text-sm text-muted-foreground">
            No panes open. Click a dot on the grid to open a chat.
          </div>
        )}
      </div>
    </div>
  )
}
