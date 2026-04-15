#!/usr/bin/env node
/**
 * Record a demo GIF of Hush for the README.
 *
 * Spins up a test daemon, seeds it with fake projects, drives the browser
 * through key UX moments, and records a video. Converts to GIF via ffmpeg.
 *
 * Usage:
 *   node scripts/record-demo.mjs
 *
 * Output:
 *   docs/demo.gif
 *
 * Requires: playwright browsers installed (`npx playwright install chromium`),
 *           ffmpeg on PATH.
 */

import { spawn, execSync } from 'child_process'
import { mkdirSync, writeFileSync, rmSync, existsSync, readdirSync } from 'fs'
import { createRequire } from 'module'
import path from 'path'
import os from 'os'

// Resolve deps from their respective node_modules
const uiRequire = createRequire(path.resolve('ui/package.json'))
const testRequire = createRequire(path.resolve('tests/package.json'))
const { chromium } = uiRequire('@playwright/test')
const { WebSocket } = testRequire('ws')

// ─── Config ──────────────────────────────────────────────────────────────────

const PORT = 19000 + (process.pid % 1000)
const VITE_PORT = 15173 + (process.pid % 100)
const DAEMON_URL = `wss://localhost:${PORT}/ws`
const UI_URL = `http://localhost:${VITE_PORT}`
const TEST_DIR = path.join(os.tmpdir(), `hush-demo-${Date.now()}`)
const STATE_FILE = path.join(TEST_DIR, 'state.json')
const DAEMON_BIN = path.resolve('daemon/target/debug/hush')
const VIDEO_DIR = path.join(TEST_DIR, 'video')
const OUTPUT_GIF = path.resolve('docs/demo.gif')

// ─── Helpers ─────────────────────────────────────────────────────────────────

function sleep(ms) {
  return new Promise(r => setTimeout(r, ms))
}

function makeTestRepo(name) {
  const dir = path.join(os.tmpdir(), `hush-demo-repo-${name}-${Date.now()}`)
  mkdirSync(dir, { recursive: true })
  execSync('git init -b main', { cwd: dir, stdio: 'pipe' })
  execSync('git config user.email "demo@hush.dev"', { cwd: dir, stdio: 'pipe' })
  execSync('git config user.name "Demo"', { cwd: dir, stdio: 'pipe' })
  writeFileSync(path.join(dir, 'README.md'), `# ${name}\n`)
  execSync('git add . && git commit -m "init"', { cwd: dir, stdio: 'pipe' })
  return dir
}

function startDaemon() {
  return new Promise((resolve, reject) => {
    const proc = spawn(DAEMON_BIN, ['--port', String(PORT), '--state-file', STATE_FILE], {
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    proc.stderr.on('data', () => {})
    proc.stdout.on('data', () => {})

    let attempts = 0
    const check = setInterval(async () => {
      attempts++
      if (attempts > 30) {
        clearInterval(check)
        reject(new Error('Daemon did not start'))
        return
      }
      try {
        const ws = new WebSocket(DAEMON_URL, { rejectUnauthorized: false })
        ws.once('open', () => { ws.close(); clearInterval(check); resolve(proc) })
        ws.once('error', () => {})
      } catch {}
    }, 200)
  })
}

function wsSend(ws, obj) {
  return new Promise((resolve) => {
    ws.send(JSON.stringify(obj), resolve)
  })
}

function wsWaitFor(ws, predicate, timeoutMs = 10000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('ws timeout')), timeoutMs)
    function onMsg(raw) {
      const msg = JSON.parse(raw)
      if (predicate(msg)) {
        clearTimeout(timer)
        ws.off('message', onMsg)
        resolve(msg)
      }
    }
    ws.on('message', onMsg)
  })
}

// ─── Main ────────────────────────────────────────────────────────────────────

mkdirSync(TEST_DIR, { recursive: true })
mkdirSync(VIDEO_DIR, { recursive: true })

const repos = []
let daemon
let viteProc
let ws

try {
  console.log('Starting demo daemon...')
  daemon = await startDaemon()

  // Start Vite dev server (exposes __MC_STORE__ for programmatic control)
  console.log('Starting Vite dev server...')
  viteProc = spawn('npx', ['vite', '--port', String(VITE_PORT), '--strictPort'], {
    cwd: path.resolve('ui'),
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  viteProc.stderr.on('data', () => {})
  // Wait for Vite to be ready
  for (let i = 0; i < 30; i++) {
    await sleep(500)
    try {
      const res = await fetch(UI_URL)
      if (res.ok) break
    } catch {}
  }

  // Seed projects via WebSocket
  ws = new WebSocket(DAEMON_URL, { rejectUnauthorized: false })
  await new Promise((res, rej) => { ws.once('open', res); ws.once('error', rej) })

  const projectNames = ['api-gateway', 'web-frontend', 'ml-pipeline']
  const worktreeIds = []

  for (const name of projectNames) {
    const repo = makeTestRepo(name)
    repos.push(repo)

    // Register project
    const p1 = wsWaitFor(ws, msg => msg.type === 'project_list')
    await wsSend(ws, { type: 'register_project', path: repo, name })
    const pl = await p1
    const proj = pl.projects.find(p => p.name === name)

    // Create worktree
    const p2 = wsWaitFor(ws, msg => msg.type === 'worktree_list')
    await wsSend(ws, {
      type: 'create_worktree',
      project_id: proj.id,
      branch: 'main',
      permission_mode: 'plan',
    })
    const wl = await p2
    const wt = wl.worktrees.find(w => w.project_id === proj.id)
    worktreeIds.push(wt.id)
    console.log(`  Seeded: ${name} → ${wt.id}`)
  }

  ws.close()
  await sleep(500)

  // ── Record browser session ─────────────────────────────────────────────

  console.log('Launching browser...')
  const browser = await chromium.launch({ headless: true })
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
    ignoreHTTPSErrors: true,
    recordVideo: { dir: VIDEO_DIR, size: { width: 1280, height: 720 } },
  })
  const page = await context.newPage()

  // The UI defaults to wss://localhost:9111/ws which would connect to the
  // user's real daemon. Pre-seed localStorage so the store only knows about
  // our test daemon on the throwaway port.
  await page.goto(UI_URL)
  await page.evaluate((daemonUrl) => {
    const state = {
      daemons: {
        demo: { id: 'demo', name: 'demo', url: daemonUrl, connected: false },
      },
      layoutMode: 'grid',
      canvas: { panels: [], nextZ: 0, autoTidy: true },
      activePanes: [],
      tileMode: '1-up',
      selectedWorktreeId: null,
      selectedProjectId: null,
    }
    localStorage.setItem('mc-ui-prefs', JSON.stringify({ state, version: 2 }))
  }, DAEMON_URL)
  // Reload so the store picks up the seeded localStorage
  await page.reload({ waitUntil: 'networkidle' })
  // Wait for WebSocket connection to establish
  await page.waitForTimeout(2000)
  // Debug: check what the store sees
  const storeDebug = await page.evaluate(() => {
    const store = window.__MC_STORE__?.getState()
    if (!store) return { error: 'no store' }
    return {
      daemons: Object.entries(store.daemons).map(([k, v]) => ({ id: k, name: v.name, url: v.url, connected: v.connected })),
      projects: Object.keys(store.projects),
      worktrees: Object.keys(store.worktrees),
    }
  })
  console.log('Store state:', JSON.stringify(storeDebug, null, 2))
  await page.waitForTimeout(1000)

  // ── Scene 1: Grid view with dots ────────────────────────────────────────
  console.log('Scene 1: Grid view with project dots...')
  await page.waitForTimeout(2000)

  // ── Scene 2: Open a terminal pane via store (dots use SVG overlay) ──────
  console.log('Scene 2: Opening terminal pane...')
  await page.evaluate(() => {
    const store = window.__MC_STORE__?.getState()
    if (!store) return
    const ids = Object.keys(store.worktrees)
    if (ids.length > 0) store.openPane(ids[0])
  })
  await page.waitForTimeout(3000)

  // ── Scene 3: Back to grid via command bar ───────────────────────────────
  console.log('Scene 3: Command bar — back to grid...')
  const cmdBar = page.getByTestId('command-bar-input')
  if (await cmdBar.isVisible({ timeout: 3000 }).catch(() => false)) {
    await cmdBar.click()
    await cmdBar.pressSequentially('back to grid', { delay: 80 })
    await page.waitForTimeout(500)
    await cmdBar.press('Enter')
    await page.waitForTimeout(2000)
  }

  // ── Scene 4: Open two panes side by side ───────────────────────────────
  console.log('Scene 4: Two panes side by side...')
  await page.evaluate(() => {
    const store = window.__MC_STORE__?.getState()
    if (!store) return
    const ids = Object.keys(store.worktrees)
    if (ids.length >= 2) {
      store.openPane(ids[0])
      store.openPane(ids[1])
    }
  })
  await page.waitForTimeout(3000)

  // ── Scene 5: Back to grid (final) ──────────────────────────────────────
  console.log('Scene 5: Final grid view...')
  await page.evaluate(() => {
    window.__MC_STORE__?.getState()?.switchToGrid()
  })
  await page.waitForTimeout(1500)

  // Close browser — this finalizes the video
  await page.close()
  await context.close()
  await browser.close()

  // ── Convert to GIF ─────────────────────────────────────────────────────

  // Find the recorded video (playwright names it randomly)
  const videos = readdirSync(VIDEO_DIR).filter(f => f.endsWith('.webm'))
  if (videos.length === 0) {
    console.error('No video recorded!')
    process.exit(1)
  }
  const videoPath = path.join(VIDEO_DIR, videos[0])
  console.log(`Video: ${videoPath}`)

  // Check for ffmpeg
  try {
    execSync('which ffmpeg', { stdio: 'pipe' })
  } catch {
    console.log(`\nNo ffmpeg found. Convert manually:`)
    console.log(`  ffmpeg -i ${videoPath} -vf "fps=12,scale=960:-1:flags=lanczos" -loop 0 ${OUTPUT_GIF}`)
    process.exit(0)
  }

  console.log('Converting to GIF...')
  mkdirSync(path.dirname(OUTPUT_GIF), { recursive: true })

  // Two-pass for better quality: generate palette first, then use it
  const palette = path.join(TEST_DIR, 'palette.png')
  execSync(
    `ffmpeg -y -i "${videoPath}" -vf "fps=12,scale=960:-1:flags=lanczos,palettegen=stats_mode=diff" "${palette}"`,
    { stdio: 'pipe' },
  )
  execSync(
    `ffmpeg -y -i "${videoPath}" -i "${palette}" -lavfi "fps=12,scale=960:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3" -loop 0 "${OUTPUT_GIF}"`,
    { stdio: 'pipe' },
  )

  console.log(`\nDemo GIF written to: ${OUTPUT_GIF}`)

} finally {
  if (ws && ws.readyState === WebSocket.OPEN) ws.close()
  if (viteProc) viteProc.kill('SIGTERM')
  if (daemon) {
    daemon.kill('SIGTERM')
    await sleep(500)
  }
  for (const r of repos) {
    try { rmSync(r, { recursive: true, force: true }) } catch {}
  }
  try { rmSync(TEST_DIR, { recursive: true, force: true }) } catch {}
}
