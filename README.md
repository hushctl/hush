# Mission Control

A browser-based command center for Claude Code. Run Claude Code sessions on one or more machines and control all of them from a single browser tab.

---

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+
- [Claude Code CLI](https://claude.ai/code) (`claude` must be on your PATH)

---

## Single machine (local)

**1. Build the daemon**

```sh
cd daemon
cargo build --release
```

Produces two binaries in `target/release/`:
- `mcd` — the daemon
- `mc-hook` — the Claude Code hook shim (must live next to `mcd`)

**2. Start the daemon**

```sh
./target/release/mcd
```

Listens on `0.0.0.0:9111` by default. State is persisted to `~/.mission-control/state.json`.

**3. Start the UI**

```sh
cd ui
npm install
npm run dev
```

Open [http://localhost:5173](http://localhost:5173).

**4. Add a project**

Click **+ project** in the command bar, enter the path to a Git repo, then enter a branch name (e.g. `main`). A Claude Code session starts in a pty — click the dot on the grid to open a terminal pane.

---

## Multiple machines over Tailscale

Each machine runs its own daemon. The browser connects to all of them and merges everything into one grid. Daemons gossip peer lists to each other, so adding one daemon in the UI is enough to discover the rest.

### On each machine

**1. Install Tailscale**

```sh
# macOS
brew install tailscale
sudo tailscaled &
tailscale up
```

Note the machine's Tailscale IP (`tailscale ip -4`) or use its MagicDNS hostname (`machine.tailnet-name.ts.net`).

**2. Build and start the daemon**

```sh
cd daemon
cargo build --release

./target/release/mcd \
  --advertise-url ws://$(tailscale ip -4):9111/ws \
  --machine-name my-laptop
```

`--advertise-url` tells peers how to reach this daemon. `--machine-name` is the label shown in the UI (defaults to `hostname`).

**3. Join an existing mesh (second machine onward)**

If another daemon is already running, pass `--join` with its WebSocket URL:

```sh
./target/release/mcd \
  --advertise-url ws://$(tailscale ip -4):9111/ws \
  --machine-name studio \
  --join ws://100.x.x.x:9111/ws
```

Within one gossip cycle (~30 seconds) every daemon will know about every other daemon.

### In the browser

Open the UI on any device on the Tailscale network. Click **+ daemon** in the command bar and enter the WebSocket URL of any one daemon (e.g. `ws://laptop.tailnet-name.ts.net:9111/ws`). The rest of the mesh auto-populates from the peer list within ~60 seconds.

---

## CLI flags

```
mcd [OPTIONS]

Options:
  -p, --port <PORT>              Port to listen on [default: 9111]
      --bind <ADDR>              Bind address [default: 0.0.0.0]
      --state-file <PATH>        State file path [default: ~/.mission-control/state.json]
      --machine-name <NAME>      Human-readable machine label (default: hostname)
      --advertise-url <URL>      WebSocket URL peers should use to reach this daemon
                                 Required for peer discovery (e.g. ws://host:9111/ws)
      --join <URL>               Seed peer URL to contact on startup (repeatable)
  -h, --help                     Print help
```

---

## Running two daemons on one machine (testing)

```sh
# Terminal 1
./target/release/mcd --port 9111 --machine-name laptop \
  --state-file ~/.mission-control/state.json \
  --advertise-url ws://localhost:9111/ws

# Terminal 2
./target/release/mcd --port 9112 --machine-name studio \
  --state-file ~/.mission-control/state-studio.json \
  --advertise-url ws://localhost:9112/ws \
  --join ws://localhost:9111/ws
```

In the UI, add `ws://localhost:9111/ws` — the second daemon (`studio`) will appear automatically after the first gossip round.

---

## Auto-start on macOS (launchd)

Create `~/Library/LaunchAgents/com.mission-control.daemon.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.mission-control.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>/path/to/mcd</string>
    <string>--advertise-url</string>
    <string>ws://YOUR_TAILSCALE_IP:9111/ws</string>
    <string>--machine-name</string>
    <string>YOUR_MACHINE_NAME</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/tmp/mcd.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/mcd.log</string>
</dict>
</plist>
```

```sh
launchctl load ~/Library/LaunchAgents/com.mission-control.daemon.plist
```

---

## How it works

```
Browser (any device)
  └── WebSocket per daemon ──► mcd (machine A)  ◄── mc-hook shim
                          └──► mcd (machine B)  ◄── mc-hook shim
```

- Each `mcd` owns its pty sessions, project registry, and state file.
- The browser namespaces IDs as `machineId:worktreeId` so dots from different machines never collide on the grid.
- Daemons gossip peer lists every 30 seconds — adding one daemon in the UI seeds the whole mesh.
- `mc-hook` is a small shim invoked by Claude Code's hook system on lifecycle events (`SessionStart`, `Stop`, `Notification`, etc.). It writes structured JSON to a Unix socket, which the daemon uses to update worktree status without parsing terminal output.
- Pty sessions survive browser disconnects (tmux model). Reconnect anytime; scrollback replays automatically.
