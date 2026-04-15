# Hush — Vision & Design

Product rationale, north star, research foundations, and UX roadmap.

---

## North star

**One screen for all your Claude Code sessions, across all your machines.**

You run Claude Code on your laptop, your desktop, your cloud box. Today that means SSH-ing around, losing track of which session is where, and never seeing the full picture. Hush gives you a single spatial interface — open a browser tab, see every session on every machine, dive into any of them.

The two pillars:

1. **Multi-session orchestration** — Run N Claude Code sessions in parallel. Visual status (green/amber/red) tells you where your attention is needed. Sessions survive browser disconnects. Context-switch in one click.

2. **Multi-machine mesh** — Every machine runs a `hush` daemon. Daemons discover each other via gossip. The browser is a viewport into the entire mesh. Build on your laptop, deploy from your server, monitor both from your iPad.

These are not separate features. The mesh is the product. A single-machine Hush is useful; a meshed Hush across your machines is the thing nobody else does.

---

## What this is

A browser-based multi-project command center for Claude Code. Instead of switching between iTerm tabs across multiple machines, you manage all your active Claude Code sessions from a single spatial interface — accessible from any device with a browser.

---

## The problem

When you're running 3-4 projects simultaneously across machines:

- Multiple iTerm tabs, SSH sessions, each with a Claude Code session
- Tab-switching to check on progress, losing spatial context
- No cross-project visibility — you can't see all projects at a glance
- Locked to whatever machine or SSH session you started from
- No way to spot cross-project patterns or reusable insights
- A dead laptop means lost terminal sessions (unless you remembered tmux)

---

## Why multi-machine is day-one, not v2

Most tools treat multi-machine as an afterthought bolted on later. That leads to architectures that fundamentally assume one machine and then leak abstractions everywhere when you add more.

Hush's architecture is multi-machine from the start:

- **Namespaced IDs** — every worktree is `machineId:worktreeId`. No collisions, no "which machine is this?" ambiguity.
- **Gossip-based discovery** — add one peer, discover the whole mesh within 30 seconds. No central registry, no config files listing every machine.
- **P2P upgrades** — build a new binary on one machine, it propagates to all peers over the existing TLS WebSocket. No GitHub access needed on receiving machines.
- **Browser connects to N daemons** — the UI opens one WebSocket per daemon and merges everything into a single grid. The user doesn't think about machines; they think about projects.
- **Pty sessions are daemon-local** — each daemon owns its processes. The browser is a thin viewport that can disconnect and reconnect without losing anything.

The mesh is not a feature. It's the topology.

---

## Design philosophy

### The canvas is infinite but attention is finite

The primary design tension: you have N parallel projects across M machines, one human brain. The interface must let you see everything at a glance while diving deep into any project without losing context on the rest.

### The interface is calm, not absent

The long-term vision is intent-driven and ambient — you speak, the UI responds, surfaces recede when not needed. But we build toward that gradually. v1 embeds the real Claude Code terminal in the browser for the actual conversation with the agent, and layers a workspace-intent command bar on top for navigation and voice. The futuristic stuff earns its way in as the product proves itself.

**The progression is: type, then talk, then glance.**

Day one, most people type. Day thirty, they're talking. Day ninety, they forget the keyboard is there. Each version is fully usable on its own — nobody waits for v3 to get value.

### Workspace grammar, not prescribed layouts

We design the **tiling system** — the mechanics of how project panes split, resize, and snap — not the arrangements themselves. Users write their own layouts; we provide the grammar.

Key principles for the tiling engine:
- The user expresses intent ("add my-project to my workspace"), the system handles geometry
- Panes have responsive breakpoints: full-width shows the live terminal + status chrome; quarter-width collapses to status + last activity + notification badge
- The system learns common pairings and offers them as one-click arrangements
- Arrangements are temporary and fast to create/destroy (Arc browser's split-view model)
- Spatial stability: projects stay where you put them across sessions

Study: i3wm/Hyprland (tiling WMs), Arc browser split view, tmux, Amethyst/Yabai.

---

## Research foundations

### Intent-based outcome specification (Jakob Nielsen)

Nielsen identifies AI as the third UI paradigm in computing history — after batch processing and command-based interaction. The user states the desired result; the system determines the procedure.

**Key concepts directly applicable to Hush:**

- **Run contracts** — Before the AI executes, show: estimated time, cost/scope, definition of "done", hard boundaries (what it won't touch). Maps to: when you say "start task X on project-a", the system shows what it'll do before doing it.
- **Conceptual breadcrumbs** — For long-running tasks, synthesized summaries of intermediate conclusions, not raw logs. Maps to: project cards showing "project-a: extraction pipeline 60% done, found 3 edge cases in retry logic" not a wall of terminal output.
- **Context reboarding** — When a user returns after switching away, a resumption summary reminds them of original intent, key decisions, and current status. Maps to: "Welcome back. Project-a finished its benchmark — results in /eval. Project-b is waiting for your approval on the config change. Project-c is idle."
- **The articulation barrier** — Half of users struggle to express intent in prose. Voice lowers this barrier. GUI affordances (buttons, status indicators) remain essential as fallback and confirmation.

**Reading list:**
- "AI Is First New UI Paradigm in 60 Years" (Nielsen, 2023) — jakobnielsenphd.substack.com
- "Intent by Discovery: Designing the AI User Experience" (Nielsen, 2025) — jakobnielsenphd.substack.com
- "The Articulation Barrier" (Nielsen, 2023) — linkedin.com

### Calm technology (Mark Weiser & John Seely Brown, Xerox PARC)

Weiser's thesis: the most profound technologies are those that disappear, weaving into everyday life until indistinguishable from it. Calm technology informs but doesn't demand focus or attention.

**Key principles for Hush:**

- **Center and periphery** — Design for both. Active project is center; other projects communicate state through periphery (color, subtle indicators, badge counts). A glance tells you "project-a is running, project-b needs me, project-c is idle" in under a second.
- **Calmness as design goal** — If computers are everywhere they better stay out of the way. The people being shared by the computers must remain serene and in control.
- **The "Dangling String" model** — Weiser's example of an 8-foot string connected to network traffic. The more traffic, the more it spins. No screen, no numbers — just ambient awareness. Our project cards should have this quality at rest.

**Reading list:**
- "The Computer for the 21st Century" (Weiser, 1991) — Scientific American / calmtech.com
- "The Coming Age of Calm Technology" (Weiser & Brown, 1996) — calmtech.com
- "Calm Technology: Principles and Patterns for Non-Intrusive Design" (Amber Case, 2015) — O'Reilly

### Zero UI / ambient interfaces

The emerging field of interfaces that fade into the background, replacing screen-based navigation with voice, gesture, and contextual awareness.

**Key caution:** Invisible doesn't mean infallible. Context confusion erodes confidence fast — the more seamless the system seems, the more jarring it feels when it fails. Hush must have graceful fallback from voice to visual to manual.

**Reading list:**
- "Ambient AI in UX: Interfaces That Work Without Buttons" (2026) — medium.com
- "Zero UI: Beyond the Screen" — medium.com/design-bootcamp
- "Shaping Interfaces With Intent" (Buildo, 2025) — buildo.com
- "Generative UI: Smart, Intent-Based, and AI-Driven" (2025) — medium.com/design-bootcamp
- IxDF's "What is No-UI Design?" resource page — interaction-design.org

### Hybrid interface model (Nielsen's recommendation)

Nielsen advocates for hybrid UIs combining intent-based natural language with GUI controls. This is our v1 strategy — but with a critical split:

- **Conversation with the agent** happens in the real Claude Code terminal, embedded in the browser. Zero mimicry, full fidelity, slash commands and tool approvals just work.
- **Intent about the workspace** happens in the command bar. "Pull up project-a and project-b", "back to grid", "close project-c" — outcome statements the system translates into layout changes.
- **Visual state** comes from project cards and the dot grid, driven by structured status events the daemon receives from Claude Code hooks (not parsed from terminal output).

Two surfaces, two jobs. The terminal is where work happens; the command bar is where workspace intent is expressed; the visual layer is ambient awareness. Neither alone is sufficient, and critically, they don't overlap — typing into the command bar does not route messages to the agent (that's what the terminal is for).

---

## UX evolution — from chat to calm

### v1: Terminal + command bar + mesh

The foundation. People already know how to use a terminal, and the command bar is a single new thing to learn.

- **Embedded Claude Code terminal per worktree** — the primary conversation surface. A real `claude` process in a pty, rendered with xterm.js. You type, approve, use slash commands exactly as you would in iTerm.
- **Project cards with status** — structured grid or tiling layout. Each card shows: project name, branch, machine, status (running/needs_you/idle/failed), one-line current task. Driven by hook events, not terminal parsing.
- **Manual tiling** — click a dot to open a terminal pane, split-view two worktrees (even across machines), simple preset layouts (1-up, 2-up side-by-side).
- **Workspace-intent command bar** — bottom of the screen. Accepts outcome statements like "pull up project-a and project-b", "back to grid", "close project-c". Does *not* forward typed text to any worktree — the terminal is where you talk to Claude.
- **Multi-machine mesh** — each machine runs a daemon, daemons discover each other via gossip, the browser merges all machines into one grid. Add one peer URL and the rest auto-populate.
- **P2P upgrades** — build once, propagate everywhere. No coordinated deployment, no CI pipeline needed for your personal mesh.
- **Run contracts** — visible inside the embedded terminal, because Claude Code already shows them there. The daemon observes `Notification` hooks to flip the worktree status to `needs_you` and amber-border the card.
- **Mic button** on the command bar, not on the terminal. Speech-to-text becomes a command-bar utterance. The terminal stays keyboard-driven in v1 because that's how Claude Code works.

### v2: Voice becomes natural

Voice starts doing more than transcription.

- **Intent parsing** — "show me what needs my attention" filters the UI to projects with pending approvals or completed tasks. "Pull up project-a and project-b side by side" rearranges the tiling.
- **Smart suggestions** — system proactively surfaces: "Project-a finished its benchmark. Project-b is waiting for approval. Project-c has been idle for 3 hours."
- **Context reboarding** — when you open the app after being away, a summary greets you: what happened, what changed, what needs you. Spans all machines.
- **Learned tiling** — system remembers your common project pairings and offers them.
- **Conceptual breadcrumbs** — project cards show synthesized progress, not raw logs.

### v3: The interface is calm

Voice is primary. The visual layer is ambient.

- **The command bar recedes** — it's still there, but voice is the default way to express workspace intent. The dot grid is the calm canvas; terminals open only when you're actually working in one.
- **Proactive surfacing** — the system doesn't wait for you to ask. It tells you what matters when you arrive.
- **Cross-project synthesis** — AI notices patterns across codebases and surfaces them without being asked.
- **The terminal stays a terminal.** Always. Conversation with Claude Code never gets mimicked or voice-translated — you either type into the embedded terminal, or you speak intent to the command bar and let it open the right terminals for you. The keyboard is always one click away.

---

## Open questions

### Deferred past v1

- Voice: Web Speech API vs. Whisper vs. Deepgram for STT? Latency requirements?
- Voice in noisy environments: push-to-talk vs. wake word vs. button?
- Voice intent parsing failure: best-guess "did you mean?" vs. fallback to text?
- Cross-project AI: how to implement the periodic sweep for insights?
- Mobile layout: how does the dot grid work on a phone screen?
- Agent harness: smarter scheduling of concurrent sessions based on task priority and resource availability
- Gemma 4 in-browser LLM — use Transformers.js with Gemma 4 1B Q4 quantized for command bar intent parsing and context reboarding summaries. Load lazily on first command bar focus.

### Mesh hardening (post-v1)

- **Auth between peers** — currently no authentication. Needs mutual TLS or token auth for untrusted networks. Tailscale is the shortcut for now (encrypted mesh + identity for free).
- **Session/credential sync** — if the mesh grows, do auth sessions (cookies, tokens for external services) need to be transferable between machines?
- **Selective mesh visibility** — should you be able to share some worktrees with a teammate's Hush but not all?

---

## The true north — a terminal-first, AI-native distro

Everything above is the public launch story. Below is where this is actually going.

Hush is not a tool. It's a **terminal-first, AI-native distro** — Claude Code is the shell, the terminal is everything, Hush is the compositor.

The browser-as-desktop model is proven (ChromeOS). Hush already has the primitives: spatial canvas (virtual desktops), daemon process management, peer gossip across machines, hook-driven status. The goal is not a full Linux DE but a daily-driver environment where Claude Code is the primary interface for everything you do on a computer.

*"Your OS has a terminal. This terminal has an OS."*

### AI-native browsing

URLs don't render as HTML. They render as **AI-distilled terminal views** with numbered clickable options.

**The pipeline:**

1. **Fetch** — A headless content extraction layer handles JS-heavy SPAs, rendering, and parsing. Hush owns auth gates (cookies, sessions, login flows).
2. **Distill** — AI reads the extracted content and generates a terminal-native summary. Local Gemma 4 handles simple pages; Claude API for complex ones.
3. **Render** — Output is A2UI components rendered natively in the terminal pane.
4. **Navigate** — Numbered options. Click a number (or say it), the agent fetches that link, re-distills, re-renders. The interaction loop is: render → choose → render.

**Per-domain preference learning** shapes what AI foregrounds over time. Images render inline via kitty/sixel protocol. Video playback is a TODO (explore mpv integration, frame extraction, or terminal graphics protocols).

### A2UI as the rendering layer

[A2UI](https://a2ui.org/) (Agent-Driven Interface Protocol) by Google, Apache 2.0, v0.8 stable / v0.9 draft.

A declarative JSON-based format for AI agents to generate rich UI without executing code. Agents describe components; clients render natively. Supports streaming/incremental generation, custom component catalogs, and cross-platform rendering.

**How it fits:**

- Hush defines a **terminal-native component catalog** — headings, text blocks, `link_list` (numbered options), inline images, tables, code blocks, video thumbnails.
- AI generates A2UI JSON describing the page in these components.
- Hush renders the A2UI natively in the terminal pane. No browser engine, no DOM.
- **Streaming support** means pages render progressively — first content appears while the rest is still being distilled.
- The agent interaction loop (click option → agent responds → re-render) maps directly to the numbered-options navigation model.

### The distro is not one machine — it's the mesh

Each machine runs a Hush daemon. The browser is a viewport into all of them on one spatial canvas. Peer gossip for discovery, worktree transfer for moving work between machines, any browser connects to any daemon.

For the AI-native browser, content extraction can run on whichever machine has best connectivity. Auth sessions need to be per-machine or transferable across the mesh.

### What the distro still needs

| Gap | Description |
|---|---|
| **Shell panes in UI** | Expose ShellAttach/ShellInput protocol in the browser — plain bash/zsh escape hatch alongside Claude panes |
| **System-level worktrees** | Workspaces not tied to a git repo (e.g. "home" or scratch workspace for general machine tasks) |
| **OS notifications** | Bridge `needs_you` status to Web Notification API so users can be in another app |
| **Quick file preview** | Image/markdown/log preview in a panel without Claude |
| **Persistent cross-worktree task queue** | "Do X when Y finishes" across projects |
| **Terminal video playback** | Videos from web pages playing within the terminal — mpv, frame extraction, or terminal graphics protocols |
| **Peer auth + tunneling** | Mutual TLS or token auth for untrusted networks. Tailscale is the shortcut. |
| **Cross-mesh credential sync** | Browser auth sessions (cookies, tokens) transferable when AI-native browser fetches through remote daemons |
