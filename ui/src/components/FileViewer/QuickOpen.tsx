import { useEffect, useRef, useState } from 'react'
import { useStore } from '@/store'
import { splitKey } from '@/store'

/** Hand-rolled fuzzy scorer: rewards substring matches and character-order matches. */
function fuzzyScore(query: string, candidate: string): number {
  if (!query) return 1
  const q = query.toLowerCase()
  const c = candidate.toLowerCase()

  // Exact substring match scores highest
  if (c.includes(q)) return 2 + (1 - q.length / c.length)

  // Character-order match (all query chars appear in order)
  let qi = 0
  let score = 0
  for (let ci = 0; ci < c.length && qi < q.length; ci++) {
    if (c[ci] === q[qi]) {
      score += 1 - ci / c.length
      qi++
    }
  }
  if (qi < q.length) return 0 // not all chars matched
  return score / q.length
}

export function QuickOpen() {
  const cmdPOpen = useStore(s => s.cmdPOpen)
  const cmdPTargetWorktree = useStore(s => s.cmdPTargetWorktree)
  const fileList = useStore(s => cmdPTargetWorktree ? s.fileList[cmdPTargetWorktree] : undefined)
  const closeCmdP = useStore(s => s.closeCmdP)
  const send = useStore(s => s.send)

  const [query, setQuery] = useState('')
  const [selectedIdx, setSelectedIdx] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  // Reset state when opening
  useEffect(() => {
    if (cmdPOpen) {
      setQuery('')
      setSelectedIdx(0)
      setTimeout(() => inputRef.current?.focus(), 0)
    }
  }, [cmdPOpen])

  if (!cmdPOpen || !cmdPTargetWorktree) return null

  const [machineId, rawId] = splitKey(cmdPTargetWorktree)

  const files = fileList ?? []
  const results = query
    ? files
        .map(f => ({ file: f, score: fuzzyScore(query, f) }))
        .filter(r => r.score > 0)
        .sort((a, b) => b.score - a.score)
        .slice(0, 50)
        .map(r => r.file)
    : files.slice(0, 50)

  function selectFile(path: string) {
    send(machineId, { type: 'read_file', worktree_id: rawId, path })
    closeCmdP()
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Escape') {
      closeCmdP()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIdx(i => Math.min(i + 1, results.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIdx(i => Math.max(i - 1, 0))
    } else if (e.key === 'Enter') {
      if (results[selectedIdx]) selectFile(results[selectedIdx])
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[15vh]"
      onClick={closeCmdP}
    >
      <div
        className="w-[560px] max-h-[60vh] flex flex-col border border-border bg-background shadow-lg overflow-hidden"
        onClick={e => e.stopPropagation()}
      >
        {/* Search input */}
        <div className="flex items-center gap-2 px-3 py-2 border-b border-border shrink-0">
          <span className="text-xs font-mono text-muted-foreground">⌘P</span>
          <input
            ref={inputRef}
            className="flex-1 bg-transparent text-sm font-mono outline-none placeholder:text-muted-foreground"
            placeholder="Search files…"
            value={query}
            onChange={e => { setQuery(e.target.value); setSelectedIdx(0) }}
            onKeyDown={handleKeyDown}
          />
          {!fileList && (
            <span className="text-xs font-mono text-muted-foreground shrink-0">loading…</span>
          )}
        </div>

        {/* Results */}
        <div className="flex-1 overflow-auto">
          {results.length === 0 && query && (
            <div className="px-3 py-4 text-xs font-mono text-muted-foreground">
              no files match "{query}"
            </div>
          )}
          {results.map((file, idx) => (
            <button
              key={file}
              className={`w-full flex items-center px-3 py-1.5 text-xs font-mono text-left transition-colors ${
                idx === selectedIdx ? 'bg-muted text-foreground' : 'text-muted-foreground hover:bg-muted hover:text-foreground'
              }`}
              onClick={() => selectFile(file)}
              onMouseEnter={() => setSelectedIdx(idx)}
            >
              <span className="truncate">{file}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}
