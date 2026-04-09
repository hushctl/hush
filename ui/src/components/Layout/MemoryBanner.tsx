import { useStore } from '@/store'

function fmtBytes(bytes: number): string {
  const gb = bytes / (1024 * 1024 * 1024)
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${(bytes / (1024 * 1024)).toFixed(0)} MB`
}

export function MemoryBanner() {
  const memoryAlerts = useStore(s => s.memoryAlerts)
  const entries = Object.entries(memoryAlerts)
  if (entries.length === 0) return null

  return (
    <div className="shrink-0">
      {entries.map(([machineId, alert]) => {
        const isWarning = alert.level === 'warning'
        const colorClass = isWarning ? 'text-amber-500 border-amber-500' : 'text-red-500 border-red-500'
        const bgClass = isWarning ? 'bg-amber-500/5' : 'bg-red-500/5'
        const label = isWarning ? 'memory warning' : 'memory critical'
        const avail = fmtBytes(alert.availableBytes)
        const total = fmtBytes(alert.totalBytes)

        return (
          <div
            key={machineId}
            className={`border-b px-3 py-1.5 flex items-center gap-2 text-xs font-mono ${colorClass} ${bgClass}`}
          >
            <span className="shrink-0">▲</span>
            <span>
              <span className="font-medium">{machineId}</span>
              {' — '}
              {label}: {avail} / {total} available. Consider closing heavy sessions or other apps.
            </span>
          </div>
        )
      })}
    </div>
  )
}
