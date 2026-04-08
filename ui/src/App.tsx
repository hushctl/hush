import { TooltipProvider } from '@/components/ui/tooltip'
import { useDaemonConnections } from '@/hooks/useDaemonConnections'
import { useStore } from '@/store'
import { DotGrid } from '@/components/DotGrid/DotGrid'
import { TilingContainer } from '@/components/Layout/TilingContainer'
import { ProjectTree } from '@/components/ProjectTree/ProjectTree'
import { CommandBar } from '@/components/Layout/CommandBar'

function DisconnectedScreen() {
  return (
    <div
      data-testid="disconnected-screen"
      className="flex flex-col items-center justify-center h-full gap-4 text-center px-8"
    >
      <div className="space-y-1">
        <p className="text-sm font-normal text-foreground">no daemon connected</p>
        <p className="text-xs text-muted-foreground font-mono">connecting to ws://localhost:9111…</p>
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

  if (!connected) {
    return (
      <div className="flex flex-col w-full h-full">
        <main className="flex-1 overflow-hidden">
          <DisconnectedScreen />
        </main>
        <CommandBar />
      </div>
    )
  }

  return (
    <div className="flex flex-col w-full h-full">
      <main className="flex-1 overflow-hidden">
        {layoutMode === 'grid' && <DotGrid />}
        {layoutMode === 'panes' && <TilingContainer />}
        {layoutMode === 'tree' && <ProjectTree />}
      </main>
      <CommandBar />
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
