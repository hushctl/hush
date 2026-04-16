# Mission Control

Browser-based command center for Claude Code across all your machines. One spatial interface for every session on every machine, accessible from any device.

For product vision, research foundations, and UX roadmap, see [docs/VISION.md](docs/VISION.md).

---

## Setup

Run `make hooks` once after cloning to install the pre-commit build check.

Run `hush trust` once per machine to install the local CA into the OS trust store — this makes browsers automatically trust every daemon's TLS cert. To distribute the CA to a second machine, see `hush trust export`.

---

## Repo layout

```
daemon/   — Rust daemon (axum + tokio): pty manager, hook listener, state, TLS, gossip
ui/       — React + xterm.js browser app
scripts/  — build helpers
tests/    — integration tests
Makefile  — build, hooks, release targets
```

---

## Core invariants

Violating any of these requires explicit discussion first.

- **Terminal is the chat.** Never build a custom chat renderer. The embedded xterm.js terminal running a real `claude` pty IS the conversation surface — slash commands, tool approvals, diffs, all of it.
- **Status comes from hooks, never from parsing pty bytes.** The pty stream is opaque. All structured state flows from `mc-hook` events.
- **Command bar = workspace intent only.** It does not relay text to any worktree. Typing into the command bar sends nothing to Claude. To talk to Claude, focus the terminal pane and type there.
- **All Claude Code sessions are daemon-spawned.** No hybrid model of "some in iTerm, some in Mission Control."
- **Local-first, no external database.** `~/.hush/state.json` for daemon state; `~/.claude/` for conversation history. Nothing external in v1.
- **Visual language: flat, square corners, font-weight 400, no gradients, no shadows.** No border-radius anywhere.

---

## Architecture

```
┌───────────────────────────────────────────┐
│  Browser App (React + xterm.js)           │  ← Spatial canvas, project cards,
│  Accessible from anywhere                 │    embedded terminals, command bar
└──────────┬────────────────────────────────┘
           │ WebSocket (control JSON + binary pty frames)
┌──────────▼────────────────────────────────┐
│  Daemon (Rust, axum + tokio)              │  ← Runs on dev machine
│  Pty manager + hook listener + state      │
└──────────┬────────────────────────────────┘
           │ pty (stdin/stdout/stderr)     ▲
           │                                │ Unix socket (structured JSON)
┌──────────▼──────────┐    ┌────────────────┴──────┐
│  claude (CLI)       │───▶│  mc-hook (shim)       │
│  one per worktree   │    │  fires on lifecycle   │
└─────────────────────┘    │  events               │
                           └───────────────────────┘
```

**Daemon responsibilities:**
1. **Pty manager** — one long-lived `claude` process per worktree. Scrollback buffer fanned out to all attached browsers. Survives browser disconnects (tmux model).
2. **Hook listener** — Unix socket at `~/.hush/hooks.sock`. Hook events drive the status state machine and broadcast `status_change` to the browser.
3. **State persistence** — `~/.hush/state.json`: worktree registry, last known status, grid positions, layout prefs.
4. **Worktree lifecycle** — `git worktree add/remove` on create/delete. Writes `.claude/settings.json` to register `mc-hook` for each new worktree.

Daemon binds to `localhost:9111` in v1. No auth, no tunneling.

---

## Hook contract

`mc-hook` is a small binary co-located with the daemon. On spawn, the daemon injects:

```
MC_WORKTREE_ID=<uuid>
MC_HOOK_SOCKET=/Users/<user>/.mission-control/hooks.sock
```

Each worktree's `.claude/settings.json` registers the shim:

```json
{
  "hooks": {
    "SessionStart":     [{"hooks": [{"type": "command", "command": "/path/to/mc-hook session_start"}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "/path/to/mc-hook user_prompt"}]}],
    "PreToolUse":       [{"hooks": [{"type": "command", "command": "/path/to/mc-hook pre_tool_use"}]}],
    "Notification":     [{"hooks": [{"type": "command", "command": "/path/to/mc-hook notification"}]}],
    "Stop":             [{"hooks": [{"type": "command", "command": "/path/to/mc-hook stop"}]}],
    "SessionEnd":       [{"hooks": [{"type": "command", "command": "/path/to/mc-hook session_end"}]}]
  }
}
```

`mc-hook` reads hook JSON from stdin, stamps `MC_WORKTREE_ID` + event type, writes one JSON line to the socket, exits.

**Status state machine:**

| Hook event | Status transition |
|---|---|
| `SessionStart`, `UserPromptSubmit`, `PreToolUse` | → `running` |
| `Notification` | → `needs_you` |
| `Stop` | → `idle` |
| `SessionEnd` | → `idle`, clear session_id |
| pty exits nonzero | → `failed` |

`UserPromptSubmit` payload carries the prompt text → becomes the card's "current task" line.

---

## Persistence & restart recovery

**Two layers:**
- `~/.claude/` — Claude Code owns this. Full conversation history per session (jsonl). `claude --continue` resumes from here.
- `~/.hush/state.json` — Daemon owns this. Worktree registry, last status, grid positions, layout prefs. Ptys are runtime-only (die with daemon).

**On restart:**
1. Daemon reads `state.json`, sees known worktrees.
2. For each: open fresh pty, spawn `claude --continue` in the worktree directory.
3. `SessionStart` hook fires automatically → status machine wires back up.
4. Browser reconnects, re-attaches to pty, receives current scrollback.

What survives: conversation history, worktree registry, grid layout, on-disk file changes.
What doesn't: pre-restart scrollback buffer, in-flight execution.

---

## Project card states

Cards communicate status through three simultaneous signals: colored dot, status pill, card border.

| State | Dot | Border | Shows |
|---|---|---|---|
| **running** | green | default | current task, breadcrumb, progress bar |
| **needs_you** | amber | amber | what it's waiting for, Approve/View diff/Discuss buttons |
| **idle** | gray | default | last session summary, queued tasks |
| **failed** | red | red | plain-English error, Resume/View logs/Retry buttons |

**Responsive breakpoints:**
- Full/half width — full card
- Quarter width — name + dot + pill + one-line breadcrumb + badge count only
- Minimal (sidebar) — name + dot only

---

## Command bar verbs (v1)

The command bar expresses intent about the workspace layout, not messages to Claude.

| Verb | Effect |
|---|---|
| `pull up <project> [and <project>...]` | Open one terminal pane per named worktree, switch to pane view |
| `open <project>/<branch>` | Open a specific worktree pane |
| `close <project>` | Close that pane (pty keeps running). Last pane closed → grid |
| `back to grid` | Close all panes, return to dot grid |
| `show me what needs me` | Open panes for every `needs_you` worktree |
| `tree <project>` | Open project tree view |
| `new worktree <branch> [in <project>]` | `git worktree add`, spawn pty, open pane |

---

## Tech stack

| Component | Tech |
|---|---|
| Browser app | React + Vite + TypeScript + Tailwind + shadcn/ui |
| Terminal renderer | `xterm.js` + `xterm-addon-fit` |
| State management | Zustand (persist middleware for layout prefs) |
| Daemon | Rust — `axum` + `tokio` |
| Pty management | `portable-pty` (same crate as Wezterm) |
| Hook transport | Unix domain socket + `mc-hook` shim |
| Daemon state | `~/.hush/state.json` (migrated from `~/.mission-control/`) |
| Conversation history | Claude Code's `~/.claude/` (via `--continue`) |
| Voice (v2) | Web Speech API → Claude intent parsing |
| Cross-project AI (v2) | Claude API |
