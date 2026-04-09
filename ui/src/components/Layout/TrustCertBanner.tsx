import { useEffect, useState } from 'react'
import { useStore } from '@/store'
import { X } from 'lucide-react'

const DISMISS_KEY = 'hush-cert-banner-dismissed'

/**
 * Shows a banner when all daemons are disconnected and none has ever connected
 * in this session. Prompts the user to trust the self-signed cert by visiting
 * the /health endpoint in their browser.
 */
export function TrustCertBanner() {
  const daemons = useStore(s => s.daemons)
  const [everConnected, setEverConnected] = useState(false)
  const [dismissed, setDismissed] = useState(
    () => sessionStorage.getItem(DISMISS_KEY) === '1'
  )
  // Give the WS a few seconds to connect before showing the banner
  const [waited, setWaited] = useState(false)

  const anyConnected = Object.values(daemons).some(d => d.connected)

  // Track if we've ever had a successful connection this session
  useEffect(() => {
    if (anyConnected) setEverConnected(true)
  }, [anyConnected])

  useEffect(() => {
    const t = setTimeout(() => setWaited(true), 4000)
    return () => clearTimeout(t)
  }, [])

  const show = waited && !anyConnected && !everConnected && !dismissed

  if (!show) return null

  // Pick the first daemon URL to build the health link
  const firstDaemon = Object.values(daemons)[0]
  const wsUrl = firstDaemon?.url ?? 'wss://localhost:9111/ws'
  const healthUrl = wsUrl
    .replace(/^wss:\/\//, 'https://')
    .replace(/^ws:\/\//, 'http://')
    .replace(/\/ws$/, '/health')

  function dismiss() {
    sessionStorage.setItem(DISMISS_KEY, '1')
    setDismissed(true)
  }

  return (
    <div className="flex items-center gap-3 px-3 py-1.5 text-xs font-mono border-b border-amber-800 bg-amber-950 text-amber-200">
      <span className="shrink-0 text-amber-400">⚠</span>
      <span className="flex-1">
        Cannot connect to daemon. If using a self-signed cert,{' '}
        <a
          href={healthUrl}
          target="_blank"
          rel="noreferrer"
          className="underline hover:text-amber-100"
        >
          trust it here
        </a>
        , then refresh the page.
      </span>
      <button
        onClick={dismiss}
        className="shrink-0 text-amber-400 hover:text-amber-100"
        title="Dismiss"
      >
        <X className="w-3 h-3" />
      </button>
    </div>
  )
}
