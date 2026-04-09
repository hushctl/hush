/**
 * Web Worker — runs Gemma 4 E2B (ONNX, WebGPU) off the main thread.
 *
 * Message protocol:
 *   in:  { type: 'load' }
 *   in:  { type: 'classify', id: string, text: string, ctx: ClassifyCtx }
 *   out: { type: 'progress', file: string, progress: number }
 *   out: { type: 'ready' }
 *   out: { type: 'error', message: string }
 *   out: { type: 'result', id: string, raw: string }
 *   out: { type: 'result_error', id: string, message: string }
 */

import { pipeline, env } from '@huggingface/transformers'

// Use browser cache so second load is instant
env.useBrowserCache = true
env.allowLocalModels = false

export type ClassifyCtx = {
  projects: string[]
  daemons: string[]
}

type InMsg =
  | { type: 'load' }
  | { type: 'classify'; id: string; text: string; ctx: ClassifyCtx }

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let generator: any = null

self.addEventListener('message', async (e: MessageEvent<InMsg>) => {
  const msg = e.data

  if (msg.type === 'load') {
    try {
      generator = await pipeline(
        'text-generation',
        'onnx-community/gemma-4-E2B-it-ONNX',
        {
          device: 'webgpu',
          dtype: 'q4',
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          progress_callback: (info: any) => {
            if (info?.status === 'progress' && typeof info.progress === 'number') {
              self.postMessage({
                type: 'progress',
                file: info.file ?? '',
                progress: Math.round(info.progress),
              })
            }
          },
        },
      )
      self.postMessage({ type: 'ready' })
    } catch (err) {
      self.postMessage({ type: 'error', message: String(err) })
    }
    return
  }

  if (msg.type === 'classify') {
    if (!generator) {
      self.postMessage({ type: 'result_error', id: msg.id, message: 'model not loaded' })
      return
    }
    try {
      const { id, text, ctx } = msg
      const messages = [{ role: 'user', content: buildPrompt(text, ctx) }]
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const output = await generator(messages as any, {
        max_new_tokens: 60,
        do_sample: false,
      })
      // transformers.js returns the full conversation; last assistant turn is what we want
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const turns: any[] = output?.[0]?.generated_text ?? []
      const last = turns[turns.length - 1]
      const raw: string = last?.content ?? last?.generated_text ?? String(output)
      self.postMessage({ type: 'result', id, raw })
    } catch (err) {
      self.postMessage({ type: 'result_error', id: msg.id, message: String(err) })
    }
  }
})

function buildPrompt(text: string, ctx: ClassifyCtx): string {
  const projects = ctx.projects.length > 0 ? ctx.projects.join(', ') : 'none'
  const daemons = ctx.daemons.length > 0 ? ctx.daemons.join(', ') : 'none'

  return `Classify this developer workspace command. Reply with ONLY a single JSON object, nothing else.

Intent options:
- {"kind":"back_to_grid"} — go back to grid, close all panes
- {"kind":"pull_up","target":"<name>"} — open/show a project terminal
- {"kind":"close","target":"<name>"} — close a project pane
- {"kind":"show_needs_me"} — show what needs attention
- {"kind":"tree","target":"<name>"} — open project file tree
- {"kind":"new_worktree","branch":"<branch>","project":"<name>"} — create git worktree
- {"kind":"inspect_daemon","target":"<name>"} — view daemon details
- {"kind":"unknown"} — cannot classify

Available projects: ${projects}
Available daemons: ${daemons}

Command: "${text}"

JSON:`
}
