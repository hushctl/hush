import { fmtBytes } from "@/components/Layout/MemoryBanner";

interface Sample {
  t: number;
  ratio: number;
}

interface Props {
  alert: {
    level: "warning" | "critical";
    availableBytes: number;
    totalBytes: number;
  } | null;
  samples: Sample[];
  connected: boolean;
}

const PULSE_COLOR: Record<string, string> = {
  warning: "#f59e0b",
  critical: "#ef4444",
  normal: "#38383e",
  disconnected: "#2e2e32",
};

export function MemoryPulse({ alert, samples, connected }: Props) {
  const level = !connected ? "disconnected" : alert ? alert.level : "normal";
  const color = PULSE_COLOR[level];

  const hasData = alert !== null || samples.length > 0;

  return (
    <div className="flex items-center gap-4">
      {/* Breathing dot */}
      <div
        className={
          level !== "disconnected" && level !== "normal"
            ? "animate-pulse"
            : undefined
        }
        style={{
          width: 40,
          height: 40,
          borderRadius: 0,
          backgroundColor: color,
          flexShrink: 0,
          opacity: connected ? 1 : 0.35,
        }}
      />

      <div className="flex flex-col gap-1 min-w-0 flex-1">
        {/* Label */}
        <span className="text-xs font-mono text-muted-foreground">
          {!connected
            ? "disconnected"
            : alert
              ? `${fmtBytes(alert.availableBytes)} / ${fmtBytes(alert.totalBytes)} free`
              : "memory ok"}
        </span>

        {/* Sparkline */}
        {hasData && samples.length > 1 && (
          <Sparkline samples={samples} level={level} />
        )}
      </div>
    </div>
  );
}

function Sparkline({ samples, level }: { samples: Sample[]; level: string }) {
  const W = 180;
  const H = 28;
  const minRatio = 0;
  const maxRatio = 1;

  const pts = samples.map((s, i) => {
    const x = (i / (samples.length - 1)) * W;
    // Invert: low ratio (less free) = higher on chart
    const y = H - ((s.ratio - minRatio) / (maxRatio - minRatio)) * H;
    return `${x},${y}`;
  });

  const color = PULSE_COLOR[level];

  return (
    <svg width={W} height={H} style={{ display: "block", overflow: "visible" }}>
      <polyline
        points={pts.join(" ")}
        fill="none"
        stroke={color}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
        opacity={0.7}
      />
    </svg>
  );
}
