import { useCallback, useRef } from 'react'

interface DragHandleProps {
  /** Called continuously with the new pixel delta since drag started */
  onDrag: (deltaX: number, deltaY: number) => void
  direction?: 'horizontal' | 'vertical'
  className?: string
}

/** Thin drag strip between resizable panels. */
export function ResizeHandle({ onDrag, direction = 'horizontal', className = '' }: DragHandleProps) {
  const startPos = useRef<{ x: number; y: number } | null>(null)

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      e.currentTarget.setPointerCapture(e.pointerId)
      startPos.current = { x: e.clientX, y: e.clientY }
    },
    []
  )

  const handlePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!e.buttons || !startPos.current) return
      const dx = e.clientX - startPos.current.x
      const dy = e.clientY - startPos.current.y
      startPos.current = { x: e.clientX, y: e.clientY }
      onDrag(dx, dy)
    },
    [onDrag]
  )

  const base =
    direction === 'horizontal'
      ? 'w-[3px] cursor-col-resize shrink-0 self-stretch'
      : 'h-[3px] cursor-row-resize shrink-0 self-stretch'

  return (
    <div
      className={`${base} bg-border hover:bg-muted-foreground/50 active:bg-muted-foreground transition-colors select-none ${className}`}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
    />
  )
}
