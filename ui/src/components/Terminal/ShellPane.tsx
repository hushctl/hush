import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useStore, splitKey } from "@/store";
import { ptyBus, type PtyPayload } from "@/lib/ptyBus";

interface Props {
  /** Namespaced worktree ID: `${machineId}:${rawId}` */
  worktreeId: string;
  /** Shell session ID — each shell gets a unique one. Defaults to "0". */
  shellId?: string;
}

/**
 * Plain shell terminal — runs $SHELL (bash/zsh) in the worktree's directory.
 * Separate from the Claude Code terminal so commands don't pollute the AI session.
 * Multiple shells per worktree are supported via unique shellId values.
 */
export function ShellPane({ worktreeId, shellId = "0" }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const send = useStore((s) => s.send);

  const [machineId, rawWorktreeId] = splitKey(worktreeId);
  // ptyBus channel includes shell_id for multi-shell routing
  const busChannel = `shell:${worktreeId}:${shellId}`;

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      fontFamily:
        "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      fontSize: 13,
      cursorBlink: true,
      theme: {
        background: "#0a0a0a",
        foreground: "#e5e5e5",
      },
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    termRef.current = term;

    requestAnimationFrame(() => {
      fit.fit();
      const cols = term.cols;
      const rows = term.rows;
      send(machineId, {
        type: "shell_attach",
        worktree_id: rawWorktreeId,
        shell_id: shellId,
        cols,
        rows,
      });
    });

    const dataDispose = term.onData((data) => {
      send(machineId, {
        type: "shell_input",
        worktree_id: rawWorktreeId,
        shell_id: shellId,
        data,
      });
    });

    const unsub = ptyBus.subscribe(busChannel, (payload: PtyPayload) => {
      if (payload.kind === "data" || payload.kind === "scrollback") {
        if (payload.data) term.write(payload.data);
      } else if (payload.kind === "exit") {
        term.write("\r\n\x1b[31m[shell exited]\x1b[0m\r\n");
      }
    });

    const ro = new ResizeObserver(() => {
      requestAnimationFrame(() => {
        try {
          fit.fit();
          if (termRef.current) {
            send(machineId, {
              type: "shell_resize",
              worktree_id: rawWorktreeId,
              shell_id: shellId,
              cols: termRef.current.cols,
              rows: termRef.current.rows,
            });
          }
        } catch {
          // Container not measurable yet — ignore.
        }
      });
    });
    ro.observe(containerRef.current);

    return () => {
      ro.disconnect();
      unsub();
      dataDispose.dispose();
      term.dispose();
      termRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [worktreeId, shellId]);

  return (
    <div
      ref={containerRef}
      data-testid={`shell-pane-${worktreeId}`}
      style={{ position: "absolute", inset: 0, background: "#0a0a0a" }}
    />
  );
}
