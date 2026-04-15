import { useStore } from "@/store";

interface Props {
  worktreeId: string;
  onOpenShell: (shellId: string) => void;
  onNewShell: () => void;
}

/**
 * Stacked shell list shown at the bottom of Claude Code terminal panes.
 * Each row shows a shell's alive status + last output line. Clicking a row
 * opens/focuses that shell's panel. A `+ shell` button at the bottom
 * creates a new shell.
 *
 * Hidden entirely when no shells exist — zero visual footprint.
 */
export function ShellFooter({ worktreeId, onOpenShell, onNewShell }: Props) {
  const shellAlive = useStore((s) => s.shellAlive);
  const lastLines = useStore((s) => s.lastLines);

  // Collect all shells for this worktree from shellAlive and lastLines keys.
  // shellAlive keys: `{worktreeId}:{shellId}`
  // lastLines keys: `shell:{worktreeId}:{shellId}`
  const prefix = `${worktreeId}:`;
  const shellPrefix = `shell:${worktreeId}:`;

  const shellIds = new Set<string>();
  for (const key of Object.keys(shellAlive)) {
    if (key.startsWith(prefix)) {
      shellIds.add(key.slice(prefix.length));
    }
  }
  for (const key of Object.keys(lastLines)) {
    if (key.startsWith(shellPrefix)) {
      shellIds.add(key.slice(shellPrefix.length));
    }
  }

  const shells = [...shellIds]
    .map((sid) => ({
      id: sid,
      alive: shellAlive[`${worktreeId}:${sid}`] ?? false,
      lastLine: lastLines[`shell:${worktreeId}:${sid}`] ?? null,
    }))
    .sort((a, b) => {
      // Alive shells first, then by ID
      if (a.alive !== b.alive) return a.alive ? -1 : 1;
      return a.id.localeCompare(b.id);
    });

  if (shells.length === 0) return null;

  return (
    <div className="shrink-0 border-t border-border bg-background">
      {shells.map((shell, i) => (
        <button
          key={shell.id}
          className="w-full flex items-center gap-2 px-2 py-0.5 hover:bg-accent/50 text-left transition-colors"
          onClick={() => onOpenShell(shell.id)}
          title={`Open shell ${i + 1}`}
        >
          <span className="text-[10px] font-mono shrink-0">
            {shell.alive ? (
              <span className="text-green-600">{">_"}</span>
            ) : (
              <span className="text-zinc-600">{">_"}</span>
            )}
          </span>
          <span className="text-[10px] font-mono text-zinc-500 shrink-0">
            {i + 1}
          </span>
          {shell.lastLine && (
            <span className="text-[10px] font-mono text-muted-foreground truncate">
              {shell.lastLine}
            </span>
          )}
        </button>
      ))}
      <button
        className="w-full px-2 py-0.5 text-[10px] font-mono text-muted-foreground hover:text-foreground hover:bg-accent/50 text-left transition-colors"
        onClick={onNewShell}
        title="Open a new shell"
      >
        + shell
      </button>
    </div>
  );
}
