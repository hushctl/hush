/**
 * Mission Control Daemon — v1 Acceptance Tests
 *
 * Criteria tested:
 *  1. Daemon starts and listens on ws://localhost:9111
 *  2. Browser can connect via WebSocket
 *  3. Browser can send: register a project (name, path)
 *  4. Browser can send: create a worktree (project, branch name)
 *  5. Daemon spawns claude --continue in that worktree directory
 *  6. Browser can send a message to a specific worktree
 *  7. Daemon reads Claude Code's stdout and relays it back over WebSocket
 *  8. Daemon persists state to ~/.mission-control/state.json
 *  9. On restart, daemon reads state.json and knows about existing projects/worktrees
 */

import WebSocket from 'ws';
import { spawn, execSync } from 'child_process';
import { mkdirSync, writeFileSync, existsSync, readFileSync, rmSync, unlinkSync } from 'fs';
import path from 'path';
import os from 'os';

// ─── Config ──────────────────────────────────────────────────────────────────

const PORT = 9111;
const WS_URL = `ws://localhost:${PORT}/ws`;
const STATE_FILE = path.join(os.homedir(), '.mission-control', 'state.json');
const DAEMON_BIN = path.resolve('../daemon/target/debug/mcd');
const TURN_TIMEOUT_MS = 30_000; // max time to wait for a claude response

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

/** Open a WebSocket and wait for it to be ready. */
function connect() {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(WS_URL);
    ws.once('open', () => resolve(ws));
    ws.once('error', reject);
    setTimeout(() => reject(new Error('connect timeout')), 5000);
  });
}

/** Send a message and collect all server messages until `predicate` is satisfied or timeout. */
function waitFor(ws, predicate, timeoutMs = TURN_TIMEOUT_MS) {
  return new Promise((resolve, reject) => {
    const collected = [];
    const timer = setTimeout(() => {
      reject(new Error(`Timeout waiting for condition. Got: ${JSON.stringify(collected.slice(-3))}`));
    }, timeoutMs);

    function onMessage(raw) {
      const msg = JSON.parse(raw);
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

/** Send JSON to the WebSocket. */
function send(ws, obj) {
  ws.send(JSON.stringify(obj));
}

/** Start the daemon and wait until it is accepting connections. */
async function startDaemon() {
  const proc = spawn(DAEMON_BIN, [], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  // Surface daemon logs to stderr for debugging
  proc.stdout.on('data', d => process.stderr.write(`[daemon] ${d}`));
  proc.stderr.on('data', d => process.stderr.write(`[daemon] ${d}`));

  // Wait until it accepts a connection
  for (let i = 0; i < 30; i++) {
    await sleep(200);
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
    setTimeout(resolve, 2000); // fallback
  });
}

/** Set up a temporary git repo with one commit on `main`. */
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

console.log('Mission Control Daemon — v1 Acceptance Tests\n');

// Clean up any leftover state from previous runs
if (existsSync(STATE_FILE)) {
  console.log(`  (removing old ${STATE_FILE})\n`);
  unlinkSync(STATE_FILE);
}

const testRepo = makeTestRepo();
console.log(`  test repo: ${testRepo}\n`);

let daemon;
let ws;

try {
  // ── Test 1: Daemon starts and listens on ws://localhost:9111 ────────────────
  console.log('Test 1: Daemon starts and listens on ws://localhost:9111');
  try {
    daemon = await startDaemon();
    pass('daemon is listening on port 9111');
  } catch (e) {
    fail('daemon failed to start', e.message);
    process.exit(1); // no point continuing
  }

  // ── Test 2: Browser can connect via WebSocket ───────────────────────────────
  console.log('\nTest 2: Browser can connect via WebSocket');
  try {
    ws = await connect();
    pass('WebSocket connection established');
  } catch (e) {
    fail('WebSocket connection failed', e.message);
    process.exit(1);
  }

  // ── Test 3: Browser can register a project ──────────────────────────────────
  console.log('\nTest 3: Register a project');
  let projectId;
  try {
    const p = waitFor(ws, msg => msg.type === 'project_list', 5000);
    send(ws, { type: 'register_project', path: testRepo, name: 'TestProject' });
    const { msg } = await p;

    const project = msg.projects.find(p => p.path === testRepo);
    if (!project) throw new Error('project not in project_list response');
    projectId = project.id;
    pass(`project registered with id=${projectId}`);
  } catch (e) {
    fail('register_project failed', e.message);
    process.exit(1);
  }

  // ── Test 4: Browser can create a worktree ───────────────────────────────────
  console.log('\nTest 4: Create a worktree (project, branch name)');
  let worktreeId;
  try {
    const p = waitFor(ws, msg => msg.type === 'worktree_list' || msg.type === 'error', 10000);
    send(ws, {
      type: 'create_worktree',
      project_id: projectId,
      branch: 'main',
      permission_mode: 'plan',
    });
    const { msg } = await p;

    if (msg.type === 'error') throw new Error(`daemon error: ${msg.message}`);
    const wt = msg.worktrees.find(w => w.project_id === projectId);
    if (!wt) throw new Error('worktree not in worktree_list response');
    worktreeId = wt.id;
    pass(`worktree created with id=${worktreeId}, working_dir=${wt.working_dir}`);
  } catch (e) {
    fail('create_worktree failed', e.message);
    process.exit(1);
  }

  // ── Test 5 + 6 + 7: Send a message, daemon spawns claude, relays response ──
  console.log('\nTest 5+6+7: Send message → daemon spawns claude --continue → response relayed');
  let gotInit = false;
  let gotAssistant = false;
  let gotResult = false;
  let gotIdle = false;

  try {
    // Collect events until session_ended arrives
    const p = waitFor(ws, (msg, all) => {
      if (msg.type === 'claude_event') {
        const ev = msg.event;
        if (ev.type === 'system' && ev.subtype === 'init') gotInit = true;
        if (ev.type === 'assistant') gotAssistant = true;
        if (ev.type === 'result' && ev.subtype === 'success') gotResult = true;
      }
      if (msg.type === 'status_change' && msg.status === 'idle') gotIdle = true;
      return msg.type === 'session_ended';
    }, TURN_TIMEOUT_MS);

    send(ws, { type: 'send_message', worktree_id: worktreeId, content: 'Reply with exactly one word: hello' });
    const { msg } = await p;

    // Test 5: claude --continue was spawned (init event means a process started)
    if (gotInit) pass('claude --continue process spawned (received system/init event)');
    else fail('claude --continue spawn', 'no system/init event received');

    // Test 6: daemon wrote our message to claude's stdin (assistant event means claude processed it)
    if (gotAssistant) pass('message sent to claude stdin and processed (received assistant event)');
    else fail('message routing to stdin', 'no assistant event received');

    // Test 7: stdout relayed back over WebSocket
    if (gotResult) pass('claude stdout relayed back to browser (received result event)');
    else fail('stdout relay', 'no result event received');

    // Bonus: status returned to idle
    if (!gotIdle) console.log('    (note: idle status_change not received before session_ended)');
  } catch (e) {
    if (!gotInit) fail('claude --continue spawn', e.message);
    else if (!gotAssistant) fail('message routing to stdin', e.message);
    else fail('stdout relay', e.message);
  }

  // ── Test 8: State persisted to ~/.mission-control/state.json ───────────────
  console.log('\nTest 8: State persisted to ~/.mission-control/state.json');
  try {
    if (!existsSync(STATE_FILE)) throw new Error(`${STATE_FILE} does not exist`);
    const state = JSON.parse(readFileSync(STATE_FILE, 'utf8'));
    const proj = state.projects?.find(p => p.path === testRepo);
    if (!proj) throw new Error('test project not in state.json');
    const wt = proj.worktrees?.find(w => w.id === worktreeId);
    if (!wt) throw new Error('worktree not in state.json');
    pass(`state.json exists with project (${proj.id}) and worktree (${wt.id})`);
  } catch (e) {
    fail('state persistence', e.message);
  }

  // ── Test 9: On restart, daemon reads state.json ─────────────────────────────
  console.log('\nTest 9: Daemon restart — reads state.json, knows existing projects/worktrees');
  try {
    // Close connection and kill daemon
    ws.close();
    await killDaemon(daemon);
    daemon = null;
    await sleep(500);

    // Restart
    daemon = await startDaemon();
    ws = await connect();

    // Ask for project list — should already have our project
    const p1 = waitFor(ws, msg => msg.type === 'project_list', 5000);
    send(ws, { type: 'list_projects' });
    const { msg: pl } = await p1;
    const proj = pl.projects?.find(p => p.path === testRepo);
    if (!proj) throw new Error('project not restored after restart');

    // Ask for worktree list — should already have our worktree
    const p2 = waitFor(ws, msg => msg.type === 'worktree_list', 5000);
    send(ws, { type: 'list_worktrees' });
    const { msg: wl } = await p2;
    const wt = wl.worktrees?.find(w => w.id === worktreeId);
    if (!wt) throw new Error('worktree not restored after restart');

    pass(`project (${proj.id}) and worktree (${wt.id}) survived restart`);
  } catch (e) {
    fail('restart recovery', e.message);
  }

} finally {
  // Cleanup
  if (ws && ws.readyState === WebSocket.OPEN) ws.close();
  if (daemon) await killDaemon(daemon);
  try { rmSync(testRepo, { recursive: true, force: true }); } catch (_) {}

  // Summary
  console.log(`\n${'─'.repeat(50)}`);
  console.log(`Results: ${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}
