import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useStore, splitKey } from "@/store";
import { ptyBus, type PtyPayload } from "@/lib/ptyBus";

interface Props {
  /** Namespaced worktree ID: `${machineId}:${rawId}` */
  worktreeId: string;
}

/**
 * Embedded Claude Code terminal — xterm.js renders a live pty stream from the
 * daemon. The component owns the xterm.Terminal instance imperatively; bytes
 * arrive via the PtyBus rather than React state to avoid per-byte re-renders.
 */
export function TerminalPane({ worktreeId }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const send = useStore((s) => s.send);
  // Keep a ref so closures inside the one-time effect always call the latest send
  // without needing to re-mount the terminal when the function reference changes.
  const sendRef = useRef(send);
  sendRef.current = send;

  // Split namespaced ID into machineId + rawId for daemon messages
  const [machineId, rawWorktreeId] = splitKey(worktreeId);

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      fontFamily:
        "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      fontSize: 13,
      cursorBlink: true,
      theme: {
        background: "#1a1a1e",
        foreground: "#c8c8c8",
      },
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    termRef.current = term;
    fitRef.current = fit;

    // Defer initial fit so the absolutely-positioned container has its layout
    requestAnimationFrame(() => {
      fit.fit();
      const cols = term.cols;
      const rows = term.rows;
      send(machineId, {
        type: "pty_attach",
        worktree_id: rawWorktreeId,
        cols,
        rows,
      });
    });

    // Intercept Shift+Enter before xterm maps it to \r (same as plain Enter).
    // Claude Code expects the kitty keyboard protocol sequence \x1b[13;2u
    // (what VSCode's xterm.js sends). Without this, Shift+Enter is
    // indistinguishable from plain Enter and multi-line input never triggers.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type === "keydown" && e.key === "Enter" && e.shiftKey) {
        sendRef.current(machineId, {
          type: "pty_input",
          worktree_id: rawWorktreeId,
          data: "\x1b[13;2u",
        });
        // preventDefault stops the hidden textarea from inserting \n, which would
        // otherwise round-trip back through xterm's onData as a bare newline.
        e.preventDefault();
        return false;
      }
      return true;
    });

    // Forward keystrokes
    const dataDispose = term.onData((data) => {
      sendRef.current(machineId, {
        type: "pty_input",
        worktree_id: rawWorktreeId,
        data,
      });
    });

    // ── Image upload (paste + drag-and-drop) ──────────────────────────────────
    // Send image files to the daemon which writes them to ~/.hush/paste/ and
    // injects the absolute path into the pty's stdin (same as Claude Code's
    // native drag-and-drop).

    /** Read a File/Blob as base64 and send a paste_image message. */
    const uploadImage = (file: File) => {
      const reader = new FileReader();
      reader.onload = () => {
        const result = reader.result;
        if (typeof result !== "string") return;
        const base64 = result.slice(result.indexOf(",") + 1);
        const ext = file.type.split("/")[1] || "png";
        const filename =
          file.name && file.name !== "" && !file.name.startsWith("image.")
            ? file.name
            : `pasted-${Date.now()}.${ext}`;
        sendRef.current(machineId, {
          type: "paste_image",
          worktree_id: rawWorktreeId,
          data: base64,
          filename,
        });
      };
      reader.readAsDataURL(file);
    };

    // Use capture phase so we see the event before xterm's internal textarea
    // handler can swallow it.
    const handlePaste = (e: ClipboardEvent) => {
      const items = e.clipboardData?.items;
      if (!items || items.length === 0) return;
      let handled = false;
      for (let i = 0; i < items.length; i++) {
        const item = items[i];
        if (!item.type.startsWith("image/")) continue;
        const file = item.getAsFile();
        if (!file) continue;
        handled = true;
        uploadImage(file);
      }
      if (handled) e.preventDefault();
    };

    const handleDragOver = (e: DragEvent) => {
      if (e.dataTransfer?.types.includes("Files")) {
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
      }
    };

    const handleDrop = (e: DragEvent) => {
      e.preventDefault();
      const files = e.dataTransfer?.files;
      if (!files) return;
      for (let i = 0; i < files.length; i++) {
        if (files[i].type.startsWith("image/")) uploadImage(files[i]);
      }
    };

    const containerEl = containerRef.current;
    containerEl.addEventListener("paste", handlePaste, true);
    containerEl.addEventListener("dragover", handleDragOver);
    containerEl.addEventListener("drop", handleDrop);

    // Subscribe to bytes from the daemon for this worktree
    const unsub = ptyBus.subscribe(worktreeId, (payload: PtyPayload) => {
      if (payload.kind === "data" || payload.kind === "scrollback") {
        if (payload.data) term.write(payload.data);
      } else if (payload.kind === "exit") {
        term.write("\r\n\x1b[31m[session exited]\x1b[0m\r\n");
      }
    });

    // Resize on container resize — rAF ensures layout is complete before fitting
    const ro = new ResizeObserver(() => {
      requestAnimationFrame(() => {
        try {
          fit.fit();
          if (termRef.current) {
            sendRef.current(machineId, {
              type: "pty_resize",
              worktree_id: rawWorktreeId,
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
      containerEl.removeEventListener("paste", handlePaste, true);
      containerEl.removeEventListener("dragover", handleDragOver);
      containerEl.removeEventListener("drop", handleDrop);
      ro.disconnect();
      unsub();
      dataDispose.dispose();
      sendRef.current(machineId, {
        type: "pty_detach",
        worktree_id: rawWorktreeId,
      });
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [worktreeId]);

  return (
    <div
      ref={containerRef}
      data-testid={`terminal-pane-${worktreeId}`}
      style={{ position: "absolute", inset: 0, background: "#1a1a1e" }}
    />
  );
}
