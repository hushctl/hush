import { useState, useEffect, useRef } from "react";
import { useStore, splitKey } from "@/store";
import { statusColor } from "@/lib/status";
import { StatusPill } from "./StatusPill";
import { Button } from "@/components/ui/button";
import type { WorktreeInfo } from "@/lib/protocol";
import type { TransferState } from "@/store/types";

interface Props {
  worktree: WorktreeInfo;
  /** @deprecated use `variant` instead */
  compact?: boolean;
  variant?: "full" | "quarter" | "minimal";
  onOpen?: () => void;
}

export function ProjectCard({ worktree, compact, variant: variantProp, onOpen }: Props) {
  const variant = variantProp ?? (compact ? "minimal" : "full");
  const project = useStore((s) => s.projects[worktree.project_id]);
  const send = useStore((s) => s.send);
  const openDaemonDetail = useStore((s) => s.openDaemonDetail);
  const daemons = useStore((s) => s.daemons);
  const transfers = useStore((s) => s.transfers);
  const borderColor = statusColor(worktree.status);
  const [transferOpen, setTransferOpen] = useState(false);
  const transferRef = useRef<HTMLDivElement>(null);

  // Close dropdown on outside click
  useEffect(() => {
    if (!transferOpen) return;
    function onOutside(e: MouseEvent) {
      if (
        transferRef.current &&
        !transferRef.current.contains(e.target as Node)
      ) {
        setTransferOpen(false);
      }
    }
    document.addEventListener("mousedown", onOutside);
    return () => document.removeEventListener("mousedown", onOutside);
  }, [transferOpen]);

  // Other connected daemons (potential transfer destinations)
  const otherDaemons = Object.values(daemons).filter(
    (d) => d.connected && d.id !== worktree.machine_id,
  );

  // Active outbound transfer for this worktree
  const activeTransfer = Object.values(transfers).find(
    (t): t is TransferState =>
      t.sourceWorktreeKey === worktree.id &&
      t.phase !== "complete" &&
      t.phase !== "failed",
  );

  function handleTransfer(destMachineId: string) {
    const [mid, rawWtId] = splitKey(worktree.id);
    send(mid || worktree.machine_id, {
      type: "transfer_worktree",
      worktree_id: rawWtId || worktree.id,
      dest_machine_id: destMachineId,
    });
    setTransferOpen(false);
  }

  const isNeedsYou = worktree.status === "needs_you";
  const isFailed = worktree.status.startsWith("failed");

  const borderClass = isNeedsYou
    ? "border-amber-400"
    : isFailed
      ? "border-red-400"
      : "border-border";

  // ── Minimal: name + dot only ─────────────────────────────────────────────
  if (variant === "minimal") {
    return (
      <div
        data-testid="project-card"
        data-status={worktree.status}
        className={`flex items-center gap-2 px-2 py-1.5 border ${borderClass} cursor-pointer hover:bg-muted transition-colors`}
        onClick={onOpen}
        style={{ borderLeftColor: borderColor, borderLeftWidth: 2 }}
      >
        <span
          className="inline-block w-2 h-2 shrink-0"
          style={{ backgroundColor: borderColor }}
        />
        <span className="text-xs font-mono truncate flex-1">
          {project?.name ?? worktree.project_id}
        </span>
      </div>
    );
  }

  // ── Quarter: name + dot + pill + one-line breadcrumb + badge count ────────
  if (variant === "quarter") {
    const breadcrumb = worktree.working_dir
      .split("/")
      .filter(Boolean)
      .slice(-2)
      .join("/");
    const queueCount = worktree.queued_tasks?.length ?? 0;
    return (
      <div
        data-testid="project-card"
        data-status={worktree.status}
        className={`flex items-center gap-2 px-2 py-1.5 border ${borderClass} cursor-pointer hover:bg-muted transition-colors`}
        onClick={onOpen}
        style={{ borderLeftColor: borderColor, borderLeftWidth: 2 }}
      >
        <span
          className="inline-block w-2 h-2 shrink-0"
          style={{ backgroundColor: borderColor }}
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="text-xs font-mono truncate">
              {project?.name ?? worktree.project_id}
            </span>
            <StatusPill status={worktree.status} />
            {queueCount > 0 && (
              <span className="text-xs font-mono border border-border px-1 text-muted-foreground shrink-0">
                {queueCount}
              </span>
            )}
          </div>
          <div className="text-xs font-mono text-muted-foreground truncate">
            {breadcrumb}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      data-testid="project-card"
      data-status={worktree.status}
      className={`border ${borderClass} bg-card`}
      style={{ borderLeftColor: borderColor, borderLeftWidth: 2 }}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border">
        <div className="flex items-center gap-2 min-w-0">
          <span
            className="inline-block w-2 h-2 shrink-0"
            style={{ backgroundColor: borderColor }}
          />
          <span className="text-sm font-normal truncate">
            {project?.name ?? worktree.project_id}
          </span>
          <span className="text-xs font-mono text-muted-foreground truncate">
            {worktree.branch}
          </span>
        </div>
        <StatusPill status={worktree.status} />
      </div>

      {/* Indeterminate progress bar — visible only while running */}
      {worktree.status === "running" && (
        <div className="h-px bg-muted overflow-hidden">
          <div className="h-full w-full bg-green-500 animate-pulse" />
        </div>
      )}

      {/* Body */}
      <div className="px-3 py-2 space-y-1">
        <div className="text-xs text-muted-foreground uppercase tracking-wide font-mono">
          {worktree.status === "running"
            ? "current task"
            : isNeedsYou
              ? "waiting for approval"
              : isFailed
                ? "error"
                : "last session"}
        </div>
        {worktree.last_task && (
          <div className="text-sm font-normal truncate">
            {worktree.last_task}
          </div>
        )}
        <div className="text-xs text-muted-foreground font-mono truncate">
          {worktree.working_dir}
        </div>
        {worktree.status === "idle" &&
          worktree.queued_tasks &&
          worktree.queued_tasks.length > 0 && (
            <div className="mt-1 space-y-0.5">
              <div className="text-xs text-muted-foreground uppercase tracking-wide font-mono">
                queued ({worktree.queued_tasks.length})
              </div>
              {worktree.queued_tasks.slice(0, 3).map((task, i) => (
                <div
                  key={i}
                  className="text-xs font-mono text-muted-foreground truncate pl-2 border-l border-border"
                >
                  {task}
                </div>
              ))}
              {worktree.queued_tasks.length > 3 && (
                <div className="text-xs font-mono text-muted-foreground pl-2">
                  +{worktree.queued_tasks.length - 3} more
                </div>
              )}
            </div>
          )}
        {worktree.machine_id && (
          <button
            className="text-xs font-mono border border-border text-muted-foreground px-1.5 py-0.5 hover:border-foreground hover:text-foreground transition-colors self-start"
            onClick={(e) => {
              e.stopPropagation();
              openDaemonDetail(worktree.machine_id);
            }}
          >
            {worktree.machine_id}
          </button>
        )}
      </div>

      {/* Actions */}
      <div className="flex items-center gap-2 px-3 py-2 border-t border-border flex-wrap">
        <Button
          data-testid="open-chat-btn"
          variant="outline"
          size="sm"
          className="rounded-none shadow-none font-normal h-7 text-xs"
          onClick={onOpen}
        >
          Open chat
        </Button>
        {isNeedsYou && (
          <>
            <Button
              data-testid="approve-btn"
              variant="outline"
              size="sm"
              className="rounded-none shadow-none font-normal h-7 text-xs border-amber-400 text-amber-600 hover:bg-amber-50"
              onClick={() => {
                const [mid, rawId] = splitKey(worktree.id);
                send(mid || worktree.machine_id, {
                  type: "pty_input",
                  worktree_id: rawId || worktree.id,
                  data: "yes, proceed\r",
                });
              }}
            >
              Approve
            </Button>
            <Button
              data-testid="view-diff-btn"
              variant="outline"
              size="sm"
              className="rounded-none shadow-none font-normal h-7 text-xs"
              onClick={onOpen}
            >
              View diff
            </Button>
            <Button
              data-testid="discuss-btn"
              variant="outline"
              size="sm"
              className="rounded-none shadow-none font-normal h-7 text-xs"
              onClick={onOpen}
            >
              Discuss
            </Button>
          </>
        )}
        {isFailed && (
          <>
            <Button
              data-testid="resume-btn"
              variant="outline"
              size="sm"
              className="rounded-none shadow-none font-normal h-7 text-xs border-red-400 text-red-600 hover:bg-red-50"
              onClick={onOpen}
            >
              Resume
            </Button>
            <Button
              data-testid="view-logs-btn"
              variant="outline"
              size="sm"
              className="rounded-none shadow-none font-normal h-7 text-xs"
              onClick={onOpen}
            >
              View logs
            </Button>
            <Button
              data-testid="retry-btn"
              variant="outline"
              size="sm"
              className="rounded-none shadow-none font-normal h-7 text-xs border-red-400 text-red-600 hover:bg-red-50"
              onClick={() => {
                const [mid, rawId] = splitKey(worktree.id);
                send(mid || worktree.machine_id, {
                  type: "pty_input",
                  worktree_id: rawId || worktree.id,
                  data: "please retry\r",
                });
              }}
            >
              Retry
            </Button>
          </>
        )}
        {otherDaemons.length > 0 && !activeTransfer && (
          <div ref={transferRef} className="relative ml-auto">
            <Button
              variant="outline"
              size="sm"
              className="rounded-none shadow-none font-normal h-7 text-xs"
              onClick={() => setTransferOpen((v) => !v)}
            >
              Transfer to…
            </Button>
            {transferOpen && (
              <div className="absolute right-0 bottom-full mb-1 z-50 border border-border bg-background min-w-max">
                {otherDaemons.map((d) => (
                  <button
                    key={d.id}
                    className="block w-full text-left px-3 py-1.5 text-xs font-mono hover:bg-muted whitespace-nowrap"
                    onClick={() => handleTransfer(d.id)}
                  >
                    {d.name}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
        {activeTransfer && (
          <div className="ml-auto flex items-center gap-2">
            <span className="text-xs font-mono text-muted-foreground">
              {activeTransfer.phase === "streaming" &&
              activeTransfer.totalBytes > 0
                ? `→ ${Math.round((activeTransfer.bytesSent / 1024 / 1024) * 10) / 10} / ${Math.round((activeTransfer.totalBytes / 1024 / 1024) * 10) / 10} MB`
                : activeTransfer.phase}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
