# Claude Code Mission Control

## Setup

Run `make hooks` once after cloning to install the pre-commit build check.

Run `hush trust` once per machine to install the local CA into the OS trust store — this makes browsers automatically trust every daemon's TLS cert, with no per-connection manual exceptions. To distribute the CA to a second machine, see `hush trust export`.

## What this is

A browser-based multi-project command center for Claude Code. Instead of switching between iTerm tabs, you manage all your active Claude Code sessions from a single spatial interface — accessible from any device.

**Inspired by**: [Nodepad](https://github.com/mskayyali/nodepad) — a spatial research tool where AI works in the background while you stay in control of the space. We apply the same philosophy to engineering workflow: spatial awareness across projects, AI-augmented but human-directed.

---

## The problem

When you're running 3-4 projects simultaneously (e.g. Kinobi, Rangoli Merge, OpenClaw, FinBox infra), the workflow today is:

- Multiple iTerm tabs, each with a Claude Code session
- Tab-switching to check on progress, losing spatial context
- No cross-project visibility — you can't see all projects at a glance
- Locked to the machine running the terminals
- No way to spot cross-project patterns or reusable insights

---

## Design philosophy

### The canvas is infinite but attention is finite

The primary design tension: you have N parallel projects, one human brain. The interface must let you see everything at a glance while diving deep into 1, 2, or N projects simultaneously without losing context on the rest.

### The interface is calm, not absent

The long-term vision is intent-driven and ambient — you speak, the UI responds, surfaces recede when not needed. But we build toward that gradually. v1 embeds the real Claude Code terminal in the browser for the actual conversation with the agent, and layers a workspace-intent command bar on top for navigation and voice. The futuristic stuff earns its way in as the product proves itself.

**The progression is: type → talk → glance.**

Day one, most people type. Day thirty, they're talking. Day ninety, they forget the keyboard is there. Each version is fully usable on its own — nobody waits for v3 to get value.

### Workspace grammar, not prescribed layouts

We design the **tiling system** — the mechanics of how project panes split, resize, and snap — not the arrangements themselves. Users write their own layouts; we provide the grammar.

Key principles for the tiling engine:
- The user expresses intent ("add Rangoli to my workspace"), the system handles geometry
- Panes have responsive breakpoints: full-width shows the live terminal + status chrome; quarter-width collapses to status + last activity + notification badge
- The system learns common pairings and offers them as one-click arrangements
- Arrangements are temporary and fast to create/destroy (Arc browser's split-view model)
- Spatial stability: projects stay where you put them across sessions

Study: i3wm/Hyprland (tiling WMs), Arc browser split view, tmux, Amethyst/Yabai.

---

## Research foundations

### Intent-based outcome specification (Jakob Nielsen)

Nielsen identifies AI as the third UI paradigm in computing history — after batch processing and command-based interaction. The user states the desired result; the system determines the procedure.

**Key concepts directly applicable to Mission Control:**

- **Run contracts** — Before the AI executes, show: estimated time, cost/scope, definition of "done", hard boundaries (what it won't touch). Maps to: when you say "start task X on Kinobi", the system shows what it'll do before doing it.
- **Conceptual breadcrumbs** — For long-running tasks, synthesized summaries of intermediate conclusions, not raw logs. Maps to: project cards showing "Kinobi: extraction pipeline 60% done, found 3 edge cases in CDX retry" not a wall of terminal output.
- **Context reboarding** — When a user returns after switching away, a resumption summary reminds them of original intent, key decisions, and current status. Maps to: "Welcome back. Kinobi finished the benchmark — results in /eval. Rangoli is waiting for your approval on the energy curve change. OpenClaw is idle."
- **The articulation barrier** — Half of users struggle to express intent in prose. Voice lowers this barrier. GUI affordances (buttons, status indicators) remain essential as fallback and confirmation.

**Reading list:**
- "AI Is First New UI Paradigm in 60 Years" (Nielsen, 2023) — jakobnielsenphd.substack.com
- "Intent by Discovery: Designing the AI User Experience" (Nielsen, 2025) — jakobnielsenphd.substack.com
- "The Articulation Barrier" (Nielsen, 2023) — linkedin.com

### Calm technology (Mark Weiser & John Seely Brown, Xerox PARC)

Weiser's thesis: the most profound technologies are those that disappear, weaving into everyday life until indistinguishable from it. Calm technology informs but doesn't demand focus or attention.

**Key principles for Mission Control:**

- **Center and periphery** — Design for both. Active project is center; other projects communicate state through periphery (color, subtle indicators, badge counts). A glance tells you "Kinobi is running, Rangoli needs me, OpenClaw is idle" in under a second.
- **Calmness as design goal** — If computers are everywhere they better stay out of the way. The people being shared by the computers must remain serene and in control.
- **The "Dangling String" model** — Weiser's example of an 8-foot string connected to network traffic. The more traffic, the more it spins. No screen, no numbers — just ambient awareness. Our project cards should have this quality at rest.

**Reading list:**
- "The Computer for the 21st Century" (Weiser, 1991) — Scientific American / calmtech.com
- "The Coming Age of Calm Technology" (Weiser & Brown, 1996) — calmtech.com
- "Calm Technology: Principles and Patterns for Non-Intrusive Design" (Amber Case, 2015) — O'Reilly

### Zero UI / ambient interfaces

The emerging field of interfaces that fade into the background, replacing screen-based navigation with voice, gesture, and contextual awareness.

**Key caution:** Invisible doesn't mean infallible. Context confusion erodes confidence fast — the more seamless the system seems, the more jarring it feels when it fails. Mission Control must have graceful fallback from voice to visual to manual.

**Reading list:**
- "Ambient AI in UX: Interfaces That Work Without Buttons" (2026) — medium.com
- "Zero UI: Beyond the Screen" — medium.com/design-bootcamp
- "Shaping Interfaces With Intent" (Buildo, 2025) — buildo.com
- "Generative UI: Smart, Intent-Based, and AI-Driven" (2025) — medium.com/design-bootcamp
- IxDF's "What is No-UI Design?" resource page — interaction-design.org

### Hybrid interface model (Nielsen's recommendation)

Nielsen advocates for hybrid UIs combining intent-based natural language with GUI controls. This is our v1 strategy — but with a critical split:

- **Conversation with the agent** happens in the real Claude Code terminal, embedded in the browser. Zero mimicry, full fidelity, slash commands and tool approvals just work.
- **Intent about the workspace** happens in the command bar. "Pull up kinobi and rangoli", "back to grid", "close finbox" — outcome statements the system translates into layout changes.
- **Visual state** comes from project cards and the dot grid, driven by structured status events the daemon receives from Claude Code hooks (not parsed from terminal output).

Two surfaces, two jobs. The terminal is where work happens; the command bar is where workspace intent is expressed; the visual layer is ambient awareness. Neither alone is sufficient, and critically, they don't overlap — typing into the command bar does not route messages to the agent (that's what the terminal is for).

---

## Core design decisions

### Both planning and execution happen through Claude Code

Planning conversations must have full codebase context — files, patterns, project memory, CLAUDE.md. A "planning chat" without that context is just vibes. Therefore:

- **There is no separate planning mode.** Both thinking and building happen inside Claude Code sessions.
- The difference is **autonomy level**, not a different system:
  - **Conversation mode** — Claude Code has full codebase context, reads files, analyzes, greps — but doesn't modify anything. You're discussing the approach.
  - **Execution mode** — Same session, same context. Now it writes code, runs tests, commits. The plan was approved, let it run.
- This is one continuous Claude Code session per project, mediated through the browser.

### The browser is a window, not a replacement

The browser app does NOT replace Claude Code's intelligence. It provides:

- **Multi-project awareness** — see all projects at once
- **Remote access** — check on builds from your phone, dispatch commands from an iPad
- **Cross-project insight** — spot when work in one project is relevant to another
- **Session persistence** — conversation history and state survive across devices

### The terminal is the chat, not a reconstruction of it

We do not build a custom chat UI that renders Claude Code's tool-use blocks, code diffs, and streaming text. That path leads to a permanent lag behind the CLI: every time Claude Code changes its output format, adds a slash command, or introduces a new TUI affordance, a custom renderer has to chase it.

Instead, the per-worktree chat surface is a **real Claude Code process** running in a pty, streamed to the browser via `xterm.js` over WebSocket. The daemon owns the pty and keeps it alive across browser disconnects (tmux model). The browser attaches, renders live output, forwards keystrokes. Slash commands work. `/compact` and `/clear` work. Tool approvals work. Theme, colors, cursor — all exact.

This means the "chat thread per project" in the UX evolution below is literally an embedded terminal, not a rendered conversation view.

### Status comes from Claude Code hooks, not TUI parsing

The daemon doesn't learn a worktree's state by parsing terminal bytes — that would couple us to output format and negate the terminal-embed decision. Instead, each spawned Claude Code session is configured to invoke a small hook binary (`mc-hook`) on key lifecycle events, which forwards structured JSON to the daemon over a Unix socket:

| Hook | Status transition |
|------|------------------|
| `SessionStart`, `UserPromptSubmit`, `PreToolUse` | → `running` |
| `Notification` (Claude wants permission or input) | → `needs_you` |
| `Stop` (turn complete) | → `idle` |
| `SessionEnd` | → `idle`, clear session_id |
| pty exits nonzero | → `failed` |

`UserPromptSubmit` also carries the prompt text, which becomes the card's "current task" line. Hooks are the single source of truth for structured status — no heuristics, no regex on stdout, no guessing from tool_use blocks.

### Multi-focus is the default, not single-project dive

Users frequently work on 2+ projects in parallel (e.g. when a shared utility spans Kinobi and OpenClaw). The tiling system must support fluid multi-project views as a first-class interaction, not a special mode.

---

## UX evolution — from chat to calm

### v1: Terminal + command bar, familiar surface

The foundation. People already know how to use a terminal, and the command bar is a single new thing to learn.

- **Embedded Claude Code terminal per worktree** — the primary conversation surface. A real `claude` process in a pty, rendered with xterm.js. You type, approve, use slash commands exactly as you would in iTerm.
- **Project cards with status** — structured grid or tiling layout. Each card shows: project name, branch, status (running/needs_you/idle/failed), one-line current task. Driven by hook events, not terminal parsing.
- **Manual tiling** — click a dot to open a terminal pane, split-view two worktrees, simple preset layouts (1-up, 2-up side-by-side).
- **Workspace-intent command bar** — bottom of the screen. Accepts outcome statements like "pull up kinobi and rangoli", "back to grid", "close finbox". Does *not* forward typed text to any worktree — the terminal is where you talk to Claude.
- **Run contracts** — visible inside the embedded terminal, because Claude Code already shows them there. The daemon observes `Notification` hooks to flip the worktree status to `needs_you` and amber-border the card.
- **Mic button** on the command bar, not on the terminal. Speech-to-text becomes a command-bar utterance. The terminal stays keyboard-driven in v1 because that's how Claude Code works.

### v2: Voice becomes natural

Voice starts doing more than transcription.

- **Intent parsing** — "show me what needs my attention" filters the UI to projects with pending approvals or completed tasks. "Pull up Kinobi and Rangoli side by side" rearranges the tiling.
- **Smart suggestions** — system proactively surfaces: "Kinobi finished its benchmark. Rangoli is waiting for approval. OpenClaw has been idle for 3 hours."
- **Context reboarding** — when you open the app after being away, a summary greets you: what happened, what changed, what needs you.
- **Learned tiling** — system remembers your common project pairings and offers them.
- **Conceptual breadcrumbs** — project cards show synthesized progress, not raw logs.

### v3: The interface is calm

Voice is primary. The visual layer is ambient.

- **The command bar recedes** — it's still there, but voice is the default way to express workspace intent. The dot grid is the calm canvas; terminals open only when you're actually working in one.
- **Proactive surfacing** — the system doesn't wait for you to ask. It tells you what matters when you arrive.
- **Cross-project synthesis** — AI notices patterns across codebases and surfaces them without being asked.
- **The terminal stays a terminal.** Always. Conversation with Claude Code never gets mimicked or voice-translated — you either type into the embedded terminal, or you speak intent to the command bar and let it open the right terminals for you. The keyboard is always one click away.

---

## Project card — the core UI component

The project card is the most important element in v1. It's the thing you look at before typing or speaking anything. It must answer "what's happening here and what do I need to do" without clicking into the project.

### Design principle: triple-redundant status

Every card communicates state through three simultaneous signals — so it's legible at any zoom level or glance duration:
1. **Colored dot** — inline with the project name. Immediate peripheral signal.
2. **Status pill** — text label (running, needs you, idle, failed). Readable at medium distance.
3. **Card border** — shifts to amber (needs you) or red (failed) when action is required. Visible even in peripheral vision.

### Four card states

**Running** — project is actively executing
- Status: green dot, "running" pill, default border
- Shows: current task name, conceptual breadcrumb (what it just did / is doing now in plain English, not logs), progress bar, metadata (time started, files read, files modified)
- No action required — pure awareness
- Breadcrumb example: "Added exponential delay with jitter. Now writing tests for timeout edge cases."

**Needs you** — Claude Code is blocked, waiting for human input
- Status: amber dot, "needs you" pill, amber border
- Shows: what it's waiting for, why (explained clearly enough to decide), the specific change proposed
- Action buttons inline: Approve, View diff, Discuss
- Metadata: how long it's been waiting, relevant prior context
- This is Nielsen's "run contract" surfaced as a decision point
- Example: "Wants to modify CasualMeera archetype: reduce energy drain from 2.1 to 1.4 per action. Affects 3 sim configs."

**Idle** — session ended or paused, no active work
- Status: gray dot, "idle" pill, default border
- Shows: last session summary (what was completed), what's queued next (if anything)
- Metadata: time since last active, number of queued tasks
- Purpose: you glance and know "OpenClaw is done for now, here's what it would do next if I resumed"

**Failed** — execution hit an error
- Status: red dot, "failed" pill, red border
- Shows: error explained in plain English (not a stack trace), rollback status
- Action buttons inline: Resume with fix, View logs, Retry
- Metadata: time since failure, rollback state (clean/dirty)
- Example: "Migration script failed: column type mismatch on events.payload (expected JSON, got String). Rolled back automatically."

### Card anatomy (top to bottom)

1. **Header row** — project name (with dot), branch name (monospace, muted), status pill (right-aligned)
2. **Section label** — "Current task" / "Waiting for approval" / "Last session" / "Error" (tiny, uppercase, muted)
3. **Task name** — one line, the human-readable task description
4. **Breadcrumb** — 1-2 lines of synthesized progress or context. Plain English, not terminal output.
5. **Progress bar** (if running or partially complete) — thin, minimal
6. **Divider** (if actions needed)
7. **Action buttons** (if needs you or failed) — inline, primary action highlighted
8. **Metadata row** — time, file counts, prior context. Smallest text, least emphasis.

### Responsive breakpoints

- **Full width (expanded/focused)** — full card header + embedded terminal pane below
- **Half width (tiled 2-up)** — full card as described above
- **Quarter width (tiled 3-4 up)** — collapses to: name + dot + pill + one-line breadcrumb + action badge count. No buttons, no progress bar. Click to expand.
- **Minimal (sidebar/list)** — name + dot only. Purely peripheral.

---

## Visual language

Flat design throughout. No border-radius anywhere — square corners on everything: input boxes, cards, buttons, dots on the grid, hover panels, mic button. No bold text in the UI — all font-weight 400. No gradients, no shadows.

Inspired by Parallel.ai's dot-grid scatter chart aesthetic: data points on a quiet field, labels that inform without shouting.

---

## The dot grid — home screen

The home screen is a dot grid. A uniform field of small square dots fills the background. Each project (and each worktree within a project) is a gravity well — nearby grid dots subtly pick up the project's status color and grow slightly larger, creating a visible warp in the field without any heavy animation.

### Grid axes

Top-left = highest urgency + most recent activity. Bottom-right = lowest urgency + least recent. Projects drift on these axes as their state changes. When Rangoli gets approved, it drifts away from the urgent corner. When FinBox fails, it pulls toward it.

### Project dots

Each dot on the grid represents a chat worktree (not a project). Dot size encodes urgency: "needs you" is largest, "failed" next, "running" medium, "idle" smallest. Dot color encodes status: amber = needs you, red = failed, green = running, gray = idle.

Labels sit right-aligned against each dot: project name (uppercase, letter-spaced), status and time below. All font-weight 400, no bold anywhere.

Hover on a dot expands a flat detail card (no border-radius) with breadcrumb, action buttons, and metadata — the same card spec defined earlier.

### Chat worktrees

A project isn't a single dot. It's a cluster of dots — one per active worktree. Like git worktrees, each is a separate Claude Code session on a different branch, sharing the same codebase.

Example: Kinobi might have three worktrees:
- main — CDX retry backoff (running)
- feat/qwen3-eval — extraction benchmark (idle)
- fix/pathfinder-t3 — tier-3 escalation hotfix (running)

The cluster is labeled with the project name, and each worktree dot sits below with its branch name. The gravity well on the background grid comes from each individual worktree, so a project with two active worktrees creates a denser warp than one with a single idle worktree.

### Project tree view

Clicking a project label on the grid opens the project tree view — a vertical tree (like git log) of all worktrees in that project. Left side shows the tree: a vertical connecting line with branching nodes, each showing branch name, status pill, current task, and time since active. Right side shows the embedded terminal pane for the selected worktree, streaming live from its pty. You can type directly into it, use slash commands, everything.

"+ new worktree" at the bottom of the tree spins up a fresh Claude Code session on a new branch — the daemon runs `git worktree add`, spawns a new pty, and the new terminal appears in the right panel.

### Context reboarding

A flat bar at the top of the grid summarizes what happened since you were last here. One line, no chrome: "rangoli / level-gen needs your approval. finbox migration failed. 6 worktrees active across 4 projects."

---

## The command bar — workspace intent layer

The command bar sits at the bottom of the screen, always present across grid/pane/tree views. It is **not** a message relay to any worktree — conversation with Claude Code happens in the embedded terminals. The command bar's job is expressing intent about *the workspace*: which terminals should be open, where they should sit, how the view should be arranged.

Its placeholder describes what it does in the current context:
- On the grid: `express workspace intent — "pull up kinobi", "show what needs me"...`
- In pane view: `"split left", "close rangoli", "back to grid"...`
- In tree view: `"new worktree feat/x", "back to grid"...`

### Intent verbs (v1)

All verbs are outcome statements — the user says the *result*, the system figures out the procedure.

| Verb | Effect |
|---|---|
| `pull up <project>[ and <project>...]` | Switch to pane view, open one terminal per named worktree. 2 projects → 2-up tiling. |
| `open <project>/<branch>` | Open a specific worktree's terminal pane. |
| `close <project>` | Close that worktree's pane. Last pane closed returns to grid. |
| `back to grid` | Close all panes, return to dot grid. |
| `show me what needs me` | Switch to pane view, open terminals for every worktree in `needs_you`. |
| `tree <project>` | Open the project tree view. |
| `new worktree <branch>[ in <project>]` | Run `git worktree add`, spawn a new pty, open its pane. |

### What the command bar is NOT

- It does not forward text to any worktree. If you type `yes` into the command bar, nothing gets sent to any Claude Code session. To say `yes` to Claude, click into the terminal and type there.
- There is no "routing target". There are no tabs for selecting which worktree to message. A worktree is talked to by focusing its terminal pane.
- There is no `@kinobi main` message routing. `@` is reserved for the v2 version of workspace verbs if we need disambiguation.

### Why this split

Two surfaces, two jobs — the Nielsen intent-paradigm argument. The terminal is direct manipulation of the agent (keystrokes → pty → Claude Code). The command bar is intent specification for the workspace (natural language → layout actions). Keeping them separate means neither has to do the other's job poorly, and it gives voice (v2) a clean home: voice goes to the command bar, never to the terminal.

### Voice (v2 preview)

In v2 the command bar accepts voice. The verbs are the same; only the input modality changes. "Pull up kinobi and rangoli" works whether typed or spoken. The terminal stays keyboard-driven because that's how conversation with Claude Code works today — voice for workspace intent, keys for agent conversation.

---

## Split view — parallel work

When you open one or more worktrees for active work, the grid recedes and terminal panes take over. The grid is always one "← grid" click or "back to grid" command-bar utterance away.

### Layout

The top bar shows all projects as status dots with names. Active (open in a pane) projects are highlighted. Inactive projects stay visible as peripheral indicators — you never lose awareness.

Each pane contains: a header (project name, branch, status pill) and the embedded Claude Code terminal streaming from its pty. The terminal is the input — no separate input box. Panes are flat, separated by a 0.5px border. Keyboard focus routes naturally: clicking into a pane's terminal makes that pane the active one.

### Cross-project insight banner

When the system detects a connection between projects, a flat banner appears below the top bar: "kinobi's Qwen3 extraction benchmark results may be relevant to openclaw's agent pipeline — link projects?" Dismissible, actionable, never intrusive.

---

## Interaction workflows

### Flow 1 — voice/command-bar driven split from grid

1. Arrive at grid. Context reboarding bar shows what happened.
2. Type or say "pull up rangoli and kinobi" into the command bar — grid recedes, two terminal panes slide in side by side.
3. Click into the left pane (rangoli's terminal), type `y` to approve the diff. Claude Code continues; `Stop` hook fires; status pill changes back to "running".
4. Click into the right pane (kinobi's terminal), type "yes, add the circuit breaker". Enter.
5. Cross-project insight banner appears.
6. Type or say "back to grid" into the command bar — panes recede, grid returns with updated dot positions. Terminals keep running in the background (pty stays alive).

### Flow 2 — click-driven single then split

1. Click the finbox dot on the grid. Pane view opens with the finbox terminal full-width.
2. In the terminal, type "show me the migration error". Claude Code responds in the same terminal.
3. Command bar: "also pull up kinobi" — single pane splits into two. Kinobi's terminal appears on the right.
4. Cross-reference finbox's error with kinobi's types across both panes.
5. Fix finbox in the left terminal, continue kinobi in the right.
6. Command bar: "close kinobi" — right pane closes, finbox expands to full width. Kinobi's pty keeps running; the pane just detached.

### Flow 3 — notification interrupt during split

1. Working in split view: kinobi + finbox, both running.
2. OpenClaw finishes a background task — its dot in the top bar shifts gray to amber.
3. Command bar: "swap finbox for openclaw" — right pane's terminal detaches, openclaw's terminal attaches in its place. Finbox's pty keeps running in the background. Zero context lost on kinobi.
4. Or ignore entirely — amber dot persists, system never forces a context switch.

---

## Architecture

### Three layers

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
│  full codebase      │    │  events via Claude    │
│                     │    │  Code hooks system    │
└─────────────────────┘    └───────────────────────┘
```

### Daemon — the critical bridge

The daemon is a lightweight Rust server running on the dev machine. It has four jobs:

1. **Pty manager** — one long-lived `claude` process per worktree, running in a pty owned by the daemon. The daemon reads stdout bytes into a circular scrollback buffer and fans them out to any attached browser over WebSocket as binary frames. Writes go stdin-ward. Ptys survive browser disconnects (tmux model) — close the laptop lid, come back, the session is still there.
2. **Hook listener** — a Unix socket at `~/.mission-control/hooks.sock` that the `mc-hook` shim writes to. Incoming hook events drive the worktree status state machine (`running` / `needs_you` / `idle` / `failed`) and broadcast `status_change` messages to the browser.
3. **State persistence** — JSON file at `~/.mission-control/state.json` with the project/worktree registry, last known status, grid positions, and layout prefs. Ptys themselves are runtime-only and not persisted (they die with the daemon).
4. **Worktree lifecycle** — `git worktree add` / `git worktree remove` on create/delete. On create it also writes or merges the worktree's `.claude/settings.json` to register `mc-hook` as the handler for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `Notification`, `Stop`, `SessionEnd`.

**Crucial property**: the daemon never parses terminal bytes to learn status. The pty byte stream is opaque data flowing to the browser. Structured state comes only from hook events. This decouples status tracking from Claude Code's output format — if the TUI changes tomorrow, the daemon doesn't care.

**All sessions are daemon-spawned.** The daemon starts every Claude Code session as a child process in a pty. No hybrid model of "some sessions in iTerm, some in Mission Control" — you stop using iTerm for Claude Code entirely. One place for everything.

**Localhost only for v1.** The daemon binds to `localhost:9111`. The browser connects to `http://localhost:9111`. No tunneling, no auth. Remote access is v2 (Cloudflare Tunnel / ngrok + token auth).

### The mc-hook shim

`mc-hook` is a small binary (ships with the daemon, lives in the same bin directory). When daemon spawns a worktree's `claude` process it injects two env vars:

```
MC_WORKTREE_ID=<uuid>
MC_HOOK_SOCKET=/Users/<user>/.mission-control/hooks.sock
```

Claude Code's `.claude/settings.json` in that worktree registers the shim for each hook:

```json
{
  "hooks": {
    "SessionStart":     [{"hooks": [{"type": "command", "command": "/path/to/mc-hook session_start"}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "/path/to/mc-hook user_prompt"}]}],
    "Notification":     [{"hooks": [{"type": "command", "command": "/path/to/mc-hook notification"}]}],
    "Stop":             [{"hooks": [{"type": "command", "command": "/path/to/mc-hook stop"}]}],
    "SessionEnd":       [{"hooks": [{"type": "command", "command": "/path/to/mc-hook session_end"}]}]
  }
}
```

`mc-hook` reads the hook's JSON stdin, stamps it with `MC_WORKTREE_ID` and event type, writes one line of JSON to the Unix socket, exits. The daemon's hook listener task parses that line and dispatches a status transition.

This means all heuristic-based "classify this terminal output" code goes away. Hook events are the ground truth.

### Autonomous work

Long-running autonomous work (e.g. a `/loop` that checks crawl status and rebalances infrastructure) runs inside the same pty as everything else. The user starts the loop by typing into the embedded terminal. Claude Code executes; each iteration fires `PreToolUse`/`Stop` hooks, which keep the card's "current task" fresh without the daemon ever parsing a byte of output. If the loop hits a `Notification` (needs permission), the dot flips to amber — same state machine, no special handling for autonomous vs. interactive.

### Persistence — local-first, no external database

No Supabase, no external database for v1. Two persistence layers already exist:

**Claude Code's own storage** (`~/.claude/`):
- Full conversation history per session
- Tool use history
- Project context (CLAUDE.md, etc.)
- Claude Code handles resume natively — start a new session in the same directory and it can load prior context via `/resume`

**Daemon state file** (local JSON or SQLite, e.g. `~/.mission-control/state.json`):
- Registry of projects and worktrees (paths, branches, color assignments)
- Last known status per worktree (running/needs_you/idle/failed)
- Last task/instruction per worktree
- Grid positions and UI layout state
- Cross-project links

The browser gets everything via WebSocket from the daemon. No external database needed.

### Restart recovery

Whether the daemon dies from laptop sleep, VM reboot, or crash, the recovery path is:

1. Daemon restarts (launchd on macOS, systemd on Linux, or manual)
2. Reads `state.json` — sees which worktrees existed
3. For each worktree: open a fresh pty, spawn `claude --continue` in the worktree directory (hooks still registered in that worktree's `.claude/settings.json`, so the status machine wires itself back up automatically via `SessionStart`)
4. `--continue` resumes the most recent conversation — full history, context, tool state — from `~/.claude/projects/<hash>/*.jsonl`
5. When the browser reconnects, each terminal pane re-attaches to its pty, receives the current (post-resume) scrollback, and continues streaming

**What survives restart:** conversation history (Claude Code's jsonl), project/worktree registry (daemon state), grid layout (daemon state), file changes in the working tree (on disk).

**What doesn't survive:** the pre-restart scrollback buffer (in-memory only) and any in-flight execution. If Claude Code was mid-write when the pty died, that operation is lost — but the conversation history knows what it was doing, so `--continue` can pick it back up.

**The /loop case:** `--continue` resumes the conversation but doesn't automatically re-send the loop command. Claude Code may ask "should I resume the loop?" on its own since the last conversation context was a running loop. If not, the user re-issues the loop in the terminal. Worth testing Claude Code's behavior here — ideal case is zero daemon complexity.

### Browser frontend

React app with a tiling workspace:

- **Project cards** — each shows status (from hook events), branch, current task, key context files (CLAUDE.md, NORTH_STAR.md)
- **Embedded Claude Code terminal per worktree** — `xterm.js` + `xterm-addon-fit` rendering the live pty stream from the daemon. This IS the chat interface — there is no custom-rendered conversation view, no markdown renderer, no tool-use block component.
  - Claude Code questions surface the same way they do in iTerm, inside the terminal
  - You respond inline with keystrokes — `y`, `n`, natural language, slash commands
  - `/compact`, `/clear`, `/resume` all just work because we're not reimplementing them
- **Tiling engine** — user-controlled splits: 1-up, 2-up, custom. System learns preferences.
- **Cross-project insights** — AI-generated connections ("Kinobi's Qwen3 eval could power OpenClaw's extraction agent")
- **Workspace-intent command bar** — see "The command bar" section above. Does not relay messages to worktrees.
- **Activity sidebar** — recent status-change events across all projects

### Key insight: not everything needs HITL simultaneously

Most of the time, 1-2 projects are in active conversation and the others are autonomous or paused. The UI makes this explicit — you see which projects are waiting for your input vs. running independently.

---

## Tech stack (tentative)

| Component | Tech | Rationale |
|-----------|------|-----------|
| Browser app | React + Vite + TypeScript + Tailwind + shadcn/ui | Spatial canvas, component library base |
| Terminal renderer | `xterm.js` + `xterm-addon-fit` | Reference web terminal; used by VS Code; handles ANSI/TUI faithfully |
| State management | Zustand | Single store, persist middleware for layout prefs |
| Daemon | Rust — `axum` + `tokio` | Async WebSocket server, robust pty handling |
| Pty management | `portable-pty` (Rust) | Cross-platform pty, same crate Wezterm uses |
| Hook transport | Unix domain socket + `mc-hook` shim | Structured status events without parsing stdout |
| Daemon state | Local JSON at `~/.mission-control/state.json` | No external dependencies, survives restarts |
| Conversation history | Claude Code's own storage (`~/.claude/`) | Already exists; `--continue` resumes from there |
| Claude Code integration | CLI subprocess (`claude --continue`) in a pty | Real interactive session, not structured JSON output |
| Voice (v2) | Web Speech API → Claude for intent parsing | Browser-native STT, command-bar verbs only |
| Cross-project AI (v2) | Claude API | Periodic sweep of project context to surface connections |

---

## MVP scope (revised)

**Phase 1 — Terminal-embed multi-project dashboard**
- Rust daemon with pty manager: one long-lived `claude` per worktree, scrollback buffer, attach/detach
- `mc-hook` shim + Unix socket listener for structured status events
- Browser UI with dot grid, project cards (hook-driven status), project tree view
- Embedded `xterm.js` terminal per worktree, binary WebSocket pty streaming
- Manual tiling (1-up, 2-up presets)
- Workspace-intent command bar (v1 verbs: pull up, open, close, back to grid, tree, new worktree)
- Run contracts visible in the terminal (Claude Code already shows them); daemon flips status to `needs_you` on `Notification` hook

**Phase 2 — Voice + smart workspace**
- Voice intent parsing ("show me what needs attention", "pull up Kinobi and Rangoli")
- Learned tiling arrangements
- Context reboarding on app open
- Conceptual breadcrumbs on project cards
- Activity feed across all projects
- Cross-project connections (manual)

**Phase 3 — Calm interface**
- Ambient project indicators
- Proactive surfacing without being asked
- AI-generated cross-project insights
- Voice-primary interaction
- Mobile-optimized responsive layout

---

## Open questions

### Resolved in this spec
- ~~How does the daemon handle long-running background tasks?~~ → Daemon-spawned sessions with output classification (routine/informational/attention/error).
- ~~What's the right granularity for session state?~~ → Claude Code stores conversation history itself (`~/.claude/`). Daemon stores only project/worktree registry and UI state.
- ~~Does Claude Code's Agent SDK offer better session management?~~ → Deferred. v1 uses CLI subprocess with `--continue` for restart recovery. Revisit if CLI approach hits limits.
- ~~Claude Code stdout format?~~ → Solved problem. Structured output available.
- ~~Permission model mapping?~~ → Verified. Claude Code has five modes (default, acceptEdits, plan, auto, bypassPermissions). The mode is set via `--permission-mode` flag at session startup. Approval prompts write to stdout, responses read from stdin — the daemon relays these to/from the browser. Per-project defaults live in `.claude/settings.json`, daemon can override per-worktree via CLI flag. The user configures permission mode per project or per worktree in the Mission Control UI, and the daemon passes the flag when spawning the session.
- ~~Worktree lifecycle?~~ → The daemon manages all worktree creation, not Claude Code. Lifecycle: create (daemon runs `git worktree add`, spawns claude session) → active → archive (user marks done, daemon cleans up git worktree directory) → delete. Stale worktrees (merged branches, abandoned conversations) surfaced periodically for cleanup.
- ~~Resource limits?~~ → The daemon manages concurrent session count based on host machine resources (CPU, memory). Hard limit enforced by daemon, excess worktrees queued. Agent harness for smarter scheduling is future scope.
- ~~Onboarding?~~ → User creates account on the webapp. First screen shows "no daemons connected" and guides user to install the daemon on their machine, start it, and connect. Daemon registers with the webapp on first run.

### Resolved by the terminal-embed direction
- ~~Chat rendering in browser (diffs, code blocks, tool use, streaming partial text)~~ → We don't render Claude Code's output in custom components. We embed the real terminal via `xterm.js` over a pty. Claude Code's TUI does the rendering; the browser just displays bytes.
- ~~The /compact and /clear problem~~ → `/compact` and `/clear` are slash commands the user types into the embedded terminal. Claude Code handles them natively. The daemon doesn't need to detect anything because there's nothing to refresh — the terminal redraws itself.
- ~~Output classification (routine/informational/attention/error)~~ → Replaced by Claude Code hook events (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `Notification`, `Stop`, `SessionEnd`). Hooks are structured ground truth. `Notification` → `needs_you`, `Stop` → `idle`, pty nonzero exit → `failed`.

### Can defer past v1

- Voice: Web Speech API vs. Whisper vs. Deepgram for STT? Latency requirements?
- Voice in noisy environments: push-to-talk vs. wake word vs. button?
- Voice intent parsing failure: best-guess "did you mean?" vs. fallback to text?
- Cross-project AI: how to implement the periodic sweep for insights?
- Mobile layout: how does the dot grid work on a phone screen?
- Multiple machines: can the daemon state sync across laptop + desktop?
- Agent harness: smarter scheduling of concurrent sessions based on task priority and resource availability