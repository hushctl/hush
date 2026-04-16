import { useMemo } from "react";
import { useStore } from "@/store";
import type { WorktreePosition } from "@/lib/gridLayout";

interface Props {
  positions: WorktreePosition[];
  size: { w: number; h: number };
}

const PHASE_LABEL: Record<string, string> = {
  killing_pty: "pausing session…",
  archiving: "archiving working dir…",
  archiving_history: "archiving history…",
  dialing: "dialing peer…",
  offering: "sending offer…",
  awaiting_ack: "waiting for peer…",
  streaming: "streaming…",
  streaming_history: "streaming history…",
  awaiting_commit: "finalizing…",
  extracting: "extracting on peer…",
  installing_history: "installing history…",
  spawning_pty: "starting session…",
};

export function TransferOverlay({ positions, size }: Props) {
  const transfers = useStore((s) => s.transfers);
  const worktrees = useStore((s) => s.worktrees);
  const daemons = useStore((s) => s.daemons);

  // Build a fast lookup: namespaced worktreeId → position
  const posMap = useMemo(() => {
    const m = new Map<string, WorktreePosition>();
    for (const pos of positions) m.set(pos.worktreeId, pos);
    return m;
  }, [positions]);

  // Ghost dot positions — computed once per transfer, stable across re-renders.
  // Keyed by transferId so a new transfer to a different destination gets its own slot.
  const ghostPositions = useMemo(() => {
    const result = new Map<string, { x: number; y: number }>();
    for (const t of Object.values(transfers)) {
      // Find dots on the destination machine
      const destDots = positions.filter((pos) => {
        const wt = worktrees[pos.worktreeId];
        return wt?.machine_id === t.destMachineId;
      });

      let ghostX: number, ghostY: number;
      if (destDots.length > 0) {
        // Centroid of destination machine's dots
        ghostX = destDots.reduce((s, p) => s + p.x, 0) / destDots.length + 32;
        ghostY = destDots.reduce((s, p) => s + p.y, 0) / destDots.length + 32;
      } else {
        // No dots on dest yet — mirror the source dot across the canvas centre
        const srcPos = posMap.get(t.sourceWorktreeKey);
        if (srcPos) {
          ghostX = size.w - srcPos.x;
          ghostY = size.h - srcPos.y;
        } else {
          ghostX = size.w * 0.8;
          ghostY = size.h * 0.5;
        }
      }
      // Clamp to canvas
      ghostX = Math.max(40, Math.min(size.w - 40, ghostX));
      ghostY = Math.max(40, Math.min(size.h - 40, ghostY));
      result.set(t.transferId, { x: ghostX, y: ghostY });
    }
    return result;
  }, [transfers, positions, worktrees, posMap, size]);

  const activeTransfers = Object.values(transfers).filter(
    (t) => t.phase !== "complete" && t.phase !== "failed",
  );

  if (activeTransfers.length === 0) return null;

  return (
    <>
      {/* SVG layer — connector path + ghost dot + source outline */}
      <svg
        style={{
          position: "absolute",
          inset: 0,
          pointerEvents: "none",
          overflow: "visible",
          zIndex: 5,
        }}
        width={size.w}
        height={size.h}
      >
        <defs>
          <style>{`
            @keyframes mc-shimmer {
              0%   { stroke-dashoffset: 0; }
              100% { stroke-dashoffset: -60; }
            }
          `}</style>
        </defs>

        {activeTransfers.map((t) => {
          const srcPos = posMap.get(t.sourceWorktreeKey);
          const ghostPos = ghostPositions.get(t.transferId);
          if (!srcPos || !ghostPos) return null;

          const pathD = `M ${srcPos.x} ${srcPos.y} L ${ghostPos.x} ${ghostPos.y}`;
          const progress =
            t.totalBytes > 0 ? Math.min(1, t.bytesSent / t.totalBytes) : 0;
          const dx = ghostPos.x - srcPos.x;
          const dy = ghostPos.y - srcPos.y;

          return (
            <g key={t.transferId}>
              {/* Background connector */}
              <path d={pathD} stroke="#2e2e32" strokeWidth={1.5} fill="none" />

              {/* Progress fill */}
              {progress > 0 && (
                <path
                  d={`M ${srcPos.x} ${srcPos.y} L ${srcPos.x + dx * progress} ${srcPos.y + dy * progress}`}
                  stroke="#636368"
                  strokeWidth={1.5}
                  fill="none"
                />
              )}

              {/* Shimmer — short bright segment travelling along the path */}
              <path
                d={pathD}
                stroke="#b8b8be"
                strokeWidth={1.5}
                fill="none"
                opacity={0.5}
                strokeDasharray="12 48"
                style={{ animation: "mc-shimmer 1.2s linear infinite" }}
              />

              {/* Source dot — dashed outline indicating "leaving" state */}
              <rect
                x={srcPos.x - srcPos.dotSize / 2 - 3}
                y={srcPos.y - srcPos.dotSize / 2 - 3}
                width={srcPos.dotSize + 6}
                height={srcPos.dotSize + 6}
                fill="none"
                stroke="#636368"
                strokeWidth={1}
                strokeDasharray="3 2"
              />

              {/* Ghost destination dot — outlined square */}
              <rect
                x={ghostPos.x - 6}
                y={ghostPos.y - 6}
                width={12}
                height={12}
                fill="none"
                stroke="#48484e"
                strokeWidth={1}
                strokeDasharray="2 2"
              />
              {/* Arrow glyph on source */}
              <text
                x={srcPos.x + srcPos.dotSize / 2 + 5}
                y={srcPos.y + 4}
                fontSize={9}
                fontFamily="ui-monospace, monospace"
                fill="#636368"
                style={{ pointerEvents: "none" }}
              >
                ⇢
              </text>
            </g>
          );
        })}
      </svg>

      {/* HTML transfer cards — anchored at path midpoint */}
      {activeTransfers.map((t) => {
        const srcPos = posMap.get(t.sourceWorktreeKey);
        const ghostPos = ghostPositions.get(t.transferId);
        if (!srcPos || !ghostPos) return null;

        const midX = (srcPos.x + ghostPos.x) / 2;
        const midY = (srcPos.y + ghostPos.y) / 2;
        const progress =
          t.totalBytes > 0 ? Math.min(1, t.bytesSent / t.totalBytes) : 0;
        const mbSent = (t.bytesSent / 1024 / 1024).toFixed(1);
        const mbTotal = (t.totalBytes / 1024 / 1024).toFixed(1);
        const srcName = daemons[t.sourceMachineId]?.name ?? t.sourceMachineId;
        const dstName = daemons[t.destMachineId]?.name ?? t.destMachineId;

        const CARD_W = 220;
        const CARD_H = 80;
        const cardX = Math.max(
          4,
          Math.min(size.w - CARD_W - 4, midX - CARD_W / 2),
        );
        const cardY = Math.max(
          4,
          Math.min(size.h - CARD_H - 4, midY - CARD_H / 2),
        );

        const phaseText =
          t.phase === "streaming" || t.phase === "streaming_history"
            ? t.totalBytes > 0
              ? `${mbSent} / ${mbTotal} MB`
              : `${mbSent} MB`
            : (PHASE_LABEL[t.phase] ?? t.phase);

        return (
          <div
            key={t.transferId}
            style={{
              position: "absolute",
              left: cardX,
              top: cardY,
              width: CARD_W,
              pointerEvents: "none",
              zIndex: 6,
            }}
            className="bg-background border border-border text-xs font-mono p-2 space-y-1"
          >
            <div className="truncate text-foreground">
              {t.projectName} / {t.branch}
            </div>
            <div className="text-muted-foreground truncate">
              {srcName} → {dstName}
            </div>
            <div className="w-full bg-muted h-1">
              {t.totalBytes > 0 ? (
                <div
                  className="bg-foreground h-full transition-[width] duration-300"
                  style={{ width: `${Math.round(progress * 100)}%` }}
                />
              ) : (
                <div className="h-full overflow-hidden">
                  <div
                    className="bg-foreground h-full w-1/3"
                    style={{ animation: "mc-shimmer 1.2s linear infinite" }}
                  />
                </div>
              )}
            </div>
            <div className="text-muted-foreground">{phaseText}</div>
          </div>
        );
      })}
    </>
  );
}
