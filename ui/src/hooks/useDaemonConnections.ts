/**
 * Manages one WebSocket per registered daemon. For each daemon in the registry:
 *   - Opens a WebSocket to daemon.url
 *   - On connect: sends list_projects + list_worktrees + list_peers
 *   - On message: calls handleServerMessage (machine_id is in every message)
 *   - On close: exponential backoff reconnect, per-daemon
 *   - On peer_list: store.mergeDiscoveredPeers auto-adds new daemons, which
 *     triggers new connections on the next render
 *
 * The hook wires all connections' send functions into the store so that
 * store.send(machineId, msg) routes to the right WebSocket.
 */
import { useEffect, useRef, useCallback } from 'react'
import { useStore } from '@/store'
import type { ClientMessage } from '@/lib/protocol'

export function useDaemonConnections() {
  const daemons = useStore(s => s.daemons)
  const setSend = useStore(s => s.setSend)
  const setDaemonConnected = useStore(s => s.setDaemonConnected)
  const handleServerMessage = useStore(s => s.handleServerMessage)

  // Map from machineId → send fn, kept in a ref so the stable send closure
  // can look up the current mapping without becoming stale.
  const sendersRef = useRef<Map<string, (msg: ClientMessage) => void>>(new Map())

  // Stable send function exposed to the store
  const send = useCallback((machineId: string, msg: ClientMessage) => {
    const fn = sendersRef.current.get(machineId)
    if (fn) {
      fn(msg)
    } else {
      console.warn(`[useDaemonConnections] no connection for machine ${machineId}`)
    }
  }, [])

  // Inject into store once
  useEffect(() => {
    setSend(send)
  }, [send, setSend])

  // Per-daemon URL → WebSocket lifecycle management.
  // We key by URL (not machineId) because we may not know the machineId
  // before the first message arrives from a newly-added daemon.
  const wsMap = useRef<Map<string, { ws: WebSocket; machineId: string; retry: number; unmounted: boolean }>>({} as never)
  if (!(wsMap.current instanceof Map)) {
    wsMap.current = new Map()
  }

  useEffect(() => {
    const daemonList = Object.values(daemons)

    // Close connections for URLs that are no longer in the registry
    for (const [url, entry] of wsMap.current.entries()) {
      if (!daemonList.find(d => d.url === url)) {
        entry.unmounted = true
        entry.ws.close()
        wsMap.current.delete(url)
        sendersRef.current.delete(entry.machineId)
      }
    }

    // Open connections for new URLs
    for (const daemon of daemonList) {
      if (wsMap.current.has(daemon.url)) continue

      const entry = { ws: null as unknown as WebSocket, machineId: daemon.id, retry: 1000, unmounted: false }
      wsMap.current.set(daemon.url, entry)

      function connect(url: string, daemonId: string) {
        if (entry.unmounted) return
        const ws = new WebSocket(url)
        entry.ws = ws

        // Register a send fn immediately using the known id; it may be
        // updated after the first server message if the id was a placeholder.
        sendersRef.current.set(daemonId, (msg: ClientMessage) => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(JSON.stringify(msg))
          }
        })

        ws.onopen = () => {
          if (entry.unmounted) { ws.close(); return }
          entry.retry = 1000
          setDaemonConnected(daemonId, true)
          ws.send(JSON.stringify({ type: 'list_projects' }))
          ws.send(JSON.stringify({ type: 'list_worktrees' }))
          ws.send(JSON.stringify({ type: 'list_peers' }))
        }

        ws.onmessage = (e: MessageEvent<string>) => {
          // On the very first message, learn the real machine_id from the
          // message itself and re-key the send fn if needed.
          try {
            const parsed = JSON.parse(e.data)
            if (parsed.machine_id && parsed.machine_id !== daemonId) {
              const realId = parsed.machine_id as string
              // Move send fn from placeholder id to real id
              const fn = sendersRef.current.get(daemonId)
              if (fn) {
                sendersRef.current.set(realId, fn)
                if (realId !== daemonId) sendersRef.current.delete(daemonId)
              }
              entry.machineId = realId
              // Update connection status on real id
              setDaemonConnected(realId, true)
              // NOTE: we don't remove the old daemon entry here —
              // that's handled by store.mergeDiscoveredPeers / addDaemon
              // on the next render if needed. The URL is the stable key.
            }
          } catch { /* ignored */ }
          handleServerMessage(e.data)
        }

        ws.onclose = () => {
          if (entry.unmounted) return
          setDaemonConnected(daemonId, false)
          const delay = entry.retry
          entry.retry = Math.min(delay * 2, 10_000)
          setTimeout(() => connect(url, daemonId), delay)
        }

        ws.onerror = () => { ws.close() }
      }

      connect(daemon.url, daemon.id)
    }

    return () => {
      // On unmount only — mark all as unmounted and close
      // (the outer useEffect cleanup runs when daemons changes, not on unmount)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(Object.keys(daemons).sort())])

  // Cleanup on component unmount
  useEffect(() => {
    return () => {
      for (const entry of wsMap.current.values()) {
        entry.unmounted = true
        entry.ws?.close()
      }
      wsMap.current.clear()
      sendersRef.current.clear()
    }
  }, [])
}
