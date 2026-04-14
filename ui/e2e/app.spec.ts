/**
 * Mission Control — Playwright acceptance tests
 *
 * Updated for the terminal-embed architecture: chat components are gone,
 * the embedded xterm.js TerminalPane is the conversation surface, and the
 * CommandBar is a workspace-intent layer (not a message router).
 *
 * The daemon is started by playwright.config.ts webServer config and persists
 * state between tests (reuseExistingServer: true).
 */

import { test, expect, type Page } from '@playwright/test'
import path from 'path'
import os from 'os'
import fs from 'fs'
import { execSync } from 'child_process'

/**
 * After each test, scrub any `mc-e2e-*` projects from the daemon and delete
 * the underlying tmp repos. This keeps the shared daemon's `state.json` clean
 * across runs — without it, every CI iteration leaves dangling worktrees that
 * the daemon tries to respawn on restart and that clutter the dot grid.
 *
 * We match by project path prefix (`<tmpdir>/mc-e2e-`) rather than tracking
 * created dirs in a module-level set, so a failed test that aborts before
 * returning a path still gets cleaned up.
 */
test.afterEach(async ({ page }) => {
  const prefix = path.join(os.tmpdir(), 'mc-e2e-')
  // Collect worktree IDs owned by e2e projects straight from the store.
  const victims = await page.evaluate((pfx) => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const st = (window as any).__MC_STORE__?.getState()
    if (!st) return { wtIds: [] as string[], paths: [] as string[] }
    const projects = st.projects ?? {}
    const worktrees = st.worktrees ?? {}
    const e2eProjectIds = new Set(
      Object.values(projects)
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        .filter((p: any) => typeof p?.path === 'string' && p.path.startsWith(pfx))
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        .map((p: any) => p.id),
    )
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const paths = Object.values(projects)
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .filter((p: any) => typeof p?.path === 'string' && p.path.startsWith(pfx))
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .map((p: any) => p.path as string)
    const wtIds = Object.values(worktrees)
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .filter((w: any) => e2eProjectIds.has(w?.project_id))
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .map((w: any) => w.id as string)
    return { wtIds, paths }
  }, prefix).catch(() => ({ wtIds: [] as string[], paths: [] as string[] }))

  // Ask the daemon to drop each worktree. `remove_worktree` cascades to
  // remove the parent project when its last worktree is gone.
  if (victims.wtIds.length > 0) {
    await page.evaluate((ids) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const st = (window as any).__MC_STORE__?.getState()
      if (!st) return
      for (const id of ids) {
        // IDs in the store are namespaced `${machineId}:${rawId}`.
        const colon = id.indexOf(':')
        const machineId = colon >= 0 ? id.slice(0, colon) : 'localhost'
        const rawId = colon >= 0 ? id.slice(colon + 1) : id
        st.send(machineId, { type: 'remove_worktree', worktree_id: rawId })
      }
    }, victims.wtIds)
    // Give the daemon a beat to process deletes and broadcast list updates.
    await page.waitForTimeout(500)
  }

  // Remove the on-disk tmp repos regardless of daemon state.
  for (const p of victims.paths) {
    try { fs.rmSync(p, { recursive: true, force: true }) } catch { /* best effort */ }
  }
  // Also sweep any orphan mc-e2e-* dirs left by aborted tests.
  try {
    for (const entry of fs.readdirSync(os.tmpdir())) {
      if (!entry.startsWith('mc-e2e-')) continue
      try { fs.rmSync(path.join(os.tmpdir(), entry), { recursive: true, force: true }) } catch { /* ignore */ }
    }
  } catch { /* ignore */ }
})

// ─── Helpers ─────────────────────────────────────────────────────────────────

function collectErrors(page: Page) {
  const errors: string[] = []
  page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()) })
  page.on('pageerror', err => errors.push(err.message))
  return errors
}

function makeTestRepo(): string {
  const dir = path.join(os.tmpdir(), `mc-e2e-${Date.now()}`)
  fs.mkdirSync(dir, { recursive: true })
  execSync('git init -b main', { cwd: dir, stdio: 'pipe' })
  execSync('git config user.email "e2e@test.com"', { cwd: dir, stdio: 'pipe' })
  execSync('git config user.name "E2E"', { cwd: dir, stdio: 'pipe' })
  fs.writeFileSync(path.join(dir, 'README.md'), '# E2E Test\n')
  execSync('git add . && git commit -m "init"', { cwd: dir, stdio: 'pipe' })
  return dir
}

/**
 * Register a project + worktree via the command bar setup flow. The setup
 * flow still exists as the onboarding shortcut — it's not part of the intent
 * verbs, just a UX affordance for first-time setup.
 */
async function registerProjectAndWorktree(page: Page, repoPath: string, branch = 'main') {
  await page.getByTestId('add-project-btn').click()
  await page.getByTestId('setup-path-input').fill(repoPath)
  await page.getByTestId('setup-path-input').press('Enter')
  await expect(page.getByTestId('setup-branch-input')).toBeVisible({ timeout: 5000 })
  await page.getByTestId('setup-branch-input').fill(branch)
  await page.getByTestId('setup-branch-input').press('Enter')
  // Wait for the new worktree to land in the store via worktree_list broadcast
  await expect.poll(
    () => page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const state = (window as any).__MC_STORE__?.getState()
      return Object.keys(state?.worktrees ?? {}).length
    }),
    { timeout: 8000 },
  ).toBeGreaterThan(0)
}

/** Clear persisted UI state and reload. Waits for WS connection. */
async function freshLoad(page: Page) {
  await page.goto('/')
  await page.evaluate(() => localStorage.removeItem('mc-ui-prefs'))
  await page.reload({ waitUntil: 'networkidle' })
  await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
}

/** Open a pane via the zustand store. More reliable than DOM clicks. */
async function openFirstPane(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const store = (window as any).__MC_STORE__
    if (!store) return false
    const state = store.getState()
    if (!state.connected) return false
    const ids = Object.keys(state.worktrees)
    if (ids.length === 0) return false
    state.openPane(ids[0])
    return true
  })
}

async function hasWorktreeDots(page: Page): Promise<boolean> {
  return (await page.locator('[data-testid^="worktree-dot-"]').count()) > 0
}

async function firstWorktreeDotId(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const state = (window as any).__MC_STORE__?.getState()
    return Object.keys(state?.worktrees ?? {})[0] ?? null
  })
}

async function firstProjectId(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const state = (window as any).__MC_STORE__?.getState()
    return Object.keys(state?.projects ?? {})[0] ?? null
  })
}

async function isConnectedWithWorktrees(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const store = (window as any).__MC_STORE__
    if (!store) return false
    const state = store.getState()
    return state.connected && Object.keys(state.worktrees).length > 0
  })
}

// ─── Section 0: Onboarding ───────────────────────────────────────────────────

test.describe('0. Onboarding', () => {
  test('0.1 disconnected/connected screen has correct elements', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    const disconnected = page.getByTestId('disconnected-screen')
    const grid = page.getByTestId('dot-grid')
    await expect(disconnected.or(grid)).toBeVisible({ timeout: 5000 })
    if (await disconnected.isVisible()) {
      await expect(disconnected).toContainText('no daemon connected')
    }
    expect(errors).toHaveLength(0)
  })

  test('0.2 daemon connects — dot grid becomes visible', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    expect(errors).toHaveLength(0)
  })
})

// ─── Section 1: Dot grid ─────────────────────────────────────────────────────

test.describe('1. Dot grid', () => {
  let testRepo: string
  test.beforeAll(() => { testRepo = makeTestRepo() })
  test.afterAll(() => { fs.rmSync(testRepo, { recursive: true, force: true }) })

  test('1.1 SVG fills the viewport area', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    const svg = page.getByTestId('dot-grid')
    await expect(svg).toBeVisible({ timeout: 5000 })
    const box = await svg.boundingBox()
    expect(box).not.toBeNull()
    expect(box!.width).toBeGreaterThan(400)
    expect(box!.height).toBeGreaterThan(200)
    expect(errors).toHaveLength(0)
  })

  test('1.2 no bold text on grid screen', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    const hasBold = await page.evaluate(() => {
      for (const el of document.querySelectorAll('*')) {
        const fw = parseInt(window.getComputedStyle(el).fontWeight, 10)
        if (fw > 400 && el.children.length === 0 && el.textContent?.trim()) return true
      }
      return false
    })
    expect(hasBold).toBe(false)
  })

  test('1.3 worktree dot has correct status data attribute', async ({ page }) => {
    const errors = collectErrors(page)
    await freshLoad(page)
    await registerProjectAndWorktree(page, testRepo)
    await page.waitForTimeout(500)

    const dot = page.locator('[data-testid^="worktree-dot-"]').first()
    await expect(dot).toBeVisible({ timeout: 5000 })
    const status = await dot.getAttribute('data-status')
    expect(status).toBeTruthy()
    expect(['idle', 'running', 'needs_you'].some(s => status?.startsWith(s) || status?.startsWith('failed'))).toBe(true)
    expect(errors).toHaveLength(0)
  })

  test('1.5 SVG text labels use textAnchor=end (labels left of dot)', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    const textWithEnd = await page.evaluate(() =>
      Array.from(document.querySelectorAll('svg text'))
        .some(t => t.getAttribute('text-anchor') === 'end')
    )
    expect(textWithEnd).toBe(true)
  })

  test('1.6 hovering a dot shows the detail card', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const wtId = await firstWorktreeDotId(page)
    if (!wtId) { test.skip(); return }

    await page.locator(`[data-testid="worktree-dot-${wtId}"]`).hover({ force: true })
    await expect(page.getByTestId('detail-card')).toBeVisible({ timeout: 2000 })
    await page.mouse.move(0, 0)
    await expect(page.getByTestId('detail-card')).not.toBeVisible({ timeout: 2000 })
    expect(errors).toHaveLength(0)
  })

  test('1.7 reboarding bar appears when worktrees exist', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (await hasWorktreeDots(page)) {
      await expect(page.getByTestId('reboarding-bar')).toBeVisible()
      const text = await page.getByTestId('reboarding-bar').textContent()
      expect(text!.length).toBeGreaterThan(0)
    }
    expect(errors).toHaveLength(0)
  })

  test('1.8 dot click opens pane (terminal pane appears)', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const wtId = await firstWorktreeDotId(page)
    if (!wtId) { test.skip(); return }

    await page.locator(`[data-testid="worktree-dot-${wtId}"]`).click({ force: true })
    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })
    await expect(page.getByTestId(`terminal-pane-${wtId}`)).toBeVisible({ timeout: 5000 })
    expect(errors).toHaveLength(0)
  })
})

// ─── Section 2: Command bar — workspace intent ──────────────────────────────

test.describe('2. Command bar (intent verbs)', () => {
  test('2.1 + project button is visible', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await page.waitForLoadState('networkidle')
    await expect(page.getByTestId('add-project-btn')).toBeVisible()
    expect(errors).toHaveLength(0)
  })

  test('2.2 setup flow opens on + project click', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await page.waitForLoadState('networkidle')
    await page.getByTestId('add-project-btn').click()
    await expect(page.getByTestId('setup-flow')).toBeVisible()
    await expect(page.getByTestId('setup-path-input')).toBeVisible()
    expect(errors).toHaveLength(0)
  })

  test('2.3 register project → worktree creation flow', async ({ page }) => {
    const errors = collectErrors(page)
    const repo = makeTestRepo()
    try {
      await freshLoad(page)
      await registerProjectAndWorktree(page, repo)
      // After registration, dot for the new worktree appears
      await expect(page.locator('[data-testid^="worktree-dot-"]').first()).toBeVisible({ timeout: 5000 })
      expect(errors).toHaveLength(0)
    } finally {
      fs.rmSync(repo, { recursive: true, force: true })
    }
  })

  test('2.4 unknown verb shows hint', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('command-input')).toBeVisible({ timeout: 5000 })
    await page.getByTestId('command-input').fill('asdfqwerty zzz')
    await page.getByTestId('command-input').press('Enter')
    await expect(page.getByTestId('command-hint')).toBeVisible({ timeout: 2000 })
    expect(errors).toHaveLength(0)
  })

  test('2.5 "back to grid" verb returns from pane view to grid', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }

    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }
    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })

    await page.getByTestId('command-input').fill('back to grid')
    await page.getByTestId('command-input').press('Enter')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 3000 })
    expect(errors).toHaveLength(0)
  })

  test('2.6 "pull up <project>" opens a terminal pane', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const projectName = await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const state = (window as any).__MC_STORE__?.getState()
      const proj = Object.values(state?.projects ?? {})[0] as { name: string } | undefined
      return proj?.name ?? null
    })
    if (!projectName) { test.skip(); return }

    await page.getByTestId('command-input').fill(`pull up ${projectName}`)
    await page.getByTestId('command-input').press('Enter')
    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })
    expect(errors).toHaveLength(0)
  })
})

// ─── Section 3: Pane view (terminal embed) ──────────────────────────────────

test.describe('3. Pane view', () => {
  test('3.1 opening a pane shows pane UI + top bar + status pill', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }
    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }

    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })
    await expect(page.getByTestId('top-bar')).toBeVisible()
    await expect(page.getByTestId('pane-header').first()).toBeVisible()
    await expect(page.getByTestId('status-pill').first()).toBeVisible({ timeout: 5000 })
    expect(errors).toHaveLength(0)
  })

  test('3.2 pane mounts a terminal pane (xterm.js)', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }

    const wtId = await firstWorktreeDotId(page)
    if (!wtId) { test.skip(); return }
    await page.evaluate((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ;(window as any).__MC_STORE__.getState().openPane(id)
    }, wtId)

    await expect(page.getByTestId(`terminal-pane-${wtId}`)).toBeVisible({ timeout: 5000 })
    // xterm renders an .xterm element inside the container
    const xtermInside = await page.locator(`[data-testid="terminal-pane-${wtId}"] .xterm`).count()
    expect(xtermInside).toBeGreaterThan(0)
    expect(errors).toHaveLength(0)
  })

  test('3.3 grid button returns from pane view to dot grid', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }
    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }
    await expect(page.getByTestId('top-bar')).toBeVisible({ timeout: 5000 })
    await page.getByTestId('grid-btn').click()
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 3000 })
    expect(errors).toHaveLength(0)
  })

  test('3.4 top bar shows worktree indicators in pane view', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }
    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }
    await expect(page.getByTestId('top-bar')).toBeVisible({ timeout: 5000 })
    await expect(page.locator('[data-testid^="top-bar-wt-"]').first()).toBeVisible({ timeout: 3000 })
    expect(errors).toHaveLength(0)
  })

  /**
   * Regression pin for the MCP-auth Enter bug.
   *
   * When we enabled TERM_PROGRAM=vscode on the daemon's pty (to get Shift+Enter
   * in Claude Code), xterm.js flipped into kitty keyboard mode and started
   * sending \x1b[13u for Enter. Claude Code's MCP-auth Ink prompt only
   * recognises \r, so Enter silently did nothing. This test pins the fix by
   * asserting that a plain Enter keypress in a focused terminal pane produces
   * exactly one pty_input frame carrying "\r" — not "\x1b[13u", and not "\n".
   * Esc is checked alongside to catch the symmetric failure mode.
   */
  test('3.5 terminal Enter sends CR, not kitty sequence (MCP auth regression)', async ({ page }) => {
    const errors = collectErrors(page)

    // Patch WebSocket.send before app code runs so we see every outgoing frame.
    await page.addInitScript(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ;(window as any).__WS_SENT = [] as string[]
      const origSend = WebSocket.prototype.send
      WebSocket.prototype.send = function (data: string | ArrayBufferLike | Blob | ArrayBufferView) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        if (typeof data === 'string') (window as any).__WS_SENT.push(data)
        return origSend.call(this, data as never)
      }
    })

    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    // Wait for the daemon WebSocket to connect.
    await expect.poll(
      () => page.evaluate(() => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const st = (window as any).__MC_STORE__?.getState()
        return Object.values(st?.daemons ?? {}).some((d: any) => d?.connected)
      }),
      { timeout: 10000 },
    ).toBe(true)

    // Ensure there's at least one worktree — register one if the daemon is empty.
    const wtCount = await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return Object.keys((window as any).__MC_STORE__?.getState()?.worktrees ?? {}).length
    })
    if (wtCount === 0) {
      const repo = makeTestRepo()
      await registerProjectAndWorktree(page, repo, 'main')
    }

    const wtId = await firstWorktreeDotId(page)
    if (!wtId) { test.skip(); return }
    await page.evaluate((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ;(window as any).__MC_STORE__.getState().openPane(id)
    }, wtId)
    const paneLocator = page.getByTestId(`terminal-pane-${wtId}`)
    await expect(paneLocator).toBeVisible({ timeout: 5000 })
    await expect(paneLocator.locator('.xterm')).toBeVisible({ timeout: 5000 })

    // Clear frames from attach/scrollback so we only inspect keypress output.
    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ;(window as any).__WS_SENT = []
    })

    // xterm's hidden textarea is the actual keyboard target.
    const textarea = paneLocator.locator('.xterm-helper-textarea')
    await textarea.focus()
    await page.keyboard.press('Enter')
    await page.keyboard.press('Escape')

    // Poll until both frames land — WS send is async relative to the keypress.
    const inputs = await page.evaluate(async () => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const sent = (): string[] => (window as any).__WS_SENT
      const deadline = Date.now() + 2000
      while (Date.now() < deadline) {
        const msgs = sent()
          .map(s => { try { return JSON.parse(s) } catch { return null } })
          .filter((m): m is { type: string; data?: string } => !!m && m.type === 'pty_input')
        if (msgs.length >= 2) return msgs.map(m => m.data ?? '')
        await new Promise(r => setTimeout(r, 50))
      }
      return sent()
        .map(s => { try { return JSON.parse(s) } catch { return null } })
        .filter((m): m is { type: string; data?: string } => !!m && m.type === 'pty_input')
        .map(m => m.data ?? '')
    })

    expect(inputs).toContain('\r')
    expect(inputs).toContain('\x1b')
    // Regression guards: kitty Enter and bare \n must never appear.
    expect(inputs).not.toContain('\x1b[13u')
    expect(inputs).not.toContain('\n')
    expect(errors).toHaveLength(0)
  })

  /**
   * Shift+Enter must send the kitty keyboard protocol sequence \x1b[13;2u so
   * Claude Code's prompt treats it as a soft newline. Without our custom key
   * handler, xterm.js maps Shift+Enter to plain \r — indistinguishable from
   * Enter — and multi-line input silently never triggers.
   */
  test('3.6 terminal Shift+Enter sends kitty sequence, not \\r', async ({ page }) => {
    const errors = collectErrors(page)

    await page.addInitScript(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ;(window as any).__WS_SENT = [] as string[]
      const origSend = WebSocket.prototype.send
      WebSocket.prototype.send = function (data: string | ArrayBufferLike | Blob | ArrayBufferView) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        if (typeof data === 'string') (window as any).__WS_SENT.push(data)
        return origSend.call(this, data as never)
      }
    })

    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    await expect.poll(
      () => page.evaluate(() => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const st = (window as any).__MC_STORE__?.getState()
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        return Object.values(st?.daemons ?? {}).some((d: any) => d?.connected)
      }),
      { timeout: 10000 },
    ).toBe(true)

    const wtCount = await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return Object.keys((window as any).__MC_STORE__?.getState()?.worktrees ?? {}).length
    })
    if (wtCount === 0) {
      const repo = makeTestRepo()
      await registerProjectAndWorktree(page, repo, 'main')
    }

    const wtId = await firstWorktreeDotId(page)
    if (!wtId) { test.skip(); return }
    await page.evaluate((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ;(window as any).__MC_STORE__.getState().openPane(id)
    }, wtId)
    const paneLocator = page.getByTestId(`terminal-pane-${wtId}`)
    await expect(paneLocator).toBeVisible({ timeout: 5000 })
    await expect(paneLocator.locator('.xterm')).toBeVisible({ timeout: 5000 })

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ;(window as any).__WS_SENT = []
    })

    const textarea = paneLocator.locator('.xterm-helper-textarea')
    await textarea.focus()
    await page.keyboard.press('Shift+Enter')

    const inputs = await page.evaluate(async () => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const sent = (): string[] => (window as any).__WS_SENT
      const deadline = Date.now() + 2000
      while (Date.now() < deadline) {
        const msgs = sent()
          .map(s => { try { return JSON.parse(s) } catch { return null } })
          .filter((m): m is { type: string; data?: string } => !!m && m.type === 'pty_input')
        if (msgs.length >= 1) return msgs.map(m => m.data ?? '')
        await new Promise(r => setTimeout(r, 50))
      }
      return sent()
        .map(s => { try { return JSON.parse(s) } catch { return null } })
        .filter((m): m is { type: string; data?: string } => !!m && m.type === 'pty_input')
        .map(m => m.data ?? '')
    })

    expect(inputs).toContain('\x1b[13;2u')
    // Regression guards: Shift+Enter must not collapse to plain CR or bare LF.
    expect(inputs).not.toContain('\r')
    expect(inputs).not.toContain('\n')
    expect(errors).toHaveLength(0)
  })
})

// ─── Section 4: Split view ───────────────────────────────────────────────────

test.describe('4. Split view', () => {
  test('4.1 closing the only pane returns to grid', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }
    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }
    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })
    await page.getByTestId('close-pane').first().click()
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 3000 })
    expect(errors).toHaveLength(0)
  })

  test('4.2 tile mode toggle buttons visible in pane view', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }
    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }
    await expect(page.getByTestId('tile-1')).toBeVisible({ timeout: 5000 })
    await expect(page.getByTestId('tile-2')).toBeVisible({ timeout: 5000 })
    expect(errors).toHaveLength(0)
  })

  test('4.3 open two panes side by side with tile-2 mode', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const wtCount = await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const state = (window as any).__MC_STORE__?.getState()
      return Object.keys(state?.worktrees ?? {}).length
    })
    if (wtCount < 2) { test.skip(); return }

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const store = (window as any).__MC_STORE__
      const state = store.getState()
      const ids = Object.keys(state.worktrees)
      state.setTileMode('2-up')
      state.openPane(ids[0])
      state.openPane(ids[1])
    })

    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })
    const paneCount = await page.getByTestId('pane').count()
    expect(paneCount).toBe(2)
    // Each pane has its own terminal
    expect(await page.locator('[data-testid^="terminal-pane-"]').count()).toBe(2)
    expect(errors).toHaveLength(0)
  })

  test('4.4 back to grid via top-bar button from split view', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }
    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }
    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })
    await page.getByTestId('grid-btn').click()
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 3000 })
    expect(errors).toHaveLength(0)
  })
})

// ─── Section 5: Project tree view ───────────────────────────────────────────

test.describe('5. Project tree view', () => {
  test('5.1 project tree opens via store action', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const hasProject = await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const state = (window as any).__MC_STORE__?.getState()
      return Object.keys(state?.projects ?? {}).length > 0
    })
    if (!hasProject) { test.skip(); return }

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const store = (window as any).__MC_STORE__
      const state = store.getState()
      const projectId = Object.keys(state.projects)[0]
      state.openProjectTree(projectId)
    })

    await expect(page.getByTestId('project-tree')).toBeVisible({ timeout: 3000 })
    await expect(page.getByTestId('tree-header')).toBeVisible()
    await expect(page.getByTestId('tree-panel')).toBeVisible()
    expect(errors).toHaveLength(0)
  })

  test('5.2 clicking project label button opens tree view', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const projId = await firstProjectId(page)
    if (!projId) { test.skip(); return }

    const labelBtn = page.locator(`[data-testid="project-label-${projId}"]`)
    if (await labelBtn.count() === 0) { test.skip(); return }
    await labelBtn.click({ force: true })
    await expect(page.getByTestId('project-tree')).toBeVisible({ timeout: 3000 })
    expect(errors).toHaveLength(0)
  })

  test('5.3 tree nodes show worktree info (branch, status)', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const store = (window as any).__MC_STORE__
      const state = store.getState()
      const wt = Object.values(state.worktrees as Record<string, { project_id: string }>)[0]
      if (wt) state.openProjectTree(wt.project_id)
    })

    await expect(page.getByTestId('project-tree')).toBeVisible({ timeout: 3000 })
    const nodes = page.locator('[data-testid^="tree-node-"]')
    await expect(nodes.first()).toBeVisible({ timeout: 3000 })
    const status = await nodes.first().getAttribute('data-status')
    expect(status).toBeTruthy()
    expect(errors).toHaveLength(0)
  })

  test('5.4 tree right panel mounts a TerminalPane for the selected worktree', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const store = (window as any).__MC_STORE__
      const state = store.getState()
      const wt = Object.values(state.worktrees as Record<string, { id: string; project_id: string }>)[0]
      if (wt) state.openProjectTree(wt.project_id)
    })

    await expect(page.getByTestId('project-tree')).toBeVisible({ timeout: 3000 })
    await expect(page.locator('[data-testid^="terminal-pane-"]').first()).toBeVisible({ timeout: 5000 })
    expect(errors).toHaveLength(0)
  })

  test('5.5 back to grid from tree view', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const hasProject = await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const state = (window as any).__MC_STORE__?.getState()
      return Object.keys(state?.projects ?? {}).length > 0
    })
    if (!hasProject) { test.skip(); return }

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const store = (window as any).__MC_STORE__
      const state = store.getState()
      const projectId = Object.keys(state.projects)[0]
      state.openProjectTree(projectId)
    })

    await expect(page.getByTestId('project-tree')).toBeVisible({ timeout: 3000 })
    await page.getByTestId('grid-btn-tree').click()
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 3000 })
    expect(errors).toHaveLength(0)
  })

  test('5.6 new worktree button is visible in tree view', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const hasProject = await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const state = (window as any).__MC_STORE__?.getState()
      return Object.keys(state?.projects ?? {}).length > 0
    })
    if (!hasProject) { test.skip(); return }

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const store = (window as any).__MC_STORE__
      const state = store.getState()
      const projectId = Object.keys(state.projects)[0]
      state.openProjectTree(projectId)
    })

    await expect(page.getByTestId('new-worktree-btn')).toBeVisible({ timeout: 3000 })
    await page.getByTestId('new-worktree-btn').click()
    await expect(page.getByTestId('new-worktree-branch-input')).toBeVisible({ timeout: 2000 })
    expect(errors).toHaveLength(0)
  })
})

// ─── Section 6: Card states ───────────────────────────────────────────────────

test.describe('6. Card states', () => {
  test('6.1 hovering a dot shows project card with status pill', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)

    const wtId = await firstWorktreeDotId(page)
    if (!wtId) { test.skip(); return }

    await page.locator(`[data-testid="worktree-dot-${wtId}"]`).hover({ force: true })
    await expect(page.getByTestId('detail-card')).toBeVisible({ timeout: 2000 })
    await expect(page.getByTestId('project-card').first()).toBeVisible()
    await expect(page.getByTestId('status-pill').first()).toBeVisible()
    expect(errors).toHaveLength(0)
  })
})

// ─── Section 9: Visual language enforcement ───────────────────────────────────

test.describe('9. Visual language', () => {
  test('9.1 no border-radius anywhere on grid screen', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    const violation = await page.evaluate(() => {
      for (const el of document.querySelectorAll('*')) {
        const br = window.getComputedStyle(el).borderRadius
        if (br && br !== '0px' && br !== '0%' && br !== '') {
          const values = br.split(' ').map(v => parseFloat(v))
          if (values.some(v => v > 0)) {
            return `${el.tagName}.${String(el.className).split(' ')[0]} → borderRadius: ${br}`
          }
        }
      }
      return null
    })
    expect(violation, `Border-radius violation: ${violation}`).toBeNull()
  })

  test('9.2 no font-weight above 400 on grid screen', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    const violation = await page.evaluate(() => {
      for (const el of document.querySelectorAll('*')) {
        const fw = parseInt(window.getComputedStyle(el).fontWeight, 10)
        if (fw > 400 && el.children.length === 0 && el.textContent?.trim()) {
          return `${el.tagName} fw=${fw} text="${el.textContent?.slice(0, 30)}"`
        }
      }
      return null
    })
    expect(violation, `Font-weight violation: ${violation}`).toBeNull()
  })

  test('9.3 no visible box-shadow on grid screen', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    const violation = await page.evaluate(() => {
      for (const el of document.querySelectorAll('*')) {
        const bs = window.getComputedStyle(el).boxShadow
        if (!bs || bs === 'none' || bs === '') continue
        const isAllTransparent = bs.split(/,(?![^(]*\))/).every(layer =>
          /rgba\(\s*[\d.]+\s*,\s*[\d.]+\s*,\s*[\d.]+\s*,\s*0\s*\)/.test(layer)
        )
        if (!isAllTransparent) {
          return `${el.tagName}.${String(el.className).split(' ')[0]} → boxShadow: ${bs}`
        }
      }
      return null
    })
    expect(violation, `Box-shadow violation: ${violation}`).toBeNull()
  })

  test('9.4 no border-radius in pane view', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('dot-grid')).toBeVisible({ timeout: 5000 })
    await page.waitForTimeout(800)
    if (!await isConnectedWithWorktrees(page)) { test.skip(); return }
    const opened = await openFirstPane(page)
    if (!opened) { test.skip(); return }
    await expect(page.getByTestId('pane').first()).toBeVisible({ timeout: 5000 })

    const violation = await page.evaluate(() => {
      for (const el of document.querySelectorAll('*')) {
        // Skip xterm internals — terminal renderer uses its own classnames
        // and may have non-zero radius on cursor/scrollbar that's part of
        // the embedded TUI, not Mission Control chrome.
        if (el.closest('.xterm')) continue
        const br = window.getComputedStyle(el).borderRadius
        if (br && br !== '0px' && br !== '0%' && br !== '') {
          const values = br.split(' ').map(v => parseFloat(v))
          if (values.some(v => v > 0)) {
            return `${el.tagName}.${String(el.className).split(' ')[0]} → borderRadius: ${br}`
          }
        }
      }
      return null
    })
    expect(violation, `Border-radius in pane view: ${violation}`).toBeNull()
  })

  test('9.5 no unhandled JS errors on load and interaction', async ({ page }) => {
    const errors = collectErrors(page)
    await page.goto('/')
    await page.waitForLoadState('networkidle')
    await page.waitForTimeout(2000)
    await page.getByTestId('add-project-btn').click()
    await page.waitForTimeout(300)
    await page.getByTestId('add-project-btn').click()
    await page.waitForTimeout(500)
    expect(errors).toHaveLength(0)
  })
})
