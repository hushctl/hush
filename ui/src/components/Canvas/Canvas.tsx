import { useRef } from 'react'
import { useStore } from '@/store'
import { PanelFrame } from './PanelFrame'

export function Canvas() {
  const panels = useStore(s => s.canvas.panels)
  const containerRef = useRef<HTMLDivElement>(null)

  return (
    <div ref={containerRef} className="absolute inset-0 overflow-hidden bg-background">
      {panels.map(panel => (
        <PanelFrame key={panel.id} panel={panel} />
      ))}
      {panels.length === 0 && (
        <div className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
          No panels open. Click a worktree dot to open a terminal.
        </div>
      )}
    </div>
  )
}

