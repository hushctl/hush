# Hush

A browser-based command center for Claude Code. Run Claude Code sessions on one or more machines and control all of them from a single browser tab.

---

## Install

Build from source (requires Rust):

```sh
git clone https://github.com/kushalhalder/hush
cd hush/daemon
cargo build --release
```

Copy the binaries to a user-writable directory on your PATH. `~/.local/bin/` is recommended — it works on both macOS and Linux without requiring root:

```sh
mkdir -p ~/.local/bin
cp target/release/hush target/release/hush-hook ~/.local/bin/
```

### Add `~/.local/bin` to your PATH

**zsh** (`~/.zshrc`):
```sh
export PATH="$HOME/.local/bin:$PATH"
```

**bash** (`~/.bashrc` or `~/.bash_profile`):
```sh
export PATH="$HOME/.local/bin:$PATH"
```

After editing, reload your shell: `source ~/.zshrc` (or open a new terminal).

> **Why not `/usr/local/bin/`?**
> On Apple Silicon Macs, `/usr/local/bin/` is owned by root. Installing there requires `sudo` and blocks P2P auto-upgrades — `hush` can't replace its own binary when running as a normal user.

---

## Prerequisites

- [Claude Code CLI](https://claude.ai/code) (`claude` must be on your PATH)

---

## Single machine (local)

**1. Trust the local CA** (once per machine)

```sh
hush trust
```

This installs Hush's self-signed CA into your OS trust store so browsers automatically trust the daemon's TLS cert.

**2. Start the daemon**

```sh
hush
```

Listens on `0.0.0.0:9111` by default. State is persisted to `~/.hush/state.json`.

**3. Open the UI**

For development: `cd ui && npm run dev` — opens at http://localhost:5173

**4. Add a project**

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

**2. Trust the local CA on each machine**

```sh
hush trust
```

**3. Start the daemon**

```sh
hush \
  --advertise-url wss://$(tailscale ip -4):9111/ws \
  --machine-name my-laptop \
  --auto-upgrade
```

`--auto-upgrade` enables P2P binary distribution: when this daemon is newer than a peer, it automatically pushes its binary to that peer over the existing gossip connection. The peer restarts with the new version — no `gh` CLI or internet access required on the receiving machine.

**4. Join an existing mesh (second machine onward)**

```sh
hush \
  --advertise-url wss://$(tailscale ip -4):9111/ws \
  --machine-name studio \
  --join wss://100.x.x.x:9111/ws \
  --auto-upgrade
```

Within ~30 seconds every daemon knows about every other daemon.

### In the browser

Click **+ daemon** in the command bar and enter any one daemon's WebSocket URL. The rest of the mesh auto-populates from gossiped peer lists within ~60 seconds.

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

## Running two daemons on one machine (testing)

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

Add `wss://localhost:9111/ws` in the UI — `studio` appears automatically.

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

`KeepAlive: true` is required for P2P upgrades — after replacing its binary, `hush` calls `process::exit(0)` and launchd restarts it automatically with the new version.

> **Note:** The `ProgramArguments` path must be absolute. `~` is not expanded by launchd. Replace `YOUR_USERNAME` with the output of `whoami`.

---

## Upgrades

Upgrades flow through the gossip mesh — no GitHub access required on any machine except the one that builds the new binary.

**To upgrade the whole mesh:**

1. Build the new binary on one machine:
   ```sh
   cd hush/daemon && cargo build --release
   cp target/release/hush target/release/hush-hook ~/.local/bin/
   ```

2. Restart that daemon with `--auto-upgrade`:
   ```sh
   pkill hush
   hush --advertise-url wss://$(tailscale ip -4):9111/ws --auto-upgrade
   ```

3. Within one gossip round (~30 seconds), the daemon detects peers running an older version and streams the new binary to each of them over the existing TLS WebSocket. Each peer replaces its binary and restarts automatically.

**Binary install location and self-upgrade:**
- If `hush` is installed in a user-writable directory (e.g. `~/.local/bin/`), upgrades replace the binary in-place and rely on launchd `KeepAlive` to restart.
- If the binary directory is not writable (e.g. `/usr/local/bin/`), the upgrade falls back to installing in `~/.hush/bin/` and execs directly from there. The process continues running with the new version; launchd will restart from the old path on the next system boot. For reliable upgrades, install to `~/.local/bin/`.

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
- P2P upgrades stream the binary over the same TLS WebSocket used for pty data, requiring no external services.
