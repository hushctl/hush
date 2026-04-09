import { useEffect } from 'react'
import { TooltipProvider } from '@/components/ui/tooltip'
import { useDaemonConnections } from '@/hooks/useDaemonConnections'
import { useStore } from '@/store'
import { splitKey } from '@/store'
import { DotGrid } from '@/components/DotGrid/DotGrid'
import { TilingContainer } from '@/components/Layout/TilingContainer'
import { CommandBar } from '@/components/Layout/CommandBar'
import { MemoryBanner } from '@/components/Layout/MemoryBanner'
import { QuickOpen } from '@/components/FileViewer/QuickOpen'
import { DaemonPanel } from '@/components/Daemon/DaemonPanel'

function DisconnectedScreen() {
  return (
    <div
      data-testid="disconnected-screen"
      className="flex flex-col items-center justify-center h-full gap-4 text-center px-8"
    >
      <div className="space-y-1">
        <p className="text-sm font-normal text-foreground">no daemon connected</p>
        <p className="text-xs text-muted-foreground font-mono">connecting to wss://localhost:9111…</p>
      </div>
      <div className="border border-border p-4 text-left space-y-2 max-w-sm w-full">
        <p className="text-xs font-mono text-muted-foreground uppercase tracking-wide">to start the daemon</p>
        <pre className="text-xs font-mono text-foreground whitespace-pre-wrap">cd mission-control/daemon{'\n'}cargo run</pre>
      </div>
      <p className="text-xs text-muted-foreground">the app will connect automatically once the daemon is running</p>
    </div>
  )
}

function AppInner() {
  useDaemonConnections()
  const daemons = useStore(s => s.daemons)
  const connected = Object.values(daemons).some(d => d.connected)
  const layoutMode = useStore(s => s.layoutMode)
  const activePanes = useStore(s => s.activePanes)
  const fileList = useStore(s => s.fileList)
  const send = useStore(s => s.send)
  const openCmdP = useStore(s => s.openCmdP)

  // Global cmd+P handler: open quick-open targeting the last active pane
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.metaKey && e.key === 'p' && activePanes.length > 0) {
        e.preventDefault()
        const targetWorktree = activePanes[activePanes.length - 1]
        // Fetch file list if not cached
        if (!fileList[targetWorktree]) {
          const [machineId, rawId] = splitKey(targetWorktree)
          send(machineId, { type: 'list_files', worktree_id: rawId })
        }
        openCmdP(targetWorktree)
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [activePanes, fileList, send, openCmdP])

  if (!connected) {
    return (
      <div className="flex flex-col w-full h-full">
        <MemoryBanner />
        <main className="flex-1 overflow-hidden">
          <DisconnectedScreen />
        </main>
        <CommandBar />
      </div>
    )
  }

  return (
    <div className="flex flex-col w-full h-full">
      <MemoryBanner />
      <main className="flex-1 overflow-hidden relative">
        {/* Grid — conditional render is fine, DotGrid has no xterm instances */}
        {layoutMode === 'grid' && (
          <div className="absolute inset-0">
            <DotGrid />
          </div>
        )}
        {/* Canvas — always mounted to preserve xterm scrollback across grid/canvas switches */}
        <div
          className="absolute inset-0"
          style={{ display: layoutMode === 'canvas' ? 'block' : 'none' }}
        >
          <TilingContainer />
        </div>
        {/* Daemon detail panel — slide-in overlay, preserves canvas underneath */}
        <DaemonPanel />
      </main>
      <CommandBar />
      <QuickOpen />
    </div>
  )
}

export default function App() {
  return (
    <TooltipProvider>
      <AppInner />
    </TooltipProvider>
  )
}
