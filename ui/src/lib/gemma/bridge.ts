/**
 * Main-thread bridge to the Gemma Web Worker.
 *
 * Singleton: one worker, one model load, shared across the app.
 * Returns GemmaResult (string targets); caller resolves to IntentResult via resolveGemmaResult().
 */

import type { ClassifyCtx } from './worker'

export type ModelStatus = 'idle' | 'loading' | 'ready' | 'error'

/** Raw model output — targets are unresolved string names from the model */
export type GemmaResult =
  | { kind: 'back_to_grid' }
  | { kind: 'pull_up'; target: string }
  | { kind: 'close'; target: string }
  | { kind: 'show_needs_me' }
  | { kind: 'tree'; target: string }
  | { kind: 'new_worktree'; branch: string; project: string }
  | { kind: 'delete_worktree'; target: string }
  | { kind: 'inspect_daemon'; target: string }
  | { kind: 'unknown'; reason: string }

type ProgressCallback = (file: string, progress: number) => void
type StatusCallback = (status: ModelStatus, error?: string) => void

const pending = new Map<string, { resolve: (r: GemmaResult) => void; reject: (e: Error) => void }>()
let reqCounter = 0
let worker: Worker | null = null
let statusCb: StatusCallback | null = null
let progressCb: ProgressCallback | null = null
let currentStatus: ModelStatus = 'idle'

function getWorker(): Worker {
  if (worker) return worker
  worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' })
  worker.addEventListener('message', (e: MessageEvent) => {
    const msg = e.data
    switch (msg.type) {
      case 'progress':
        progressCb?.(msg.file as string, msg.progress as number)
        break
      case 'ready':
        currentStatus = 'ready'
        statusCb?.('ready')
        break
      case 'error':
        currentStatus = 'error'
        statusCb?.('error', msg.message as string)
        break
      case 'result': {
        const entry = pending.get(msg.id as string)
        if (entry) { pending.delete(msg.id as string); entry.resolve(parseRaw(msg.raw as string)) }
        break
      }
      case 'result_error': {
        const entry = pending.get(msg.id as string)
        if (entry) { pending.delete(msg.id as string); entry.reject(new Error(msg.message as string)) }
        break
      }
    }
  })
  return worker
}

export function startModelLoad(onStatus: StatusCallback, onProgress: ProgressCallback): void {
  if (currentStatus !== 'idle') return
  currentStatus = 'loading'
  statusCb = onStatus
  progressCb = onProgress
  onStatus('loading')
  getWorker().postMessage({ type: 'load' })
}

export function getModelStatus(): ModelStatus {
  return currentStatus
}

export async function classify(text: string, ctx: ClassifyCtx): Promise<GemmaResult> {
  if (currentStatus !== 'ready') throw new Error('model not ready')
  const id = String(reqCounter++)
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject })
    getWorker().postMessage({ type: 'classify', id, text, ctx })
  })
}

function parseRaw(raw: string): GemmaResult {
  const match = raw.match(/\{[^{}]+\}/)
  if (!match) return { kind: 'unknown', reason: `no JSON in: ${raw.slice(0, 80)}` }
  let parsed: Record<string, string>
  try { parsed = JSON.parse(match[0]) }
  catch { return { kind: 'unknown', reason: `bad JSON: ${match[0]}` } }

  switch (parsed.kind) {
    case 'back_to_grid':  return { kind: 'back_to_grid' }
    case 'show_needs_me': return { kind: 'show_needs_me' }
    case 'pull_up':       return { kind: 'pull_up', target: parsed.target ?? '' }
    case 'close':         return { kind: 'close', target: parsed.target ?? '' }
    case 'tree':          return { kind: 'tree', target: parsed.target ?? '' }
    case 'new_worktree':  return { kind: 'new_worktree', branch: parsed.branch ?? '', project: parsed.project ?? '' }
    case 'delete_worktree': return { kind: 'delete_worktree', target: parsed.target ?? '' }
    case 'inspect_daemon':return { kind: 'inspect_daemon', target: parsed.target ?? '' }
    default:              return { kind: 'unknown', reason: `unknown kind: ${parsed.kind}` }
  }
}
