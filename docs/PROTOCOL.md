# Hush WebSocket Protocol

This document describes the WebSocket message protocol between the Hush browser app and daemon, and between daemon peers.

---

## Connections

### Browser → Daemon: `/ws`

Connect to `wss://<host>:9111/ws?token=<auth_token>`.

The auth token is a random 64-character hex string written to `~/.hush/auth_token` on first daemon start. The browser reads it from `/config/local` (loopback-only HTTP endpoint).

All JSON messages are sent as WebSocket text frames. Binary frames carry raw pty output (see [Binary Frames](#binary-frames)).

### Daemon → Daemon: `/peer`

Daemon-to-daemon communication uses `wss://<host>:9111/peer`.

**This endpoint requires a valid TLS client certificate** signed by the mesh CA (obtained via `hush invite` + `hush --join-token`). Connections without a cert receive `403 Forbidden` before the WebSocket upgrade.

Both browser and peer connections share the same TLS listener. The CA is installed into the OS trust store by `hush trust`, so browsers accept the daemon's certificate without warnings.

---

## Message Format

All messages are JSON objects with a `"type"` discriminant field:

```json
{ "type": "message_type", ...fields }
```

Types use `snake_case` (e.g. `"register_project"`, `"project_list"`).

`machine_id` in server messages identifies which daemon sent the message — important in multi-machine setups where the browser is connected to multiple daemons simultaneously.

`worktree_id` values are namespaced as `"<machine_id>:<raw_id>"` in the browser (e.g. `"laptop:wt_1"`). The daemon receives and sends raw IDs (e.g. `"wt_1"`) and the browser layer adds the namespace prefix.

---

## Client Messages (Browser → Daemon)

### Project management

#### `register_project`
Register an existing directory as a project. The daemon validates the path exists.
```json
{ "type": "register_project", "path": "/Users/me/myproject", "name": "My Project" }
```
Response: `project_list`

If the path does not exist: `path_not_found`

#### `create_and_register_project`
Create a missing directory, run `git init`, and register it.
```json
{ "type": "create_and_register_project", "path": "/Users/me/new", "name": "New Project" }
```
Response: `project_list`

#### `create_worktree`
Create a new git worktree on a branch within an existing project.
```json
{
  "type": "create_worktree",
  "project_id": "proj_1",
  "branch": "feature/foo",
  "permission_mode": "default"
}
```
`permission_mode`: `"plan"` | `"default"` | `"auto"` | `"dangerously-skip-permissions"` (default if omitted)

Response: `worktree_list`

#### `remove_worktree`
Remove a worktree record, kill its pty, and run `git worktree remove`.
```json
{ "type": "remove_worktree", "worktree_id": "wt_1" }
```
Response: `worktree_list`

#### `list_projects`
Request the current project list.
```json
{ "type": "list_projects" }
```
Response: `project_list`

#### `list_worktrees`
Request the current worktree list.
```json
{ "type": "list_worktrees" }
```
Response: `worktree_list`

---

### Terminal (Claude pty)

#### `pty_attach`
Attach to a worktree's Claude pty. Spawns `claude --continue` if not already running. The daemon replies with scrollback and then streams live output.
```json
{ "type": "pty_attach", "worktree_id": "wt_1", "cols": 220, "rows": 50 }
```
Response: `pty_scrollback` (immediate), then `pty_data` (live stream)

#### `pty_detach`
Stop streaming. The pty keeps running.
```json
{ "type": "pty_detach", "worktree_id": "wt_1" }
```

#### `pty_input`
Forward keyboard input to the pty stdin. Data is plain UTF-8; send `"\r"` for Enter.
If no pty is running, auto-spawns one first.
```json
{ "type": "pty_input", "worktree_id": "wt_1", "data": "hello\r" }
```

#### `pty_resize`
Resize the pty.
```json
{ "type": "pty_resize", "worktree_id": "wt_1", "cols": 200, "rows": 40 }
```

#### `pty_kill`
Kill the pty process.
```json
{ "type": "pty_kill", "worktree_id": "wt_1" }
```

#### `paste_image`
Paste an image into the pty. The daemon writes the bytes to `~/.hush/paste/` and injects the file path into pty stdin so Claude Code can read it.
```json
{
  "type": "paste_image",
  "worktree_id": "wt_1",
  "data": "<base64-encoded image bytes>",
  "filename": "screenshot.png"
}
```
`filename` is optional; a timestamp-based name is used if omitted.

---

### Shell pty (plain shell, not Claude)

#### `shell_attach`
Attach to a worktree's plain shell pty (bash/zsh). Each shell session has a unique `shell_id`.
```json
{ "type": "shell_attach", "worktree_id": "wt_1", "shell_id": "s1", "cols": 220, "rows": 50 }
```
Response: `shell_scrollback`, then `shell_data`

#### `shell_input`
```json
{ "type": "shell_input", "worktree_id": "wt_1", "shell_id": "s1", "data": "ls\r" }
```

#### `shell_resize`
```json
{ "type": "shell_resize", "worktree_id": "wt_1", "shell_id": "s1", "cols": 200, "rows": 40 }
```

#### `shell_kill`
```json
{ "type": "shell_kill", "worktree_id": "wt_1", "shell_id": "s1" }
```

---

### File operations

#### `git_status`
Request a one-shot git status snapshot.
```json
{ "type": "git_status", "worktree_id": "wt_1" }
```
Response: `git_status`

#### `list_files`
List all non-gitignored files in a worktree (for cmd+P).
```json
{ "type": "list_files", "worktree_id": "wt_1" }
```
Response: `file_list`

#### `read_file`
Read a file from a worktree's working directory (relative path, max 256 KB).
```json
{ "type": "read_file", "worktree_id": "wt_1", "path": "src/main.rs" }
```
Response: `file_content`

---

### Mesh

#### `list_peers`
Request the daemon's known peer list.
```json
{ "type": "list_peers" }
```
Response: `peer_list`

#### `peer_hello`
Daemon-to-daemon gossip greeting. Also accepted from browsers.
```json
{
  "type": "peer_hello",
  "machine_id": "laptop",
  "url": "wss://laptop.local:9111/ws",
  "peers": [...],
  "version": "0.13.2",
  "ca_cert_pem": "-----BEGIN CERTIFICATE-----\n..."
}
```

#### `transfer_worktree`
Move a worktree to another machine.
```json
{ "type": "transfer_worktree", "worktree_id": "wt_1", "dest_machine_id": "desktop" }
```

#### `transfer_project`
Move an entire project (all worktrees) to another machine.
```json
{ "type": "transfer_project", "project_id": "proj_1", "dest_machine_id": "desktop" }
```

#### `peer_upgrade`
Push the local binary to an older peer.
```json
{ "type": "peer_upgrade", "dest_machine_id": "desktop" }
```

---

## Server Messages (Daemon → Browser)

All server messages include `"machine_id"` identifying the originating daemon.

### `project_list`
```json
{
  "type": "project_list",
  "machine_id": "laptop",
  "projects": [
    { "id": "proj_1", "name": "My Project", "path": "/Users/me/proj", "worktree_count": 2, "machine_id": "laptop" }
  ]
}
```

### `worktree_list`
```json
{
  "type": "worktree_list",
  "machine_id": "laptop",
  "worktrees": [
    {
      "id": "wt_1",
      "project_id": "proj_1",
      "branch": "main",
      "working_dir": "/Users/me/proj",
      "status": "idle",
      "last_task": "write unit tests",
      "session_id": "abc123",
      "machine_id": "laptop",
      "shell_alive": false
    }
  ]
}
```

`status` values: `"idle"` | `"running"` | `"needs_you"` | `"failed: <message>"`

### `status_change`
Sent when a hook event transitions a worktree's status.
```json
{ "type": "status_change", "machine_id": "laptop", "worktree_id": "wt_1", "status": "running" }
```

### `pty_scrollback`
Initial scrollback replay on `pty_attach`. Data is base64-encoded raw terminal bytes.
```json
{ "type": "pty_scrollback", "machine_id": "laptop", "worktree_id": "wt_1", "data": "<base64>" }
```

### `pty_data`
Live pty output. Data is base64-encoded raw terminal bytes (ANSI sequences included).
```json
{ "type": "pty_data", "machine_id": "laptop", "worktree_id": "wt_1", "data": "<base64>" }
```

### `pty_exit`
Pty process exited.
```json
{ "type": "pty_exit", "machine_id": "laptop", "worktree_id": "wt_1", "code": 0 }
```
`code` is `null` if the process was killed by a signal.

### `shell_scrollback` / `shell_data` / `shell_exit`
Same shape as the pty equivalents, but include `"shell_id"`:
```json
{ "type": "shell_data", "machine_id": "laptop", "worktree_id": "wt_1", "shell_id": "s1", "data": "<base64>" }
```

### `error`
Generic error (e.g. worktree not found, spawn failure).
```json
{ "type": "error", "machine_id": "laptop", "message": "Worktree wt_99 not found", "worktree_id": "wt_99" }
```
`worktree_id` is `null` for non-worktree errors.

### `path_not_found`
Sent when `register_project` is called with a path that doesn't exist. The browser should ask the user if they want to create it.
```json
{ "type": "path_not_found", "machine_id": "laptop", "path": "/Users/me/new", "name": "New" }
```

### `peer_list`
```json
{
  "type": "peer_list",
  "machine_id": "laptop",
  "peers": [
    { "machine_id": "desktop", "url": "wss://desktop.local:9111/ws", "last_seen": 1713000000, "version": "0.13.2" }
  ],
  "version": "0.13.2"
}
```

### `git_status`
```json
{
  "type": "git_status",
  "machine_id": "laptop",
  "worktree_id": "wt_1",
  "staged": ["src/main.rs"],
  "modified": ["README.md"],
  "untracked": ["scratch.txt"]
}
```

### `file_list`
```json
{ "type": "file_list", "machine_id": "laptop", "worktree_id": "wt_1", "files": ["src/main.rs", "Cargo.toml"] }
```

### `file_content`
```json
{
  "type": "file_content",
  "machine_id": "laptop",
  "worktree_id": "wt_1",
  "path": "src/main.rs",
  "content": "fn main() { ... }",
  "truncated": false
}
```
`truncated` is `true` if the file was larger than the 256 KB read limit.

### `memory_pressure`
Only sent on transitions between levels (not on every poll).
```json
{ "type": "memory_pressure", "machine_id": "laptop", "level": "warning", "available_bytes": 2000000000, "total_bytes": 16000000000 }
```
`level`: `"normal"` | `"warning"` | `"critical"`

### Transfer progress messages

| Type | When |
|---|---|
| `transfer_ack` | Destination accepted; includes `dest_path` |
| `transfer_progress` | Periodic; includes `phase`, `bytes_sent`, `total_bytes` |
| `transfer_complete` | Success; includes `new_worktree_id` on destination |
| `transfer_error` | Failure; includes `message` |

`phase` values: `"starting"` | `"streaming"` | `"extracting"` | `"installing_history"` | `"spawning_pty"` | `"complete"` | `"failed"`

### Upgrade progress messages

| Type | When |
|---|---|
| `upgrade_ack` | Destination ready to receive binary |
| `upgrade_progress` | Periodic; includes `bytes_sent`, `total_bytes` |
| `upgrade_complete` | Upgrade applied; destination is restarting |
| `upgrade_error` | Failure |

---

## Binary Frames

Raw pty and transfer bytes are sent as WebSocket **binary frames**.

### Pty output
Pty output is also sent as JSON `pty_data` / `pty_scrollback` with base64-encoded data. There are no raw binary pty frames in the current protocol.

### Transfer and upgrade streams
During worktree transfer and peer upgrade, the source daemon sends raw binary frames after the offer/ack handshake. Frames carry compressed tar data (`tar.gz`) and are streamed to completion before `transfer_commit` / `upgrade_commit` is sent.

---

## Hook Socket Protocol

The `hush-hook` shim writes to a Unix socket at `~/.hush/hooks.sock` (or the state dir equivalent for multiple daemons). Each connection carries exactly one JSON line:

```json
{ "event": "session_start", "worktree_id": "wt_1", "payload": {...} }
```

### Event → Status mapping

| Hook event | Worktree status |
|---|---|
| `session_start` | `running` |
| `user_prompt` | `running` (also sets `last_task` from `payload.prompt`) |
| `pre_tool_use` | `running` |
| `notification` | `needs_you` |
| `stop` | `idle` |
| `session_end` | `idle` |

Unknown events are silently ignored (forward-compatible).

Each hook line may carry a `payload` object. Notable fields:
- `user_prompt.payload.prompt` — the user's message text, stored as the worktree's `last_task`
- `*.payload.session_id` — Claude Code session identifier, stored for `--continue`

---

## Connection lifecycle

```
Browser                         Daemon
  |                               |
  |-- GET /ws?token=... --------> |  (HTTP upgrade)
  |<-- 101 Switching Protocols -- |
  |                               |
  |<-- worktree_list ------------ |  (on connect: current state)
  |<-- project_list ------------- |
  |                               |
  |-- pty_attach worktree_id ---> |
  |<-- pty_scrollback ----------- |  (buffered history)
  |<-- pty_data (stream) -------- |  (live output)
  |                               |
  |-- pty_input "hello\r" ------> |
  |<-- pty_data (echo) ---------- |
  |                               |
  |  [Claude hook fires]          |
  |<-- status_change "running" -- |
  |<-- worktree_list ------------ |  (updated last_task)
  |                               |
  |  [Claude done]                |
  |<-- status_change "idle" ----- |
```
