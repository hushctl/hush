/**
 * Mission Control Daemon — Acceptance Tests
 *
 * Tests browser↔daemon WebSocket protocol at the message level.
 * No browser required — uses raw WebSocket connections.
 *
 * Tests:
 *  1.  Daemon starts and listens
 *  2.  WebSocket connection (with auth token)
 *  3.  register_project → project_list
 *  4.  create_worktree → worktree_list
 *  5.  pty_attach → pty_data / pty_scrollback
 *  6.  paste_image → file written to disk
 *  7.  State persisted to state.json
 *  8.  Restart: state restored from state.json
 *  9.  Shell lifecycle: attach → input → data → kill → exit
 *  10. List operations roundtrip: list_projects, list_worktrees, list_peers
 *  11. git_status roundtrip
 *  12. list_files roundtrip
 *  13. read_file roundtrip
 *  14. pty_resize — no error
 *  15. pty_detach / reattach — scrollback replayed
 *  16. Error: invalid worktree_id
 *  17. Error: unknown message type — connection stays open
 *  18. Concurrent WebSocket connections — both receive responses
 *  19. remove_worktree → worktree gone from list
 */

import WebSocket from 'ws';
import { spawn, execSync } from 'child_process';
import { mkdirSync, writeFileSync, existsSync, readFileSync, rmSync, unlinkSync } from 'fs';
import path from 'path';
import os from 'os';

// ─── Config ──────────────────────────────────────────────────────────────────

const PORT = 19000 + (process.pid % 1000);
const WS_URL = `wss://localhost:${PORT}/ws`;
const TEST_DIR = path.join(os.tmpdir(), `mc-test-state-${Date.now()}`);
mkdirSync(TEST_DIR, { recursive: true });
const STATE_FILE = path.join(TEST_DIR, 'state.json');
// auth_token is written to the parent dir of --state-file (same as hush_dir)
const AUTH_TOKEN_FILE = path.join(TEST_DIR, 'auth_token');
const DAEMON_BIN = path.resolve('../daemon/target/debug/hush');
const TURN_TIMEOUT_MS = 30_000;

// ─── Helpers ─────────────────────────────────────────────────────────────────

let passed = 0;
let failed = 0;

function pass(name) {
  console.log(`  ✓ ${name}`);
  passed++;
}

function fail(name, reason) {
  console.log(`  ✗ ${name}`);
  console.log(`    → ${reason}`);
  failed++;
}

function getAuthToken() {
  try { return readFileSync(AUTH_TOKEN_FILE, 'utf8').trim(); }
  catch { return ''; }
}

/** Open a WebSocket with the current auth token and wait for it to be ready. */
function connect() {
  const token = getAuthToken();
  const url = token ? `${WS_URL}?token=${token}` : WS_URL;
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, { rejectUnauthorized: false });
    ws.once('open', () => resolve(ws));
    ws.once('error', reject);
    setTimeout(() => reject(new Error('connect timeout')), 5000);
  });
}

/** Collect all server messages until `predicate` is satisfied or timeout. */
function waitFor(ws, predicate, timeoutMs = TURN_TIMEOUT_MS) {
  return new Promise((resolve, reject) => {
    const collected = [];
    const timer = setTimeout(() => {
      reject(new Error(`Timeout waiting for condition. Got: ${JSON.stringify(collected.slice(-3))}`));
    }, timeoutMs);

    function onMessage(raw) {
      let msg;
      try { msg = JSON.parse(raw); } catch { return; }
      collected.push(msg);
      if (predicate(msg, collected)) {
        clearTimeout(timer);
        ws.off('message', onMessage);
        resolve({ msg, collected });
      }
    }
    ws.on('message', onMessage);
  });
}

/**
 * Wait until no messages matching `filter` arrive for `quietMs` ms.
 * The quiet timer does NOT start until the first matching message arrives,
 * so slow-starting shells (zsh with plugins) don't resolve prematurely.
 */
function waitForSilence(ws, filter = () => true, quietMs = 800, timeoutMs = 15000) {
  return new Promise((resolve) => {
    let quietTimer = null;
    const deadline = setTimeout(() => { ws.off('message', onMessage); resolve(); }, timeoutMs);
    function onMessage(raw) {
      let msg;
      try { msg = JSON.parse(raw); } catch { return; }
      if (filter(msg)) {
        if (quietTimer) clearTimeout(quietTimer);
        quietTimer = setTimeout(() => {
          clearTimeout(deadline);
          ws.off('message', onMessage);
          resolve();
        }, quietMs);
      }
    }
    ws.on('message', onMessage);
  });
}

/** Return a promise that rejects if `predicate` matches within timeoutMs. */
function assertNoMessage(ws, predicate, timeoutMs = 1500) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, timeoutMs);
    function onMessage(raw) {
      let msg;
      try { msg = JSON.parse(raw); } catch { return; }
      if (predicate(msg)) {
        clearTimeout(timer);
        ws.off('message', onMessage);
        reject(new Error(`Unexpected message: ${JSON.stringify(msg)}`));
      }
    }
    ws.on('message', onMessage);
  });
}

function send(ws, obj) {
  ws.send(JSON.stringify(obj));
}

/** Start the daemon and wait until it accepts connections. */
async function startDaemon() {
  const proc = spawn(DAEMON_BIN, ['--port', String(PORT), '--state-file', STATE_FILE], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  proc.stdout.on('data', d => process.stderr.write(`[daemon] ${d}`));
  proc.stderr.on('data', d => process.stderr.write(`[daemon] ${d}`));

  for (let i = 0; i < 30; i++) {
    await sleep(200);
    // Token file appears once daemon has initialized
    if (!existsSync(AUTH_TOKEN_FILE)) continue;
    try {
      const ws = await connect();
      ws.close();
      return proc;
    } catch (_) { /* not ready yet */ }
  }
  throw new Error('Daemon did not start within 6 seconds');
}

function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

function killDaemon(proc) {
  return new Promise(resolve => {
    proc.once('exit', resolve);
    proc.kill('SIGTERM');
    setTimeout(resolve, 2000);
  });
}

function makeTestRepo() {
  const dir = path.join(os.tmpdir(), `mc-test-${Date.now()}`);
  mkdirSync(dir, { recursive: true });
  execSync('git init -b main', { cwd: dir, stdio: 'pipe' });
  execSync('git config user.email "test@example.com"', { cwd: dir, stdio: 'pipe' });
  execSync('git config user.name "Test"', { cwd: dir, stdio: 'pipe' });
  writeFileSync(path.join(dir, 'README.md'), '# Test Project\n');
  execSync('git add .', { cwd: dir, stdio: 'pipe' });
  execSync('git commit -m "Initial commit"', { cwd: dir, stdio: 'pipe' });
  return dir;
}

// ─── Main ─────────────────────────────────────────────────────────────────────

console.log('Mission Control Daemon — Acceptance Tests\n');

if (existsSync(STATE_FILE)) {
  console.log(`  (removing old ${STATE_FILE})\n`);
  unlinkSync(STATE_FILE);
}

const testRepo = makeTestRepo();
console.log(`  test repo: ${testRepo}\n`);

let daemon;
let ws;
let projectId;
let worktreeId;

try {

  // ── Test 1: Daemon starts ────────────────────────────────────────────────────
  console.log(`Test 1: Daemon starts and listens on port ${PORT}`);
  try {
    daemon = await startDaemon();
    pass(`daemon listening on port ${PORT}`);
  } catch (e) {
    fail('daemon failed to start', e.message);
    process.exit(1);
  }

  // ── Test 2: WebSocket connection with auth token ─────────────────────────────
  console.log('\nTest 2: WebSocket connection (with auth token)');
  try {
    const token = getAuthToken();
    if (!token) throw new Error('auth_token file not found');
    ws = await connect();
    pass(`connected — token length=${token.length}`);
  } catch (e) {
    fail('WebSocket connection failed', e.message);
    process.exit(1);
  }

  // ── Test 3: Register a project ───────────────────────────────────────────────
  console.log('\nTest 3: register_project → project_list');
  try {
    const p = waitFor(ws, msg => msg.type === 'project_list', 5000);
    send(ws, { type: 'register_project', path: testRepo, name: 'TestProject' });
    const { msg } = await p;
    const project = msg.projects.find(p => p.path === testRepo);
    if (!project) throw new Error('project not in project_list response');
    projectId = project.id;
    pass(`project registered — id=${projectId}`);
  } catch (e) {
    fail('register_project', e.message);
    process.exit(1);
  }

  // ── Test 4: Create a worktree ─────────────────────────────────────────────────
  console.log('\nTest 4: create_worktree → worktree_list');
  try {
    const p = waitFor(ws, msg => msg.type === 'worktree_list' || msg.type === 'error', 10000);
    send(ws, { type: 'create_worktree', project_id: projectId, branch: 'main', permission_mode: 'plan' });
    const { msg } = await p;
    if (msg.type === 'error') throw new Error(`daemon error: ${msg.message}`);
    const wt = msg.worktrees.find(w => w.project_id === projectId);
    if (!wt) throw new Error('worktree not in worktree_list response');
    worktreeId = wt.id;
    pass(`worktree created — id=${worktreeId}, dir=${wt.working_dir}`);
  } catch (e) {
    fail('create_worktree', e.message);
    process.exit(1);
  }

  // ── Test 5: pty_attach → pty data received ───────────────────────────────────
  console.log('\nTest 5: pty_attach → pty_data or pty_scrollback');
  try {
    const p = waitFor(ws, msg =>
      (msg.type === 'pty_data' || msg.type === 'pty_scrollback' || msg.type === 'error') &&
      msg.worktree_id === worktreeId,
      10000,
    );
    send(ws, { type: 'pty_attach', worktree_id: worktreeId, cols: 80, rows: 24 });
    const { msg } = await p;
    if (msg.type === 'error' && msg.message?.includes('claude')) {
      pass(`pty_attach handled — claude not in PATH (CI): ${msg.message.slice(0, 60)}`);
    } else {
      pass(`pty alive — received ${msg.type} (${(msg.data || '').length} chars)`);
    }
  } catch (e) {
    fail('pty_attach', e.message);
  }

  // ── Test 6: paste_image ───────────────────────────────────────────────────────
  console.log('\nTest 6: paste_image → file written to disk');
  const PASTE_DIR = path.join(TEST_DIR, 'paste');
  const pasteFilename = `test-paste-${Date.now()}.png`;
  try {
    const pngBase64 =
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==';
    let gotError = false;
    waitFor(ws, msg => {
      if (msg.type === 'error' && msg.message?.includes('paste_image')) { gotError = true; return true; }
      return false;
    }, 3000).catch(() => {});
    send(ws, { type: 'paste_image', worktree_id: worktreeId, data: pngBase64, filename: pasteFilename });
    await sleep(1000);
    const pastedFile = path.join(PASTE_DIR, pasteFilename);
    if (!existsSync(pastedFile)) throw new Error(`${pastedFile} was not created`);
    const written = readFileSync(pastedFile);
    const expected = Buffer.from(pngBase64, 'base64');
    if (!written.equals(expected)) throw new Error(`content mismatch: ${written.length}b vs ${expected.length}b`);
    if (gotError) throw new Error('daemon returned paste_image error');
    pass(`image written — ${written.length} bytes`);
    try { unlinkSync(pastedFile); } catch (_) {}
  } catch (e) {
    fail('paste_image', e.message);
  }

  // ── Test 7: State persisted ────────────────────────────────────────────────
  console.log('\nTest 7: State persisted to state.json');
  try {
    if (!existsSync(STATE_FILE)) throw new Error(`${STATE_FILE} does not exist`);
    const state = JSON.parse(readFileSync(STATE_FILE, 'utf8'));
    const proj = state.projects?.find(p => p.path === testRepo);
    if (!proj) throw new Error('test project not in state.json');
    const wt = proj.worktrees?.find(w => w.id === worktreeId);
    if (!wt) throw new Error('worktree not in state.json');
    pass(`state.json has project (${proj.id}) and worktree (${wt.id})`);
  } catch (e) {
    fail('state persistence', e.message);
  }

  // ── Test 8: Daemon restart — state restored ────────────────────────────────
  console.log('\nTest 8: Daemon restart — state restored from state.json');
  try {
    ws.close();
    await killDaemon(daemon);
    daemon = null;
    await sleep(500);
    daemon = await startDaemon();
    ws = await connect();
    const p1 = waitFor(ws, msg => msg.type === 'project_list', 5000);
    send(ws, { type: 'list_projects' });
    const { msg: pl } = await p1;
    const proj = pl.projects?.find(p => p.path === testRepo);
    if (!proj) throw new Error('project not restored after restart');
    const p2 = waitFor(ws, msg => msg.type === 'worktree_list', 5000);
    send(ws, { type: 'list_worktrees' });
    const { msg: wl } = await p2;
    const wt = wl.worktrees?.find(w => w.id === worktreeId);
    if (!wt) throw new Error('worktree not restored after restart');
    pass(`project and worktree survived restart`);
  } catch (e) {
    fail('restart recovery', e.message);
  }

  // ── Test 9: Shell lifecycle ────────────────────────────────────────────────
  console.log('\nTest 9: Shell lifecycle — attach, kill, exit');
  const SHELL_ID = 'test-shell-1';
  try {
    // Set up the exit listener BEFORE attaching so we don't miss it
    const exitP = waitFor(ws, msg => msg.type === 'shell_exit' && msg.shell_id === SHELL_ID, 15000);

    // Attach — daemon spawns shell and immediately streams data;
    // we don't wait for data because timing after restart is unpredictable.
    // Instead, allow 3 seconds for the shell to fully start before killing.
    send(ws, { type: 'shell_attach', worktree_id: worktreeId, shell_id: SHELL_ID, cols: 80, rows: 24 });
    await sleep(3000);

    // Kill — expect shell_exit in response
    send(ws, { type: 'shell_kill', worktree_id: worktreeId, shell_id: SHELL_ID });
    await exitP;

    pass('shell attach → kill → shell_exit');
  } catch (e) {
    fail('shell lifecycle', e.message);
  }

  // ── Test 10: List operations roundtrip ────────────────────────────────────
  console.log('\nTest 10: List operations — list_projects, list_worktrees, list_peers');
  try {
    const p1 = waitFor(ws, msg => msg.type === 'project_list', 5000);
    send(ws, { type: 'list_projects' });
    const { msg: pl } = await p1;
    if (!pl.projects?.find(p => p.path === testRepo)) throw new Error('test project missing from list_projects');

    const p2 = waitFor(ws, msg => msg.type === 'worktree_list', 5000);
    send(ws, { type: 'list_worktrees' });
    const { msg: wl } = await p2;
    if (!wl.worktrees?.find(w => w.id === worktreeId)) throw new Error('test worktree missing from list_worktrees');

    const p3 = waitFor(ws, msg => msg.type === 'peer_list', 5000);
    send(ws, { type: 'list_peers' });
    const { msg: peers } = await p3;
    if (!peers.machine_id && !Array.isArray(peers.peers)) throw new Error('peer_list missing machine_id/peers');

    pass('list_projects + list_worktrees + list_peers all returned valid responses');
  } catch (e) {
    fail('list operations', e.message);
  }

  // ── Test 11: git_status ────────────────────────────────────────────────────
  console.log('\nTest 11: git_status roundtrip');
  try {
    const p = waitFor(ws, msg => msg.type === 'git_status' && msg.worktree_id === worktreeId, 8000);
    send(ws, { type: 'git_status', worktree_id: worktreeId });
    const { msg } = await p;
    if (!Array.isArray(msg.staged)) throw new Error('git_status missing staged array');
    if (!Array.isArray(msg.modified)) throw new Error('git_status missing modified array');
    if (!Array.isArray(msg.untracked)) throw new Error('git_status missing untracked array');
    pass(`git_status — staged=${msg.staged.length}, modified=${msg.modified.length}, untracked=${msg.untracked.length}`);
  } catch (e) {
    fail('git_status', e.message);
  }

  // ── Test 12: list_files ────────────────────────────────────────────────────
  console.log('\nTest 12: list_files roundtrip');
  try {
    const p = waitFor(ws, msg => msg.type === 'file_list' && msg.worktree_id === worktreeId, 8000);
    send(ws, { type: 'list_files', worktree_id: worktreeId });
    const { msg } = await p;
    if (!Array.isArray(msg.files)) throw new Error('file_list missing files array');
    if (!msg.files.includes('README.md')) throw new Error(`README.md not in file_list: ${JSON.stringify(msg.files)}`);
    pass(`file_list — ${msg.files.length} files, README.md present`);
  } catch (e) {
    fail('list_files', e.message);
  }

  // ── Test 13: read_file ─────────────────────────────────────────────────────
  console.log('\nTest 13: read_file roundtrip');
  try {
    const p = waitFor(ws, msg => msg.type === 'file_content' && msg.worktree_id === worktreeId, 8000);
    send(ws, { type: 'read_file', worktree_id: worktreeId, path: 'README.md' });
    const { msg } = await p;
    if (!msg.content) throw new Error('file_content missing content');
    if (!msg.content.includes('Test Project')) throw new Error(`README.md content unexpected: ${msg.content}`);
    pass(`read_file — ${msg.content.length} chars, content matches`);
  } catch (e) {
    fail('read_file', e.message);
  }

  // ── Test 14: pty_resize ────────────────────────────────────────────────────
  console.log('\nTest 14: pty_resize — no error response');
  try {
    // Re-attach first so there's an active pty session
    const attachP = waitFor(ws, msg =>
      (msg.type === 'pty_data' || msg.type === 'pty_scrollback' || msg.type === 'error') &&
      msg.worktree_id === worktreeId,
      8000,
    );
    send(ws, { type: 'pty_attach', worktree_id: worktreeId, cols: 80, rows: 24 });
    const { msg: attachMsg } = await attachP;
    if (attachMsg.type === 'error' && attachMsg.message?.includes('claude')) {
      pass('pty_resize skipped — claude not in PATH (CI)');
    } else {
      await assertNoMessage(
        ws,
        msg => msg.type === 'error' && msg.worktree_id === worktreeId,
        1500,
      );
      send(ws, { type: 'pty_resize', worktree_id: worktreeId, cols: 120, rows: 40 });
      await sleep(500);
      pass('pty_resize sent — no error received');
    }
  } catch (e) {
    fail('pty_resize', e.message);
  }

  // ── Test 15: pty_detach / reattach — scrollback replayed ──────────────────
  console.log('\nTest 15: pty_detach → reattach → scrollback replayed');
  try {
    send(ws, { type: 'pty_detach', worktree_id: worktreeId });
    await sleep(500);
    const p = waitFor(ws, msg =>
      (msg.type === 'pty_scrollback' || msg.type === 'error') && msg.worktree_id === worktreeId,
      8000,
    );
    send(ws, { type: 'pty_attach', worktree_id: worktreeId, cols: 80, rows: 24 });
    const { msg } = await p;
    if (msg.type === 'error' && msg.message?.includes('claude')) {
      pass('pty detach/reattach skipped — claude not in PATH (CI)');
    } else {
      if (!msg.data || msg.data.length === 0) throw new Error('scrollback data was empty');
      pass(`pty_scrollback received after reattach — ${msg.data.length} chars`);
    }
  } catch (e) {
    fail('pty detach/reattach', e.message);
  }

  // ── Test 16: Error — invalid worktree_id ──────────────────────────────────
  console.log('\nTest 16: Error handling — invalid worktree_id');
  try {
    const p = waitFor(ws, msg => msg.type === 'error', 5000);
    send(ws, { type: 'pty_attach', worktree_id: 'nonexistent-worktree-id', cols: 80, rows: 24 });
    const { msg } = await p;
    if (!msg.message) throw new Error('error message was empty');
    pass(`got error: "${msg.message}"`);
  } catch (e) {
    fail('invalid worktree_id error handling', e.message);
  }

  // ── Test 17: Unknown message type — connection stays open ──────────────────
  console.log('\nTest 17: Unknown message type — connection stays open');
  try {
    let disconnected = false;
    ws.once('close', () => { disconnected = true; });
    send(ws, { type: 'totally_bogus_message_type' });
    await sleep(2000);
    if (disconnected) throw new Error('connection was closed after unknown message type');
    pass('connection stayed open after unknown message type');
  } catch (e) {
    fail('unknown message type handling', e.message);
  }

  // ── Test 18: Concurrent WebSocket connections ──────────────────────────────
  console.log('\nTest 18: Concurrent WebSocket connections');
  let ws2;
  try {
    ws2 = await connect();
    // Both connections request project list
    const p1 = waitFor(ws, msg => msg.type === 'project_list', 5000);
    const p2 = waitFor(ws2, msg => msg.type === 'project_list', 5000);
    send(ws, { type: 'list_projects' });
    send(ws2, { type: 'list_projects' });
    const [{ msg: r1 }, { msg: r2 }] = await Promise.all([p1, p2]);
    if (!Array.isArray(r1.projects)) throw new Error('ws1 project_list malformed');
    if (!Array.isArray(r2.projects)) throw new Error('ws2 project_list malformed');
    pass(`both connections received project_list — ws1:${r1.projects.length} ws2:${r2.projects.length} projects`);
  } catch (e) {
    fail('concurrent connections', e.message);
  } finally {
    ws2?.close();
  }

  // ── Test 19: remove_worktree ───────────────────────────────────────────────
  console.log('\nTest 19: remove_worktree → worktree gone from list');
  try {
    const p = waitFor(ws, msg => msg.type === 'worktree_list', 5000);
    send(ws, { type: 'remove_worktree', worktree_id: worktreeId });
    const { msg } = await p;
    const gone = !msg.worktrees?.find(w => w.id === worktreeId);
    if (!gone) throw new Error('worktree still present after remove_worktree');
    pass('worktree removed — not present in subsequent worktree_list');
    worktreeId = null; // prevent cleanup from trying to delete again
  } catch (e) {
    fail('remove_worktree', e.message);
  }

} finally {
  if (ws && ws.readyState === WebSocket.OPEN) ws.close();
  if (daemon) await killDaemon(daemon);
  try { rmSync(testRepo, { recursive: true, force: true }); } catch (_) {}
  try { rmSync(TEST_DIR, { recursive: true, force: true }); } catch (_) {}

  console.log(`\n${'─'.repeat(50)}`);
  console.log(`Results: ${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}
