import { statusColor, statusLabel } from "@/lib/status";
import type { WorktreeStatus } from "@/lib/protocol";

interface Props {
  status: WorktreeStatus | string;
}

export function StatusPill({ status }: Props) {
  const label = statusLabel(status);
  const color = statusColor(status);
  return (
    <span
      data-testid="status-pill"
      data-status={status}
      className="inline-flex items-center gap-1 px-2 py-0.5 text-xs font-mono border rounded-none"
      style={{ borderColor: color, color }}
    >
      <span
        className="inline-block w-1.5 h-1.5"
        style={{ backgroundColor: color }}
      />
      {label}
    </span>
  );
}
