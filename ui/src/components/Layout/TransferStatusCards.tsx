import { useStore } from '@/store'

const PHASE_LABEL: Record<string, string> = {
  killing_pty:        'pausing session…',
  archiving:          'archiving working dir…',
  archiving_history:  'archiving history…',
  dialing:            'dialing peer…',
  offering:           'sending offer…',
  awaiting_ack:       'waiting for peer…',
  streaming:          'streaming…',
  streaming_history:  'streaming history…',
  awaiting_commit:    'finalizing…',
  extracting:         'extracting on peer…',
  installing_history: 'installing history…',
  spawning_pty:       'starting session…',
  complete:           'done',
  failed:             'failed',
}

/**
 * Fixed bottom-right cards showing in-flight transfer status.
 * Visible in both grid and canvas modes.
 * Failed transfers stay until dismissed.
 */
export function TransferStatusCards() {
  const transfers = useStore(s => s.transfers)
  const daemons = useStore(s => s.daemons)
  const dismiss = useStore(s => s.dismissTransfer)

  const active = Object.values(transfers)
  if (active.length === 0) return null

  return (
    <div
      style={{
        position: 'fixed',
        bottom: 48, // above CommandBar
        right: 8,
        zIndex: 9990,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      {active.map(t => {
        const progress = t.totalBytes > 0 ? Math.min(1, t.bytesSent / t.totalBytes) : 0
        const mbSent = (t.bytesSent / 1024 / 1024).toFixed(1)
        const mbTotal = (t.totalBytes / 1024 / 1024).toFixed(1)
        const srcName = daemons[t.sourceMachineId]?.name ?? t.sourceMachineId
        const dstName = daemons[t.destMachineId]?.name ?? t.destMachineId
        const isFailed = t.phase === 'failed'
        const isComplete = t.phase === 'complete'
        const isDone = isFailed || isComplete

        const phaseText = (t.phase === 'streaming' || t.phase === 'streaming_history')
          ? t.totalBytes > 0
            ? `${mbSent} / ${mbTotal} MB`
            : `${mbSent} MB`
          : (PHASE_LABEL[t.phase] ?? t.phase)

        return (
          <div
            key={t.transferId}
            className={`bg-background border text-xs font-mono p-2 space-y-1 ${isFailed ? 'border-red-500' : isComplete ? 'border-border' : 'border-border'}`}
            style={{ width: 240 }}
          >
            <div className="flex items-start justify-between gap-2">
              <div className={`truncate font-normal ${isFailed ? 'text-red-400' : 'text-foreground'}`}>
                {t.projectName} / {t.branch}
              </div>
              {isDone && (
                <button
                  className="shrink-0 text-muted-foreground hover:text-foreground leading-none"
                  onClick={() => dismiss(t.transferId)}
                  title="Dismiss"
                >
                  ✕
                </button>
              )}
            </div>
            <div className="text-muted-foreground truncate">{srcName} → {dstName}</div>
            {!isComplete && !isFailed && (
              <div className="w-full bg-muted h-1">
                {t.totalBytes > 0 ? (
                  <div
                    className="bg-foreground h-full transition-[width] duration-300"
                    style={{ width: `${Math.round(progress * 100)}%` }}
                  />
                ) : (
                  <div className="h-full bg-muted-foreground/30 animate-pulse" />
                )}
              </div>
            )}
            <div className={isFailed ? 'text-red-400' : isComplete ? 'text-foreground' : 'text-muted-foreground'}>
              {isFailed && t.errorMessage ? t.errorMessage : phaseText}
            </div>
          </div>
        )
      })}
    </div>
  )
}
