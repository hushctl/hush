import { useEffect, useRef, useCallback } from 'react'
import { useStore } from '@/store'
import type { ClientMessage } from '@/lib/protocol'

const WS_URL = '/ws'

export function useWebSocket() {
  const setConnected = useStore(s => s.setConnected)
  const setSend = useStore(s => s.setSend)
  const handleServerMessage = useStore(s => s.handleServerMessage)
  const wsRef = useRef<WebSocket | null>(null)
  const retryDelay = useRef(1000)
  const unmounted = useRef(false)

  const send = useCallback((msg: ClientMessage) => {
    const ws = wsRef.current
    if (ws?.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(msg))
    }
  }, [])

  useEffect(() => {
    setSend(send)
  }, [send, setSend])

  useEffect(() => {
    unmounted.current = false

    function connect() {
      if (unmounted.current) return
      const ws = new WebSocket(WS_URL)
      wsRef.current = ws

      ws.onopen = () => {
        if (unmounted.current) { ws.close(); return }
        retryDelay.current = 1000
        setConnected(true)
        // Hydrate initial state from daemon
        ws.send(JSON.stringify({ type: 'list_projects' }))
        ws.send(JSON.stringify({ type: 'list_worktrees' }))
      }

      ws.onmessage = (e: MessageEvent<string>) => {
        handleServerMessage(e.data)
      }

      ws.onclose = () => {
        if (unmounted.current) return
        setConnected(false)
        const delay = retryDelay.current
        retryDelay.current = Math.min(delay * 2, 10_000)
        setTimeout(connect, delay)
      }

      ws.onerror = () => {
        ws.close()
      }
    }

    connect()

    return () => {
      unmounted.current = true
      wsRef.current?.close()
    }
  }, [setConnected, handleServerMessage])

  return send
}
