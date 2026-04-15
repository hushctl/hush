import { useRef, useEffect } from "react";
import { useStore } from "@/store";
import { PanelFrame } from "./PanelFrame";
import { CanvasConnectors } from "./CanvasConnectors";

export function Canvas() {
  const panels = useStore((s) => s.canvas.panels);
  const setCanvasSize = useStore((s) => s.setCanvasSize);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const { width, height } = entries[0].contentRect;
      setCanvasSize(Math.floor(width), Math.floor(height));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [setCanvasSize]);

  return (
    <div
      ref={containerRef}
      className="absolute inset-0 overflow-hidden bg-background"
    >
      {/* Connectors sit below panels */}
      <CanvasConnectors />
      {panels.map((panel) => (
        <PanelFrame key={panel.id} panel={panel} />
      ))}
      {panels.length === 0 && (
        <div className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
          No panels open. Click a worktree dot to open a terminal.
        </div>
      )}
    </div>
  );
}
