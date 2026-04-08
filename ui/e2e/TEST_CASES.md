# Mission Control — Playwright test cases

These are the acceptance tests. The project is successful when all of these pass.
Tests are grouped by screen/flow. Each test describes the user action and the expected outcome.
Tests assume the daemon is running with at least 2 registered projects, each with 1-2 worktrees.

---

## 0. Onboarding

### 0.1 first launch — no daemon
- open app in browser
- expect: screen shows "no daemon connected"
- expect: instructions to install and start the daemon are visible
- expect: no dot grid, no project cards, no input bar

### 0.2 daemon connects
- start daemon on localhost
- expect: app detects daemon via WebSocket within 3 seconds
- expect: screen transitions from "no daemon" to the dot grid (empty if no projects registered)

### 0.3 register first project
- with daemon connected and zero projects
- send register project command (via input bar or daemon API)
- expect: project appears on the dot grid as a single worktree dot
- expect: dot is gray/idle (no active session yet)
- expect: daemon state.json now contains the project entry

---

## 1. Dot grid — the glance

### 1.1 grid renders with background dots
- open app with daemon running and 2+ projects registered
- expect: background dot grid fills the viewport
- expect: dots are square (no border-radius)
- expect: no bold text anywhere on screen

### 1.2 worktree dots appear at correct positions
- expect: each worktree has a dot on the grid
- expect: dots with "needs you" status are positioned top-left (high urgency)
- expect: dots with "idle" status are positioned bottom-right (low urgency)
- expect: dot size corresponds to urgency: needs_you > failed > running > idle

### 1.3 dot color matches status
- expect: running worktree dot is green
- expect: needs_you worktree dot is amber
- expect: idle worktree dot is gray
- expect: failed worktree dot is red

### 1.4 gravity wells on background
- expect: background grid dots near a project dot are tinted with the project's status color
- expect: background grid dots near a project dot are slightly larger than distant dots
- expect: a project cluster with 2 active worktrees has a denser warp than a single idle worktree

### 1.5 labels are right-aligned against dots
- expect: project name label sits to the left of the dot, right-aligned against it
- expect: project name is uppercase with letter spacing
- expect: status and time sit below the name
- expect: all labels are font-weight 400

### 1.6 hover expands detail card
- hover over a worktree dot
- expect: a flat detail card appears (no border-radius)
- expect: card shows branch name, status pill, breadcrumb, metadata
- expect: card shows action buttons if status is needs_you or failed
- move mouse away from dot
- expect: card disappears

### 1.7 context reboarding bar
- open app after projects have been active
- expect: a flat bar at the top summarizes current state
- expect: bar mentions worktrees that need attention
- expect: bar is a single line, no chrome

### 1.8 project clusters
- register a project with 2 worktrees
- expect: project name label appears once
- expect: two worktree dots appear below the label, each with branch name
- expect: both contribute to the background gravity well

---

## 2. Input routing

### 2.1 default routing — all projects
- on dot grid, do not click any dot or tab
- expect: input bar shows "talking to all worktrees..."

### 2.2 tab selection routes input
- click a project tab above the input bar
- expect: tab highlights as active
- expect: input bar updates to "talking to [project name]..."
- click "all projects" tab
- expect: input bar returns to "talking to all worktrees..."

### 2.3 dot click routes input
- click a worktree dot on the grid
- expect: input bar updates to "talking to [project] / [branch]..."
- expect: corresponding project tab highlights

### 2.4 routing persists until changed
- click a worktree dot to route
- type a message and send
- expect: message is sent to the routed worktree
- expect: input bar still shows the same routing after send

### 2.5 sending a message to a routed worktree
- route to a specific worktree
- type "hello" in the input bar and press enter
- expect: message appears in that worktree's chat thread
- expect: daemon receives the message for the correct worktree
- expect: claude code response streams back and appears in the chat

---

## 3. Single project — the dive

### 3.1 open single project from grid
- click a worktree dot on the grid
- type a message in the input bar
- expect: grid recedes, single pane chat view appears
- expect: pane header shows project name, branch, status pill
- expect: chat thread shows conversation history
- expect: input box at bottom shows "message [project] / [branch]..."
- expect: mic button is visible next to input

### 3.2 back to grid
- in single pane view, click "← grid"
- expect: pane recedes, dot grid returns
- expect: worktree dot reflects any status changes from the session

### 3.3 top bar shows all projects
- in single pane view
- expect: top bar shows all project names with status dots
- expect: active project is highlighted
- expect: other projects' dots reflect their current status

### 3.4 status change during session
- in single pane view, send a message that triggers claude code to ask for approval
- expect: status pill in pane header changes to "needs you"
- expect: approval card appears in chat thread with approve/diff/discuss buttons

### 3.5 approve action from chat
- with an approval card visible in chat
- click "approve" button
- expect: approval card is replaced by a confirmation message
- expect: claude code continues execution
- expect: status pill changes from "needs you" to "running"

---

## 4. Split view — multi-focus

### 4.1 open two projects side by side
- from single pane view of project A
- trigger "also pull up [project B]" (via input or UI action)
- expect: screen splits into two equal panes
- expect: left pane shows project A, right pane shows project B
- expect: each pane has its own chat thread, header, and input box
- expect: panes are separated by a 0.5px border

### 4.2 independent input per pane
- in split view with two panes
- click input box in left pane
- type a message and send
- expect: message goes to left pane's worktree only
- expect: right pane is unaffected
- click input box in right pane
- type a different message and send
- expect: message goes to right pane's worktree only

### 4.3 close one pane
- in split view, close the right pane
- expect: left pane expands to full width
- expect: closed project returns to the top bar as a status dot

### 4.4 swap a pane
- in split view with project A (left) and project B (right)
- trigger "swap [project B] for [project C]"
- expect: right pane changes to project C
- expect: project B returns to top bar
- expect: left pane (project A) is completely unaffected — no scroll position change, no re-render

### 4.5 back to grid from split
- in split view, click "← grid"
- expect: both panes recede, dot grid returns
- expect: all worktree dots reflect current status

### 4.6 notification in split view
- in split view working on project A + B
- project C's status changes (e.g. idle → needs_you)
- expect: project C's dot in the top bar changes color to amber
- expect: no forced context switch — current panes remain undisturbed
- expect: notification is visible but not intrusive

---

## 5. Project tree view

### 5.1 open project tree from grid
- click a project cluster label on the dot grid
- expect: project tree view opens
- expect: left side shows vertical tree with connecting line
- expect: each worktree is a node on the tree

### 5.2 worktree nodes show correct info
- in project tree view
- expect: each node shows branch name (monospace), status pill, task name, last message preview
- expect: each node shows message count, files modified count, time since active
- expect: node with "needs you" status has amber border

### 5.3 click worktree shows conversation preview
- click a worktree node in the tree
- expect: right panel shows full conversation history for that worktree
- expect: input box at bottom of right panel allows sending messages

### 5.4 create new worktree
- click "+ new worktree" at bottom of tree
- expect: prompt for branch name
- provide branch name
- expect: daemon creates git worktree in the project directory
- expect: new node appears in the tree with "idle" status
- expect: daemon spawns a new claude code session for this worktree

### 5.5 back to grid from tree view
- click "← grid" from project tree view
- expect: returns to dot grid
- expect: any new worktrees created are visible as dots

---

## 6. Card states

### 6.1 running state
- worktree is actively executing
- expect: green dot, "running" pill
- expect: card shows current task name
- expect: card shows breadcrumb (plain english, not logs)
- expect: card shows metadata (time, files read, files modified)
- expect: no action buttons

### 6.2 needs you state
- worktree is waiting for approval
- expect: amber dot, "needs you" pill, amber border on hover card
- expect: card shows what it's waiting for and why
- expect: card shows approve, diff, discuss buttons
- expect: card shows how long it's been waiting

### 6.3 idle state
- worktree session ended
- expect: gray dot, "idle" pill
- expect: card shows last session summary
- expect: card shows what's queued next (if anything)
- expect: card shows time since last active

### 6.4 failed state
- worktree hit an error
- expect: red dot, "failed" pill, red border on hover card
- expect: card shows error in plain english
- expect: card shows rollback status
- expect: card shows resume, logs, retry buttons

### 6.5 status transitions
- start a worktree (idle → running)
- expect: dot changes gray → green, pill updates, dot drifts on grid
- worktree asks for approval (running → needs_you)
- expect: dot changes green → amber, pill updates, dot drifts toward urgent corner
- approve the action (needs_you → running)
- expect: dot changes amber → green, pill updates
- worktree errors (running → failed)
- expect: dot changes green → red, pill updates, dot drifts toward urgent corner

---

## 7. Chat rendering

### 7.1 user message appears
- type a message in the input and send
- expect: message appears right-aligned in the chat thread
- expect: message has distinct styling from AI responses

### 7.2 AI response streams in
- send a message that triggers a response
- expect: response appears left-aligned in the chat thread
- expect: response renders markdown (code blocks, inline code, etc.)

### 7.3 file modification tags
- claude code modifies files during execution
- expect: file tags appear in the chat (filename + action)
- expect: file tags are compact, monospaced

### 7.4 system events
- claude code runs a tool or completes a step
- expect: muted inline status line appears ("modified 2 files", "running tests...")
- expect: system events are visually distinct from user/AI messages

### 7.5 approval card in chat
- claude code requests permission for an action
- expect: structured approval card appears in chat
- expect: card shows what action is proposed
- expect: card has approve / diff / discuss buttons
- click approve
- expect: card updates to show approval confirmed
- expect: claude code continues execution

---

## 8. Daemon integration

### 8.1 daemon connection lifecycle
- start app without daemon running
- expect: "no daemon connected" state
- start daemon
- expect: app connects within 3 seconds
- stop daemon
- expect: app shows "daemon disconnected" within 5 seconds
- expect: last known state of all projects is preserved in UI

### 8.2 message roundtrip
- route to a worktree, send a message
- expect: daemon receives message via WebSocket
- expect: daemon writes message to claude code stdin
- expect: claude code response appears in browser within reasonable time

### 8.3 state persistence
- register projects and create worktrees
- stop daemon
- restart daemon
- expect: all projects and worktrees reappear
- expect: daemon runs `claude --continue` for previously active worktrees
- expect: status shows "resuming" briefly, then settles

### 8.4 permission mode per worktree
- create two worktrees with different permission modes (e.g. default, acceptEdits)
- expect: worktree A prompts for approval on file edits
- expect: worktree B auto-approves file edits, only prompts for bash

### 8.5 concurrent sessions
- have 4+ worktrees running simultaneously
- send messages to different worktrees in quick succession
- expect: each message reaches the correct worktree
- expect: responses from different worktrees don't cross-contaminate
- expect: UI updates for each worktree independently

### 8.6 conversation replay on connect
- have a worktree with existing conversation history (messages sent previously)
- refresh the browser page
- open that worktree's chat view
- expect: prior conversation messages appear in the chat thread
- expect: user messages, AI responses, file tags, system events all render
- expect: no visual difference between replayed and live messages
- send a new message
- expect: new message appends after replayed history normally

### 8.7 conversation replay on worktree switch
- in split view with project A and project B
- close project B, open project C (which has prior conversation history)
- expect: project C's chat thread shows its full conversation history
- expect: project A's chat thread is unaffected

### 8.8 conversation replay after daemon restart
- have active conversations on 2 worktrees
- stop daemon, restart daemon
- open a worktree's chat view in the browser
- expect: conversation history from before the restart is visible
- expect: claude code session is resumed via --continue
- expect: new messages from the resumed session append after the history

### 8.9 conversation replay after /compact
- have a worktree with a long conversation
- trigger /compact in that session
- expect: daemon detects the compaction
- expect: browser refreshes the chat view
- expect: chat shows the compacted summary, not the original verbose messages

### 8.10 conversation replay after /clear
- have a worktree with conversation history
- trigger /clear in that session
- expect: daemon detects the clear
- expect: browser clears the chat view
- expect: chat is empty, ready for new messages

---

## 9. Visual language enforcement

### 9.1 no border-radius anywhere
- inspect all cards, buttons, inputs, dots, hover panels
- expect: border-radius is 0 on every element

### 9.2 no bold text
- inspect all text on all screens
- expect: font-weight is 400 everywhere (no 500, 600, 700)

### 9.3 no gradients or shadows
- inspect all elements
- expect: no box-shadow, no background gradient, no text-shadow

### 9.4 flat design consistency
- navigate through all screens (grid, single pane, split, tree view)
- expect: consistent 0.5px borders
- expect: consistent flat color fills
- expect: consistent typography scale
