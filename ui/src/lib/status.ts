// Pure status helpers — color, label, urgency ordering. Used by everything
// that visualizes worktree state. No dependency on Claude Code event shape.

export function statusColor(status: string): string {
  if (status === 'running') return '#22c55e'
  if (status === 'needs_you') return '#f59e0b'
  if (status.startsWith('failed')) return '#ef4444'
  return '#9ca3af'
}

export function statusLabel(status: string): string {
  if (status === 'running') return 'running'
  if (status === 'needs_you') return 'needs you'
  if (status.startsWith('failed')) return 'failed'
  return 'idle'
}

export function urgencyOrder(status: string): number {
  if (status === 'needs_you') return 0
  if (status.startsWith('failed')) return 1
  if (status === 'running') return 2
  return 3
}
