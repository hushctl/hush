# Hush

A browser-based command center for Claude Code. Run Claude Code sessions on one or more machines and control all of them from a single browser tab.

---

## Install

Releases are stored in [GitHub Releases](../../releases). The repo is private — use the `gh` CLI to download (run `gh auth login` once per machine).

**macOS (Apple Silicon)**
```sh
gh release download --repo kushalhalder/hush --pattern "hush-macos-arm64.tar.gz" \
  && tar -xzf hush-macos-arm64.tar.gz \
  && sudo cp hush-*/hush hush-*/hush-hook /usr/local/bin/
```

**macOS (Intel)**
```sh
gh release download --repo kushalhalder/hush --pattern "hush-macos-x86_64.tar.gz" \
  && tar -xzf hush-macos-x86_64.tar.gz \
  && sudo cp hush-*/hush hush-*/hush-hook /usr/local/bin/
```

**Linux (x86_64)**
```sh
gh release download --repo kushalhalder/hush --pattern "hush-linux-x86_64.tar.gz" \
  && tar -xzf hush-linux-x86_64.tar.gz \
  && sudo cp hush-*/hush hush-*/hush-hook /usr/local/bin/
```

**Linux (ARM64)**
```sh
gh release download --repo kushalhalder/hush --pattern "hush-linux-arm64.tar.gz" \
  && tar -xzf hush-linux-arm64.tar.gz \
  && sudo cp hush-*/hush hush-*/hush-hook /usr/local/bin/
```

To download a specific version, add `--tag v0.1.0`. Without `--tag`, the latest release is used.

Each archive contains `hush`, `hush-hook`, and `README.md`.

---

## Prerequisites

- [Claude Code CLI](https://claude.ai/code) (`claude` must be on your PATH)

---

## Build from source

```sh
git clone https://github.com/your-org/hush
cd hush

# Daemon
cd daemon
cargo build --release
# Binaries: target/release/hush  target/release/hush-hook

# UI
cd ../ui
npm install
npm run build
# Output: dist/
```

---

## Single machine (local)

**1. Start the daemon**

```sh
./hush
```

Listens on `0.0.0.0:9111` by default. State is persisted to `~/.hush/state.json`.

**2. Open the UI**

Serve `ui/` from any static server, or just open `ui/index.html` directly. The browser connects to `ws://localhost:9111/ws`.

> For development: `cd ui && npm run dev` — opens at http://localhost:5173

**3. Add a project**

Click **+ project** in the command bar, enter the path to a Git repo, then enter a branch name (e.g. `main`). A Claude Code session starts — click the dot on the grid to open a terminal pane.

---

## Multiple machines over Tailscale

Each machine runs its own `hush` daemon. The browser connects to all of them and merges everything into one grid. Daemons gossip peer lists to each other, so adding one daemon URL in the UI is enough to discover the rest.

### On each machine

**1. Install Tailscale**

```sh
# macOS
brew install tailscale && sudo tailscaled & && tailscale up
```

Note your Tailscale IP (`tailscale ip -4`) or MagicDNS hostname.

**2. Start the daemon**

```sh
./hush \
  --advertise-url ws://$(tailscale ip -4):9111/ws \
  --machine-name my-laptop
```

**3. Join an existing mesh (second machine onward)**

```sh
./hush \
  --advertise-url ws://$(tailscale ip -4):9111/ws \
  --machine-name studio \
  --join ws://100.x.x.x:9111/ws
```

Within ~30 seconds every daemon knows about every other daemon.

### In the browser

Click **+ daemon** in the command bar and enter any one daemon's WebSocket URL. The rest of the mesh auto-populates from gossiped peer lists within ~60 seconds.

---

## CLI reference

```
hush [OPTIONS]

Options:
  -p, --port <PORT>              Port to listen on [default: 9111]
      --bind <ADDR>              Bind address [default: 0.0.0.0]
      --state-file <PATH>        State file [default: ~/.hush/state.json]
      --machine-name <NAME>      Label shown in the UI (default: hostname)
      --advertise-url <URL>      WebSocket URL peers should dial to reach this daemon
                                 Required for peer discovery (e.g. ws://host:9111/ws)
      --join <URL>               Seed peer URL on startup (repeatable)
  -h, --help                     Print help
```

---

## Running two daemons on one machine (testing)

```sh
# Terminal 1
./hush --port 9111 --machine-name laptop \
  --advertise-url ws://localhost:9111/ws

# Terminal 2
./hush --port 9112 --machine-name studio \
  --state-file ~/.hush/state-studio.json \
  --advertise-url ws://localhost:9112/ws \
  --join ws://localhost:9111/ws
```

Add `ws://localhost:9111/ws` in the UI — `studio` appears automatically.

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
    <string>/path/to/hush</string>
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
  <string>/tmp/hush.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/hush.log</string>
</dict>
</plist>
```

```sh
launchctl load ~/Library/LaunchAgents/com.hush.daemon.plist
```

---

## Releasing

Tag a commit to trigger a GitHub Actions build across all four platforms:

```sh
git tag v0.1.0
git push origin v0.1.0
```

Binaries and a `checksums.txt` are attached to the GitHub Release automatically.

---

## How it works

```
Browser (any device)
  └── WebSocket per daemon ──► hush (machine A)  ◄── hush-hook shim
                          └──► hush (machine B)  ◄── hush-hook shim
```

- Each `hush` daemon owns its pty sessions, project registry, and state file.
- The browser namespaces IDs as `machineId:worktreeId` so dots from different machines never collide.
- Daemons gossip peer lists every 30 seconds — adding one daemon seeds the whole mesh.
- `hush-hook` is a shim invoked by Claude Code's hook system on lifecycle events (`SessionStart`, `Stop`, `Notification`, etc.). It writes structured JSON to a Unix socket so the daemon tracks status without parsing terminal output.
- Pty sessions survive browser disconnects. Reconnect anytime; scrollback replays automatically.
