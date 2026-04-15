import { useRef, useCallback, useState } from "react";
import { useStore, splitKey } from "@/store";
import { statusColor } from "@/lib/status";
import { X, RotateCw } from "lucide-react";
import { ShellFooter } from "@/components/Terminal/ShellFooter";
import { TerminalPane } from "@/components/Terminal/TerminalPane";
import { ShellPane } from "@/components/Terminal/ShellPane";
import { FileRail } from "@/components/FileViewer/FileRail";
import { WorktreeListPanel } from "./WorktreeListPanel";
import type { Panel } from "@/store/types";

interface Props {
  panel: Panel;
}

export function PanelFrame({ panel }: Props) {
  const worktrees = useStore((s) => s.worktrees);
  const projects = useStore((s) => s.projects);
  const movePanel = useStore((s) => s.movePanel);
  const resizePanel = useStore((s) => s.resizePanel);
  const focusPanel = useStore((s) => s.focusPanel);
  const closePanel = useStore((s) => s.closePanel);
  const openPanel = useStore((s) => s.openPanel);
  const send = useStore((s) => s.send);

  // Restart epoch — incrementing forces TerminalPane to remount (fresh pty_attach)
  const [restartEpoch, setRestartEpoch] = useState(0);

  // Derive title and status dot
  const wt = panel.kind !== "worktree_list" ? worktrees[panel.targetId] : null;
  const proj = wt
    ? projects[wt.project_id]
    : panel.kind === "worktree_list"
      ? projects[panel.targetId]
      : null;
  const color = wt ? statusColor(wt.status) : null;

  const kindLabel =
    panel.kind === "terminal"
      ? "terminal"
      : panel.kind === "shell"
        ? "shell"
        : panel.kind === "file_rail"
          ? "files"
          : "worktrees";
  const title = wt
    ? `${proj?.name ?? wt.project_id} / ${wt.branch} · ${kindLabel}`
    : `${proj?.name ?? panel.targetId} · ${kindLabel}`;

  // Suppress CSS transition during drag/resize so the panel tracks the cursor 1:1.
  const [interacting, setInteracting] = useState(false);

  // ── Drag (header) ──────────────────────────────────────────────────────────
  const dragStart = useRef<{
    x: number;
    y: number;
    px: number;
    py: number;
  } | null>(null);

  const onHeaderPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if ((e.target as HTMLElement).closest("button")) return; // let close button work
      e.currentTarget.setPointerCapture(e.pointerId);
      dragStart.current = {
        x: e.clientX,
        y: e.clientY,
        px: panel.x,
        py: panel.y,
      };
      setInteracting(true);
      focusPanel(panel.id);
    },
    [panel.x, panel.y, panel.id, focusPanel],
  );

  const onHeaderPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!e.buttons || !dragStart.current) return;
      const dx = e.clientX - dragStart.current.x;
      const dy = e.clientY - dragStart.current.y;
      movePanel(panel.id, dragStart.current.px + dx, dragStart.current.py + dy);
    },
    [panel.id, movePanel],
  );

  const onHeaderPointerUp = useCallback(() => {
    // No-op if the pointerup came from a header button (close/$/◫) — dragStart is only set
    // in pointerdown when the target isn't a button. Without this guard, clicking close
    // would call movePanel and flip autoTidy off.
    if (!dragStart.current) return;
    const snapped = {
      x: Math.round(panel.x / 8) * 8,
      y: Math.round(panel.y / 8) * 8,
    };
    movePanel(panel.id, snapped.x, snapped.y);
    dragStart.current = null;
    setInteracting(false);
  }, [panel.id, panel.x, panel.y, movePanel]);

  // ── Resize (edge/corner handles) ──────────────────────────────────────────
  type Edge = "n" | "s" | "e" | "w" | "nw" | "ne" | "sw" | "se";

  const resizeStart = useRef<{
    x: number;
    y: number;
    px: number;
    py: number;
    pw: number;
    ph: number;
    edge: Edge;
  } | null>(null);

  const onResizePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, edge: Edge) => {
      e.stopPropagation();
      e.currentTarget.setPointerCapture(e.pointerId);
      resizeStart.current = {
        x: e.clientX,
        y: e.clientY,
        px: panel.x,
        py: panel.y,
        pw: panel.width,
        ph: panel.height,
        edge,
      };
      setInteracting(true);
      focusPanel(panel.id);
    },
    [panel.x, panel.y, panel.width, panel.height, panel.id, focusPanel],
  );

  const onResizePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!e.buttons || !resizeStart.current) return;
      const { x, y, px, py, pw, ph, edge } = resizeStart.current;
      const dx = e.clientX - x;
      const dy = e.clientY - y;

      let nx = px,
        ny = py,
        nw = pw,
        nh = ph;

      if (edge.includes("e")) nw = Math.max(200, pw + dx);
      if (edge.includes("s")) nh = Math.max(120, ph + dy);
      if (edge.includes("w")) {
        nw = Math.max(200, pw - dx);
        nx = px + pw - nw;
      }
      if (edge.includes("n")) {
        nh = Math.max(120, ph - dy);
        ny = py + ph - nh;
      }

      movePanel(panel.id, Math.max(0, nx), Math.max(0, ny));
      resizePanel(panel.id, nw, nh);
    },
    [panel.id, movePanel, resizePanel],
  );

  const onResizePointerUp = useCallback(() => {
    if (!resizeStart.current) return;
    const sw = Math.round(panel.width / 8) * 8;
    const sh = Math.round(panel.height / 8) * 8;
    resizePanel(panel.id, sw, sh);
    resizeStart.current = null;
    setInteracting(false);
  }, [panel.id, panel.width, panel.height, resizePanel]);

  // ── Resize handle factory ─────────────────────────────────────────────────
  function makeHandle(edge: Edge, style: React.CSSProperties) {
    const cursors: Record<Edge, string> = {
      n: "ns-resize",
      s: "ns-resize",
      e: "ew-resize",
      w: "ew-resize",
      nw: "nwse-resize",
      se: "nwse-resize",
      ne: "nesw-resize",
      sw: "nesw-resize",
    };
    return (
      <div
        key={edge}
        style={{
          ...style,
          position: "absolute",
          cursor: cursors[edge],
          zIndex: 10,
        }}
        className="select-none"
        onPointerDown={(e) => onResizePointerDown(e, edge)}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
      />
    );
  }

  const EDGE = 4; // px width/height of edge handles
  const CORNER = 8; // px corner size

  return (
    <div
      style={{
        position: "absolute",
        left: panel.x,
        top: panel.y,
        width: panel.width,
        height: panel.height,
        zIndex: panel.z,
        transition: interacting
          ? "none"
          : "left 150ms ease, top 150ms ease, width 150ms ease, height 150ms ease",
      }}
      className="flex flex-col border border-border bg-background overflow-hidden"
      onPointerDown={() => focusPanel(panel.id)}
    >
      {/* Header — drag handle */}
      <div
        className="flex items-center justify-between px-2 shrink-0 h-7 border-b border-border select-none cursor-grab active:cursor-grabbing"
        style={
          color ? { borderLeftColor: color, borderLeftWidth: 2 } : undefined
        }
        onPointerDown={onHeaderPointerDown}
        onPointerMove={onHeaderPointerMove}
        onPointerUp={onHeaderPointerUp}
      >
        <div className="flex items-center gap-1.5 min-w-0">
          {color && (
            <span
              className="inline-block w-1.5 h-1.5 shrink-0"
              style={{ backgroundColor: color }}
            />
          )}
          <span className="text-xs font-mono text-muted-foreground truncate">
            {title}
          </span>
        </div>
        <div className="flex items-center gap-1 shrink-0">
          {/* Quick-open companion panels from terminal header */}
          {panel.kind === "terminal" && (
            <>
              <button
                className="px-1 text-xs font-mono text-muted-foreground hover:text-foreground"
                title="Open shell"
                onClick={() =>
                  openPanel({ kind: "shell", targetId: panel.targetId })
                }
              >
                $
              </button>
              <button
                className="px-1 text-xs font-mono text-muted-foreground hover:text-foreground"
                title="Open file viewer"
                onClick={() =>
                  openPanel({ kind: "file_rail", targetId: panel.targetId })
                }
              >
                ◫
              </button>
              <button
                className="w-4 h-4 flex items-center justify-center text-muted-foreground hover:text-foreground"
                title="Restart Claude Code session"
                onClick={() => {
                  const [mid, rawId] = splitKey(panel.targetId);
                  send(mid, { type: "pty_kill", worktree_id: rawId });
                  setRestartEpoch((e) => e + 1);
                }}
              >
                <RotateCw className="w-2.5 h-2.5" />
              </button>
            </>
          )}
          <button
            className="w-4 h-4 flex items-center justify-center text-muted-foreground hover:text-foreground"
            onClick={() => closePanel(panel.id)}
          >
            <X className="w-2.5 h-2.5" />
          </button>
        </div>
      </div>

      {/* Body — position:relative so absolute-inset terminal fills it correctly */}
      <div className="flex-1 overflow-hidden min-h-0 relative">
        {panel.kind === "terminal" && (
          <TerminalPane key={restartEpoch} worktreeId={panel.targetId} />
        )}
        {panel.kind === "shell" && (
          <ShellPane worktreeId={panel.targetId} shellId={panel.shellId} />
        )}
        {panel.kind === "file_rail" && (
          <FileRail worktreeId={panel.targetId} className="h-full" />
        )}
        {panel.kind === "worktree_list" && (
          <WorktreeListPanel projectId={panel.targetId} />
        )}
      </div>

      {/* Shell status footer — stacked list of all shells for this worktree */}
      {panel.kind === "terminal" && (
        <ShellFooter
          worktreeId={panel.targetId}
          onOpenShell={(shellId) =>
            openPanel({ kind: "shell", targetId: panel.targetId, shellId })
          }
          onNewShell={() =>
            openPanel({ kind: "shell", targetId: panel.targetId })
          }
        />
      )}

      {/* Edge resize handles */}
      {makeHandle("n", { top: 0, left: CORNER, right: CORNER, height: EDGE })}
      {makeHandle("s", {
        bottom: 0,
        left: CORNER,
        right: CORNER,
        height: EDGE,
      })}
      {makeHandle("e", { right: 0, top: CORNER, bottom: CORNER, width: EDGE })}
      {makeHandle("w", { left: 0, top: CORNER, bottom: CORNER, width: EDGE })}
      {/* Corner resize handles */}
      {makeHandle("nw", { top: 0, left: 0, width: CORNER, height: CORNER })}
      {makeHandle("ne", { top: 0, right: 0, width: CORNER, height: CORNER })}
      {makeHandle("sw", { bottom: 0, left: 0, width: CORNER, height: CORNER })}
      {makeHandle("se", { bottom: 0, right: 0, width: CORNER, height: CORNER })}
    </div>
  );
}
