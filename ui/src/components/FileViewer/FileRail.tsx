import { useStore } from "@/store";
import { splitKey } from "@/store";

interface Props {
  worktreeId: string;
  className?: string;
}

type Section = "staged" | "modified" | "untracked";

const SECTION_LABELS: Record<Section, string> = {
  staged: "STAGED",
  modified: "MODIFIED",
  untracked: "UNTRACKED",
};

const SECTION_CHARS: Record<Section, string> = {
  staged: "A",
  modified: "M",
  untracked: "?",
};

const SECTION_COLORS: Record<Section, string> = {
  staged: "#22c55e", // green
  modified: "#f59e0b", // amber
  untracked: "#6b7280", // gray
};

export function FileRail({ worktreeId, className = "" }: Props) {
  const gitStatus = useStore((s) => s.gitStatus[worktreeId]);
  const fileContent = useStore((s) => s.fileContents[worktreeId]);
  const send = useStore((s) => s.send);
  const clearFileContent = useStore((s) => s.clearFileContent);

  const [machineId, rawId] = splitKey(worktreeId);

  function openFile(path: string) {
    send(machineId, { type: "read_file", worktree_id: rawId, path });
  }

  if (fileContent) {
    return (
      <div
        className={`flex flex-col overflow-hidden text-xs font-mono ${className}`}
      >
        {/* File viewer header */}
        <div className="flex items-center gap-2 px-2 py-1.5 border-b border-border shrink-0">
          <button
            className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
            onClick={() => clearFileContent(worktreeId)}
            title="Back to file list"
          >
            ←
          </button>
          <span
            className="truncate text-muted-foreground"
            title={fileContent.path}
          >
            {fileContent.path}
          </span>
          {fileContent.truncated && (
            <span className="shrink-0 text-amber-500">truncated</span>
          )}
        </div>
        {/* File content */}
        <pre className="flex-1 overflow-auto p-2 text-xs leading-relaxed whitespace-pre-wrap break-all">
          {fileContent.content}
        </pre>
      </div>
    );
  }

  const sections: Section[] = ["staged", "modified", "untracked"];
  const hasAny =
    gitStatus &&
    (gitStatus.staged.length > 0 ||
      gitStatus.modified.length > 0 ||
      gitStatus.untracked.length > 0);

  return (
    <div
      className={`flex flex-col overflow-hidden text-xs font-mono ${className}`}
    >
      <div className="px-2 py-1.5 border-b border-border shrink-0">
        <span className="text-xs uppercase tracking-wider text-muted-foreground">
          changes
        </span>
      </div>

      {!gitStatus || !hasAny ? (
        <div className="flex-1 flex items-center justify-center px-3">
          <span className="text-muted-foreground text-xs">
            — clean working tree —
          </span>
        </div>
      ) : (
        <div className="flex-1 overflow-auto">
          {sections.map((section) => {
            const files = gitStatus[section];
            if (files.length === 0) return null;
            return (
              <div key={section}>
                <div
                  className="px-2 py-1 uppercase tracking-wider text-muted-foreground"
                  style={{ fontSize: "10px" }}
                >
                  {SECTION_LABELS[section]}
                </div>
                {files.map((file) => (
                  <button
                    key={file}
                    className="w-full flex items-center gap-1.5 px-2 py-0.5 hover:bg-muted text-left transition-colors disabled:opacity-50 disabled:cursor-default"
                    onClick={() => !file.endsWith("/") && openFile(file)}
                    disabled={file.endsWith("/")}
                    title={file}
                  >
                    <span
                      className="shrink-0 font-mono"
                      style={{
                        color: SECTION_COLORS[section],
                        fontSize: "10px",
                      }}
                    >
                      {SECTION_CHARS[section]}
                    </span>
                    <span className="truncate text-foreground">{file}</span>
                  </button>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
