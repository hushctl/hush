import { useEffect, useState } from "react";
import { useStore } from "@/store";
import { statusColor } from "@/lib/status";
import { StateSentence } from "./StateSentence";
import { MemoryPulse } from "./MemoryPulse";
import type { WorktreeInfo } from "@/lib/protocol";

export function DaemonPanel() {
  const selectedDaemonId = useStore((s) => s.selectedDaemonId);
  const closeDaemonDetail = useStore((s) => s.closeDaemonDetail);
  const openDaemonDetail = useStore((s) => s.openDaemonDetail);
  const daemons = useStore((s) => s.daemons);
  const worktrees = useStore((s) => s.worktrees);
  const memoryAlerts = useStore((s) => s.memoryAlerts);
  const memorySamples = useStore((s) => s.memorySamples);
  const openPane = useStore((s) => s.openPane);

  const [trustExpanded, setTrustExpanded] = useState(false);

  const daemon = selectedDaemonId ? daemons[selectedDaemonId] : null;

  // Close on Esc
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") closeDaemonDetail();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeDaemonDetail]);

  if (!daemon) return null;

  const machineId = daemon.id;
  const daemonWorktrees: WorktreeInfo[] = Object.values(worktrees).filter(
    (w) => w.machine_id === machineId,
  );
  const memoryAlert = memoryAlerts[machineId] ?? null;
  const samples = memorySamples[machineId] ?? [];

  // Peers: other daemons that are connected (v1: all other connected daemons are "peers")
  const peers = Object.values(daemons).filter(
    (d) => d.id !== machineId && d.connected,
  );

  return (
    <aside
      data-testid="daemon-panel"
      className="fixed right-0 top-0 bottom-0 w-[420px] border-l border-border bg-background flex flex-col overflow-hidden"
      style={{ zIndex: 9999 }}
    >
      {/* Header */}
      <div className="px-4 py-3 border-b border-border shrink-0">
        <div className="flex items-center justify-between">
          <span className="text-sm font-normal">{daemon.name}</span>
          <button
            className="text-xs font-mono text-muted-foreground hover:text-foreground"
            onClick={closeDaemonDetail}
          >
            ✕
          </button>
        </div>
        <div className="flex items-center gap-2 mt-0.5">
          <span
            className="inline-block w-1.5 h-1.5"
            style={{
              backgroundColor: daemon.connected ? "#22c55e" : "#ef4444",
            }}
          />
          <span className="text-xs font-mono text-muted-foreground">
            {daemon.connected ? "connected" : "disconnected"}
          </span>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {/* State sentence */}
        <div className="px-4 py-3 border-b border-border">
          <StateSentence
            daemon={daemon}
            worktrees={daemonWorktrees}
            memoryAlert={memoryAlert}
            peerCount={peers.length}
          />
        </div>

        {/* Memory pulse */}
        <div className="px-4 py-3 border-b border-border">
          <div className="text-xs font-mono text-muted-foreground uppercase tracking-wide mb-2">
            memory
          </div>
          <MemoryPulse
            alert={memoryAlert}
            samples={samples}
            connected={daemon.connected}
          />
        </div>

        {/* Worktrees strip */}
        {daemonWorktrees.length > 0 && (
          <div className="px-4 py-3 border-b border-border">
            <div className="text-xs font-mono text-muted-foreground uppercase tracking-wide mb-2">
              worktrees — {daemonWorktrees.length}
            </div>
            <div className="flex flex-wrap gap-2">
              {daemonWorktrees.map((wt) => (
                <WorktreeDot
                  key={wt.id}
                  worktree={wt}
                  onOpen={() => openPane(wt.id)}
                />
              ))}
            </div>
            {/* Status summary */}
            <StatusSummary worktrees={daemonWorktrees} />
          </div>
        )}

        {/* Peer mesh */}
        {(peers.length > 0 ||
          Object.values(daemons).filter((d) => d.id !== machineId).length >
            0) && (
          <div className="px-4 py-3 border-b border-border">
            <div className="text-xs font-mono text-muted-foreground uppercase tracking-wide mb-2">
              peers
            </div>
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-xs font-mono border border-foreground px-2 py-1">
                {daemon.name}
              </span>
              {peers.length > 0 && (
                <>
                  <span className="text-xs text-muted-foreground">—</span>
                  {peers.map((peer) => (
                    <button
                      key={peer.id}
                      className="text-xs font-mono border border-border text-muted-foreground px-2 py-1 hover:border-foreground hover:text-foreground transition-colors"
                      onClick={() => openDaemonDetail(peer.id)}
                    >
                      {peer.name}
                    </button>
                  ))}
                </>
              )}
              {peers.length === 0 && (
                <span className="text-xs font-mono text-muted-foreground">
                  no connected peers
                </span>
              )}
            </div>
          </div>
        )}

        {/* Trust & identity (collapsed) */}
        <div className="px-4 py-3 border-b border-border">
          <button
            className="text-xs font-mono text-muted-foreground hover:text-foreground flex items-center gap-1"
            onClick={() => setTrustExpanded((v) => !v)}
          >
            <span>{trustExpanded ? "▾" : "▸"}</span>
            <span>trust &amp; identity</span>
          </button>
          {trustExpanded && (
            <div className="mt-2 space-y-1">
              <div className="text-xs font-mono text-muted-foreground">
                machine id
              </div>
              <div className="text-xs font-mono break-all">{machineId}</div>
              <div className="text-xs font-mono text-muted-foreground mt-2">
                address
              </div>
              <div className="text-xs font-mono break-all">{daemon.url}</div>
              <button
                className="mt-2 text-xs font-mono border border-border px-2 py-1 text-muted-foreground hover:border-foreground hover:text-foreground transition-colors"
                onClick={() => navigator.clipboard.writeText(machineId)}
              >
                copy id
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Footer */}
      <div className="px-4 py-2 border-t border-border flex items-center gap-3 shrink-0">
        <span className="text-xs font-mono text-muted-foreground">
          actions:
        </span>
        <button
          className="text-xs font-mono text-muted-foreground hover:text-foreground transition-colors"
          onClick={() => {
            const wt = daemonWorktrees[0];
            if (wt) openPane(wt.id);
          }}
          disabled={daemonWorktrees.length === 0}
        >
          open terminal
        </button>
      </div>
    </aside>
  );
}

function WorktreeDot({
  worktree,
  onOpen,
}: {
  worktree: WorktreeInfo;
  onOpen: () => void;
}) {
  const color = statusColor(worktree.status);
  return (
    <button
      title={`${worktree.branch} — ${worktree.status}`}
      onClick={onOpen}
      className="flex items-center gap-1.5 px-2 py-1 border border-border text-xs font-mono text-muted-foreground hover:border-foreground hover:text-foreground transition-colors"
    >
      <span
        className="inline-block w-2 h-2 shrink-0"
        style={{ backgroundColor: color }}
      />
      <span className="truncate max-w-[120px]">{worktree.branch}</span>
    </button>
  );
}

function StatusSummary({ worktrees }: { worktrees: WorktreeInfo[] }) {
  const counts: Record<string, number> = {};
  for (const w of worktrees) {
    const s = w.status.startsWith("failed") ? "failed" : w.status;
    counts[s] = (counts[s] ?? 0) + 1;
  }
  const entries = Object.entries(counts);
  if (entries.length === 0) return null;
  return (
    <div className="flex gap-3 mt-2">
      {entries.map(([status, count]) => (
        <span key={status} className="text-xs font-mono text-muted-foreground">
          {count} {status}
        </span>
      ))}
    </div>
  );
}
