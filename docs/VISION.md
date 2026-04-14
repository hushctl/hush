# Mission Control — Vision & Design

Product rationale, research foundations, UX roadmap, and design philosophy. Not loaded automatically — read once for context, reference on demand.

---

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

## Open questions

### Deferred past v1

- Voice: Web Speech API vs. Whisper vs. Deepgram for STT? Latency requirements?
- Voice in noisy environments: push-to-talk vs. wake word vs. button?
- Voice intent parsing failure: best-guess "did you mean?" vs. fallback to text?
- Cross-project AI: how to implement the periodic sweep for insights?
- Mobile layout: how does the dot grid work on a phone screen?
- Multiple machines: can the daemon state sync across laptop + desktop?
- Agent harness: smarter scheduling of concurrent sessions based on task priority and resource availability
- Gemma 4 in-browser LLM — use Transformers.js with Gemma 4 1B Q4 quantized for command bar intent parsing and context reboarding summaries. Load lazily on first command bar focus.
