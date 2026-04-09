import type { DaemonConfig } from '@/store/types'
import type { WorktreeInfo } from '@/lib/protocol'
import { fmtBytes } from '@/components/Layout/MemoryBanner'

interface Props {
  daemon: DaemonConfig
  worktrees: WorktreeInfo[]
  memoryAlert: { level: 'warning' | 'critical'; availableBytes: number; totalBytes: number } | null
  peerCount: number
}

export function StateSentence({ daemon, worktrees, memoryAlert, peerCount }: Props) {
  if (!daemon.connected) {
    return (
      <p className="text-sm text-muted-foreground font-mono">
        {daemon.name} has disconnected.
      </p>
    )
  }

  const running = worktrees.filter(w => w.status === 'running').length
  const needsYou = worktrees.filter(w => w.status === 'needs_you').length
  const total = worktrees.length

  const parts: string[] = []

  if (memoryAlert?.level === 'critical') {
    parts.push(`${daemon.name} is under severe memory pressure — ${fmtBytes(memoryAlert.availableBytes)} free.`)
  } else if (memoryAlert?.level === 'warning') {
    parts.push(`${daemon.name} is running hot — memory tight at ${fmtBytes(memoryAlert.availableBytes)} free.`)
  } else if (running > 0) {
    parts.push(`${daemon.name} is active.`)
  } else {
    parts.push(`${daemon.name} is quietly idle.`)
  }

  if (running > 0) {
    parts.push(`${running} worktree${running !== 1 ? 's' : ''} running${needsYou > 0 ? `, ${needsYou} waiting on you` : ''}.`)
  } else if (total > 0) {
    parts.push(`${total} worktree${total !== 1 ? 's' : ''} parked.`)
  }

  if (!memoryAlert && peerCount > 0) {
    parts.push(`Linked with ${peerCount} peer${peerCount !== 1 ? 's' : ''}.`)
  }

  if (!memoryAlert && running === 0 && total === 0) {
    parts.push('No worktrees registered.')
  }

  return (
    <p className="text-sm font-normal leading-snug">
      {parts.join(' ')}
    </p>
  )
}
