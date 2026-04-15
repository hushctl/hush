# Hush

One screen for all your Claude Code sessions, across all your machines.

<!-- Re-record with: make demo -->
![Hush demo](docs/demo.gif)

Run Claude Code on your laptop, your desktop, your cloud box — and control all of them from a single browser tab. Hush gives you a spatial canvas where every session on every machine is visible at a glance.

- **Multi-session orchestration** — Run N Claude Code sessions in parallel. Status dots (green/amber/red) tell you where your attention is needed.
- **Multi-machine mesh** — Each machine runs a daemon. Daemons discover each other via gossip. Open one browser tab, see everything.
- **Sessions survive disconnects** — Close your laptop, Claude keeps working. Reconnect later, scrollback replays automatically.
- **P2P upgrades** — Build once, propagate to the whole mesh. No CI, no GitHub access needed on receiving machines.

---

## Quick start

### Prerequisites

- [Rust](https://rustup.rs/) (for building)
- [Node.js](https://nodejs.org/) 18+ (for building the UI)
- [Claude Code CLI](https://claude.ai/code) (`claude` must be on your PATH)

### Build and install

```sh
git clone https://github.com/nicholasgasior/hush
cd hush
make install
```

This builds the daemon + UI and installs to `~/.local/bin/` and `~/.hush/ui/`.

Add `~/.local/bin` to your PATH if it isn't already:

```sh
# Add to ~/.zshrc or ~/.bashrc
export PATH="$HOME/.local/bin:$PATH"
```

### Run

```sh
hush
```

On first run, Hush generates a TLS certificate authority and installs it into your OS trust store (macOS will prompt for your password once). After that, open **https://localhost:9111** in your browser.

Click **+ project** in the command bar, enter the path to a Git repo, then enter a branch name. A Claude Code session starts — click the dot on the grid to open a terminal pane.

---

## Multiple machines

Each machine runs its own `hush` daemon. The browser connects to all of them and merges everything into one grid. Daemons gossip peer lists, so adding one daemon URL is enough to discover the rest.

### Setup

**1. Install Tailscale on each machine** (for encrypted networking)

```sh
# macOS
brew install tailscale && sudo tailscaled & && tailscale up
```

**2. Build and install Hush on each machine** (or use P2P upgrades after the first)

```sh
make install
```

**3. Start the first machine**

```sh
hush \
  --advertise-url wss://$(tailscale ip -4):9111/ws \
  --machine-name laptop \
  --auto-upgrade
```

**4. Join from additional machines**

```sh
hush \
  --advertise-url wss://$(tailscale ip -4):9111/ws \
  --machine-name studio \
  --join wss://100.x.x.x:9111/ws \
  --auto-upgrade
```

The joining machine automatically receives the mesh CA via gossip and installs it (macOS will prompt for your password once). Within 30 seconds, every daemon knows about every other daemon.

**5. Open the browser**

Navigate to any daemon's URL (e.g. `https://100.x.x.x:9111`). Click **+ daemon** in the command bar to add a second daemon's URL — or just wait for gossip to auto-populate the rest.

---

## CLI reference

```
hush [OPTIONS] [COMMAND]

Commands:
  upgrade   Pull a newer binary from a peer (manual trigger)
  trust     Manage the local CA used for TLS certificates

Options:
  -p, --port <PORT>              Port to listen on [default: 9111]
      --bind <ADDR>              Bind address [default: 0.0.0.0]
      --state-file <PATH>        State file [default: ~/.hush/state.json]
      --machine-name <NAME>      Label shown in the UI (default: hostname)
      --advertise-url <URL>      WebSocket URL peers should dial to reach this daemon
                                 Required for peer discovery (e.g. wss://host:9111/ws)
      --join <URL>               Seed peer URL on startup (repeatable)
      --auto-upgrade             Automatically push this binary to older peers
      --tls-dir <PATH>           Directory for TLS CA and leaf cert (default: ~/.hush/)
  -h, --help                     Print help
```

---

## Testing two daemons on one machine

```sh
# Terminal 1
hush --port 9111 --machine-name laptop \
  --advertise-url wss://localhost:9111/ws

# Terminal 2
hush --port 9112 --machine-name studio \
  --state-file ~/.hush/state-studio.json \
  --advertise-url wss://localhost:9112/ws \
  --join wss://localhost:9111/ws
```

Add `wss://localhost:9111/ws` in the browser UI — `studio` appears automatically.

---

## Auto-start on macOS (launchd)

Create `~/Library/LaunchAgents/com.hush.daemon.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.hush.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/YOUR_USERNAME/.local/bin/hush</string>
    <string>--advertise-url</string>
    <string>wss://YOUR_TAILSCALE_IP:9111/ws</string>
    <string>--machine-name</string>
    <string>YOUR_MACHINE_NAME</string>
    <string>--auto-upgrade</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/tmp/hush.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/hush.log</string>
</dict>
</plist>
```

```sh
launchctl load ~/Library/LaunchAgents/com.hush.daemon.plist
```

`KeepAlive: true` is required for P2P upgrades — after replacing its binary, `hush` exits and launchd restarts it with the new version.

---

## Upgrades

Upgrades flow through the gossip mesh — no GitHub access required on any machine except the one that builds the new binary.

1. Build the new binary on one machine:
   ```sh
   cd hush && make install
   ```

2. Restart that daemon with `--auto-upgrade`. Within one gossip round (~30 seconds), it streams the new binary to each older peer over the existing TLS WebSocket. Each peer replaces its binary and restarts automatically.

---

## How it works

```
Browser (any device)
  └── WebSocket per daemon ──► hush (machine A)  ◄── hush-hook shim
                          └──► hush (machine B)  ◄── hush-hook shim
```

- Each `hush` daemon owns its pty sessions, project registry, and state file.
- The browser namespaces IDs as `machineId:worktreeId` so projects from different machines never collide.
- Daemons gossip peer lists every 30 seconds — adding one daemon seeds the whole mesh.
- `hush-hook` is a shim invoked by Claude Code's hook system on lifecycle events (`SessionStart`, `Stop`, `Notification`, etc.). It writes structured JSON to a Unix socket so the daemon tracks status without parsing terminal output.
- Pty sessions survive browser disconnects. Reconnect anytime; scrollback replays automatically.
- P2P upgrades stream the binary over the same TLS WebSocket used for pty data.
- TLS certificates are automatically distributed via gossip — no manual `scp` or certificate management needed.

---

## Development

```sh
# Start the daemon (debug build)
cd daemon && cargo run

# Start the UI dev server (hot reload)
cd ui && npm run dev
```

The UI dev server runs on `http://localhost:5173` and connects to the daemon's WebSocket at `wss://localhost:9111/ws`.

**Optional: AI-powered command bar.** The command bar uses regex parsing by default. To enable natural language intent classification (downloads a ~300MB model on first load):

```sh
VITE_ENABLE_AI_INTENT=true npm run dev
```

Run `make hooks` once after cloning to install the pre-commit build check.

---

## License

MIT
