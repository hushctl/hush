import { useStore } from "@/store";
import { statusColor } from "@/lib/status";
import type { Panel } from "@/store/types";

/**
 * SVG overlay that draws connecting lines between panels belonging to the same
 * project. Rendered below the panels so it never blocks interaction.
 */
export function CanvasConnectors() {
  const panels = useStore((s) => s.canvas.panels);
  const worktrees = useStore((s) => s.worktrees);

  // Resolve a panel's project ID
  function projectId(panel: Panel): string | null {
    if (panel.kind === "worktree_list") return panel.targetId;
    const wt = worktrees[panel.targetId];
    return wt?.project_id ?? null;
  }

  // Group panels by project — only draw when 2+ panels share a project
  const byProject = new Map<string, Panel[]>();
  for (const panel of panels) {
    const pid = projectId(panel);
    if (!pid) continue;
    const group = byProject.get(pid) ?? [];
    group.push(panel);
    byProject.set(pid, group);
  }

  const lines: React.ReactNode[] = [];

  for (const [pid, group] of byProject) {
    if (group.length < 2) continue;

    // Pick a representative color from the first worktree in the project
    const repWt = Object.values(worktrees).find((w) => w.project_id === pid);
    const color = repWt ? statusColor(repWt.status) : "#636368";

    // Connect each panel to the next in the group (chain, not full mesh)
    for (let i = 0; i < group.length - 1; i++) {
      const a = group[i];
      const b = group[i + 1];

      // Anchor at the colored status dot (top-left of header, px-2 inset, h-7 center)
      const DOT_X = 10; // 8px padding + ~2px to dot center
      const DOT_Y = 14; // h-7 (28px) / 2
      const ax = a.x + DOT_X;
      const ay = a.y + DOT_Y;
      const bx = b.x + DOT_X;
      const by = b.y + DOT_Y;

      // Midpoint control point — bow outward by 40px perpendicular to the line
      const mx = (ax + bx) / 2;
      const my = (ay + by) / 2;
      const dx = bx - ax;
      const dy = by - ay;
      const len = Math.hypot(dx, dy) || 1;
      const bow = 40;
      const cx = mx - (dy / len) * bow;
      const cy = my + (dx / len) * bow;

      lines.push(
        <path
          key={`${a.id}-${b.id}`}
          d={`M ${ax} ${ay} Q ${cx} ${cy} ${bx} ${by}`}
          fill="none"
          stroke={color}
          strokeWidth={1}
          strokeOpacity={0.35}
          strokeDasharray="4 4"
        />,
      );
    }
  }

  if (lines.length === 0) return null;

  return (
    <svg
      className="absolute inset-0 pointer-events-none"
      style={{ width: "100%", height: "100%" }}
    >
      {lines}
    </svg>
  );
}
