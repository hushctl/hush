import React, { useRef, useEffect, useState, useCallback } from "react";
import { useStore } from "@/store";
import { urgencyOrder } from "@/lib/status";
import {
  generateVoronoiCells,
  computeWorktreePositions,
  type WorktreePosition,
} from "@/lib/gridLayout";
import { DetailCard } from "./DetailCard";
import { TransferOverlay } from "./TransferOverlay";
import type { WorktreeInfo } from "@/lib/protocol";

/** One-line summary of all worktrees for the reboarding bar */
function buildReboardingText(worktrees: WorktreeInfo[]): string | null {
  if (worktrees.length === 0) return null;
  const needsYou = worktrees.filter((w) => w.status === "needs_you").length;
  const running = worktrees.filter((w) => w.status === "running").length;
  const failed = worktrees.filter((w) => w.status.startsWith("failed")).length;
  const parts: string[] = [];
  if (needsYou > 0)
    parts.push(
      `${needsYou} ${needsYou === 1 ? "worktree needs" : "worktrees need"} your attention`,
    );
  if (running > 0) parts.push(`${running} running`);
  if (failed > 0) parts.push(`${failed} failed`);
  if (parts.length === 0)
    parts.push(
      `${worktrees.length} ${worktrees.length === 1 ? "worktree" : "worktrees"} idle`,
    );
  return parts.join(" · ");
}

export function DotGrid() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({
    w: window.innerWidth,
    h: window.innerHeight,
  });
  const [hovered, setHovered] = useState<{
    wt: WorktreeInfo;
    pos: WorktreePosition;
  } | null>(null);
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );
  const openPane = useStore((s) => s.openPane);
  const openProjectTree = useStore((s) => s.openProjectTree);
  const worktrees = useStore((s) => s.worktrees);
  const projects = useStore((s) => s.projects);
  const lastLines = useStore((s) => s.lastLines);
  const shellAlive = useStore((s) => s.shellAlive);

  // No per-message activity tracking now that conversation lives in the
  // pty (we don't track terminal bytes as "events"). For v1, treat all
  // worktrees as having the same recency. Status changes from hooks will
  // become the activity signal in a follow-up.
  const lastActivities = new Map<string, number>();

  useEffect(() => {
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setSize({ w: entry.contentRect.width, h: entry.contentRect.height });
      }
    });
    if (containerRef.current) ro.observe(containerRef.current);
    return () => ro.disconnect();
  }, []);

  const { w, h } = size;
  const wtList = Object.values(worktrees);
  const wtPositions = computeWorktreePositions(wtList, w, h, lastActivities);
  const voronoiCells = generateVoronoiCells(w, h, wtPositions);

  // Group worktree positions by project (topmost dot gets the project label)
  const projectFirstDot = new Map<string, string>(); // project_id → worktreeId of topmost dot
  for (const pos of [...wtPositions].sort((a, b) => a.y - b.y)) {
    const wt = worktrees[pos.worktreeId];
    if (wt && !projectFirstDot.has(wt.project_id)) {
      projectFirstDot.set(wt.project_id, pos.worktreeId);
    }
  }

  const reboardingText = buildReboardingText(wtList);

  const scheduleHide = useCallback(() => {
    clearTimeout(hideTimerRef.current);
    hideTimerRef.current = setTimeout(() => setHovered(null), 250);
  }, []);

  const cancelHide = useCallback(() => {
    clearTimeout(hideTimerRef.current);
  }, []);

  // Dot click opens that worktree's terminal pane (CLAUDE.md flow 2).
  // The command bar is for workspace intent, not message routing — there's
  // no longer a "routing target" to set, the terminal IS the input.
  const handleDotClick = useCallback(
    (wt: WorktreeInfo) => {
      openPane(wt.id);
    },
    [openPane],
  );

  const handleProjectLabelClick = useCallback(
    (projectId: string) => {
      openProjectTree(projectId);
    },
    [openProjectTree],
  );

  return (
    <div className="relative w-full h-full overflow-hidden flex flex-col">
      {/* Context reboarding bar */}
      {reboardingText && (
        <div
          data-testid="reboarding-bar"
          className="shrink-0 px-3 py-1.5 border-b border-border bg-background text-xs font-mono text-muted-foreground"
        >
          {reboardingText}
        </div>
      )}

      {/* Grid canvas */}
      <div ref={containerRef} className="relative flex-1 overflow-hidden">
        {/* Background dot grid — SVG only, no interaction */}
        <svg
          data-testid="dot-grid"
          width={w}
          height={h}
          className="absolute inset-0 pointer-events-none"
          style={{ userSelect: "none" }}
        >
          {voronoiCells.map((cell, i) => (
            <polygon
              key={i}
              points={cell.polygon.map(([x, y]) => `${x},${y}`).join(" ")}
              fill={cell.fill}
              stroke="#2e2e32"
              strokeWidth={0.5}
              style={{ transition: "fill 0.4s ease" }}
            />
          ))}

          {/* Labels — SVG text, right-aligned to left of each dot */}
          {wtPositions.map((pos) => {
            const wt = worktrees[pos.worktreeId];
            if (!wt) return null;
            const project = projects[wt.project_id];
            const projectName = (project?.name ?? wt.project_id).toUpperCase();
            const isFirstInProject =
              projectFirstDot.get(wt.project_id) === pos.worktreeId;
            const labelX = pos.x - pos.dotSize / 2 - 8;

            return (
              <g key={`label-${pos.worktreeId}`}>
                {isFirstInProject && (
                  <text
                    x={labelX}
                    y={pos.y - pos.dotSize / 2 - 4}
                    fontSize={9}
                    fontFamily="ui-monospace, monospace"
                    fontWeight={400}
                    fill="#636368"
                    textAnchor="end"
                    letterSpacing="0.08em"
                    style={{ pointerEvents: "none" }}
                  >
                    {projectName}
                  </text>
                )}
                <text
                  x={labelX}
                  y={pos.y + 4}
                  fontSize={10}
                  fontFamily="ui-monospace, monospace"
                  fontWeight={400}
                  fill="#48484e"
                  textAnchor="end"
                  style={{ pointerEvents: "none" }}
                >
                  {wt.branch}
                </text>
                {lastLines[pos.worktreeId] && (
                  <text
                    x={labelX}
                    y={pos.y + 16}
                    fontSize={9}
                    fontFamily="ui-monospace, monospace"
                    fontWeight={400}
                    fill="#38383e"
                    textAnchor="end"
                    style={{ pointerEvents: "none" }}
                  >
                    {lastLines[pos.worktreeId].length > 60
                      ? lastLines[pos.worktreeId].slice(0, 57) + "..."
                      : lastLines[pos.worktreeId]}
                  </text>
                )}
              </g>
            );
          })}
        </svg>

        {/* Project label click zones — invisible HTML buttons over SVG text */}
        {wtPositions.map((pos) => {
          const wt = worktrees[pos.worktreeId];
          if (!wt) return null;
          const isFirstInProject =
            projectFirstDot.get(wt.project_id) === pos.worktreeId;
          if (!isFirstInProject) return null;
          const project = projects[wt.project_id];
          const projectName = (project?.name ?? wt.project_id).toUpperCase();
          const labelX = pos.x - pos.dotSize / 2 - 8;
          // Approximate width of label text
          const approxWidth = projectName.length * 7;
          return (
            <button
              key={`proj-label-${wt.project_id}`}
              data-testid={`project-label-${wt.project_id}`}
              style={{
                position: "absolute",
                right: w - labelX,
                top: pos.y - pos.dotSize / 2 - 20,
                width: approxWidth,
                height: 16,
                background: "transparent",
                border: "none",
                cursor: "pointer",
                padding: 0,
              }}
              onClick={() => handleProjectLabelClick(wt.project_id)}
              title={`Open ${project?.name ?? wt.project_id} tree`}
            />
          );
        })}

        {/* Worktree dots — HTML buttons for reliable click handling */}
        {wtPositions.map((pos) => {
          const wt = worktrees[pos.worktreeId];
          if (!wt) return null;
          const isUrgent = urgencyOrder(wt.status) < 2;
          return (
            <React.Fragment key={pos.worktreeId}>
            <button
              data-testid={`worktree-dot-${wt.id}`}
              data-status={wt.status}
              style={{
                position: "absolute",
                left: pos.x - pos.dotSize / 2,
                top: pos.y - pos.dotSize / 2,
                width: pos.dotSize,
                height: pos.dotSize,
                backgroundColor: pos.color,
                border: "none",
                borderRadius: 0,
                cursor: "pointer",
                padding: 0,
                transition: "all 0.4s ease",
                outline: isUrgent ? `1px solid ${pos.color}` : "none",
                outlineOffset: 2,
              }}
              onClick={() => handleDotClick(wt)}
              onMouseEnter={() => {
                cancelHide();
                setHovered({ wt, pos });
              }}
              onMouseLeave={scheduleHide}
              title={`${wt.branch} — ${wt.status}`}
            />
            {shellAlive[pos.worktreeId] && (
              <span
                style={{
                  position: "absolute",
                  left: pos.x + pos.dotSize / 2 + 4,
                  top: pos.y - 5,
                  fontSize: 9,
                  fontFamily: "ui-monospace, monospace",
                  color: "#8e8e96",
                  pointerEvents: "none",
                  userSelect: "none",
                }}
                title="Shell is running"
              >
                {">_"}
              </span>
            )}
          </React.Fragment>
          );
        })}

        {/* Transfer overlay — connector paths + ghost dots + progress cards */}
        <TransferOverlay positions={wtPositions} size={size} />

        {/* Hover detail card — HTML overlay */}
        {hovered && (
          <div
            data-testid="detail-card"
            className="absolute z-10 w-72"
            style={{
              left: Math.min(
                hovered.pos.x + hovered.pos.dotSize / 2 + 8,
                w - 300,
              ),
              top: Math.min(hovered.pos.y - 40, Math.max(0, h - 260)),
            }}
            onMouseEnter={cancelHide}
            onMouseLeave={scheduleHide}
          >
            <DetailCard
              worktree={hovered.wt}
              onOpen={() => {
                openPane(hovered.wt.id);
                setHovered(null);
              }}
            />
          </div>
        )}

        {/* Empty state */}
        {wtList.length === 0 && (
          <div className="absolute inset-0 flex flex-col items-center justify-center text-center pointer-events-none">
            <p className="text-sm text-muted-foreground">
              No projects registered yet.
            </p>
            <p className="text-xs text-muted-foreground mt-1">
              Use the command bar below to get started.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
