# Hush

One screen for all your Claude Code sessions, across all your machines.

<!-- Re-record with: make demo -->
![Hush demo](docs/demo.gif)

Run Claude Code on your laptop, your desktop, your cloud box — and control all of them from a single browser tab. Hush gives you a spatial canvas where every session on every machine is visible at a glance.

- **Multi-session orchestration** — Run N Claude Code sessions in parallel. Status dots (green/amber/red) tell you where your attention is needed.
- **Multi-machine mesh** — Each machine runs a daemon. Daemons discover each other via gossip. Open one browser tab, see everything.
- **Sessions survive disconnects** — Close your laptop, Claude keeps working. Reconnect later, scrollback replays automatically.
- **P2P upgrades** — Build once, propagate to the whole mesh. No CI, no GitHub access needed on receiving machines.

> **Private network only.** Hush is designed for use over Tailscale, a VPN, or a trusted LAN. Do not expose the daemon port to the public internet — there is no authentication on the daemon-to-daemon gossip channel.

---

## Table of contents

- [Installation](#installation)
  - [Homebrew (macOS)](#homebrew-macos)
  - [Download pre-built binary](#download-pre-built-binary)
  - [Build from source](#build-from-source)
- [Getting started](#getting-started)
- [Multiple machines](#multiple-machines)
- [Features](#features)
  - [Queued tasks](#queued-tasks)
  - [mDNS peer discovery](#mdns-peer-discovery)
  - [P2P upgrades](#p2p-upgrades)
  - [Responsive project cards](#responsive-project-cards)
- [CLI reference](#cli-reference)
- [Auto-start on macOS (launchd)](#auto-start-on-macos-launchd)
- [Testing two daemons on one machine](#testing-two-daemons-on-one-machine)
- [How it works](#how-it-works)
- [Development](#development)
- [Troubleshooting](#troubleshooting)
- [License](#license)

---

## Installation

### Homebrew (macOS)

```sh
brew install hushctl/hush/hush
```

Jump to [Getting started](#getting-started).

---

### Download pre-built binary

Requires the [GitHub CLI](https://cli.github.com/) (`gh`).

**Apple Silicon (M1/M2/M3):**

```sh
gh release download --repo hushctl/hush --pattern 'hush-darwin-aarch64.tar.gz'
tar xzf hush-darwin-aarch64.tar.gz
mkdir -p ~/.local/bin ~/.hush/ui
mv hush-darwin-aarch64/hush hush-darwin-aarch64/hush-hook ~/.local/bin/
cp -r hush-darwin-aarch64/ui/* ~/.hush/ui/
rm -rf hush-darwin-aarch64 hush-darwin-aarch64.tar.gz
```

**Intel Mac:**

```sh
gh release download --repo hushctl/hush --pattern 'hush-darwin-x86_64.tar.gz'
tar xzf hush-darwin-x86_64.tar.gz
mkdir -p ~/.local/bin ~/.hush/ui
mv hush-darwin-x86_64/hush hush-darwin-x86_64/hush-hook ~/.local/bin/
cp -r hush-darwin-x86_64/ui/* ~/.hush/ui/
rm -rf hush-darwin-x86_64 hush-darwin-x86_64.tar.gz
```

Add `~/.local/bin` to your PATH if it isn't already:

```sh
# Add to ~/.zshrc or ~/.bashrc
export PATH="$HOME/.local/bin:$PATH"
```

Jump to [Getting started](#getting-started).

---

### Build from source

**Prerequisites:**

- [Rust](https://rustup.rs/)
- [Node.js](https://nodejs.org/) 18+
- [Claude Code CLI](https://claude.ai/code) (`claude` must be on your PATH)

**Linux only:** install OpenSSL dev headers before building:

```sh
# Debian / Ubuntu
sudo apt-get install pkg-config libssl-dev

# Fedora / RHEL
sudo dnf install pkg-config openssl-devel
```

```sh
git clone https://github.com/hushctl/hush
cd hush
make install
```

This builds the daemon + UI and installs to `~/.local/bin/` and `~/.hush/ui/`.

Add `~/.local/bin` to your PATH if it isn't already:

```sh
# Add to ~/.zshrc or ~/.bashrc
export PATH="$HOME/.local/bin:$PATH"
```

---

## Getting started

```sh
hush
```

On first run, Hush:

1. Generates a TLS certificate authority for the mesh.
2. Encrypts the CA private key — you will be prompted to set a passphrase. Press Enter to use an empty passphrase (still encrypts the file; you won't be prompted again on restart if you set `HUSH_CA_PASSPHRASE=""`).
3. Installs the CA into your OS trust store so browsers automatically accept the daemon's certificate.
   - **macOS:** prompts for your login keychain password once.
   - **Linux:** run `hush trust` then follow the printed instructions (e.g. `sudo update-ca-certificates` on Debian/Ubuntu, `sudo trust anchor` on Fedora). Restart your browser after.

> **Non-interactive / launchd / systemd:** set `HUSH_CA_PASSPHRASE` before starting so the daemon never prompts. See [Auto-start on macOS](#auto-start-on-macos-launchd).

After the CA is trusted, open **https://localhost:9111** in your browser.

Click **+ project** in the command bar, enter the path to a Git repo, then enter a branch name. A Claude Code session starts — click the dot on the grid to open a terminal pane.

---

## Multiple machines

Each machine runs its own `hush` daemon. The browser connects to all of them and merges everything into one grid.

### 1. Install Tailscale on each machine

```sh
# macOS
brew install tailscale && sudo tailscaled & && tailscale up
```

### 2. Install Hush on each machine

See [Installation](#installation) above. Homebrew and the pre-built binary are the fastest paths on remote machines.

### 3. Start the first (CA) machine

```sh
hush \
  --bind 0.0.0.0 \
  --advertise-url wss://$(tailscale ip -4):9111/ws \
  --machine-name laptop \
  --auto-upgrade
```

`--bind 0.0.0.0` is required so remote peers can reach this daemon over Tailscale. The default (`127.0.0.1`) is intentionally localhost-only for single-machine setups.

### 4. Enroll additional machines

On the **first machine**, generate a short-lived join token:

```sh
hush invite
# prints: hush-join-XXXX-XXXX  (expires in 10 minutes)
```

On **each additional machine:**

```sh
hush \
  --bind 0.0.0.0 \
  --advertise-url wss://$(tailscale ip -4):9111/ws \
  --machine-name studio \
  --join wss://<first-machine-tailscale-ip>:9111/peer \
  --join-token hush-join-XXXX-XXXX \
  --auto-upgrade
```

The joining machine receives a signed TLS leaf cert from the CA machine, installs the mesh CA into its OS trust store (macOS prompts once), and starts. Within 30 seconds, every daemon knows about every other daemon via gossip.

> **After the first join:** subsequent restarts of the enrolled machine do not need `--join` or `--join-token` — the cert is already on disk.

### 5. Open the browser

Navigate to **https://localhost:9111** (or any daemon's URL). Click **+ daemon** in the command bar and enter the remote daemon's URL (e.g. `https://<tailscale-ip>:9111`) — or wait for gossip to auto-populate it within 30 seconds.

---

## Features

### Queued tasks

When a worktree is idle, queue up prompts that run sequentially. Each prompt dispatches automatically when the previous one finishes.

**From the UI:** click **+ task** in the worktree card when it is idle.

**Via WebSocket:**

```sh
npm install -g wscat
wscat -c "wss://localhost:9111/ws" --no-check
# Then send:
{"type":"queue_task","worktree_id":"<wt_id>","prompt":"add unit tests for auth module"}
```

The card shows queue depth as a badge. When Claude finishes, the next prompt is injected automatically after a 500 ms settle delay.

---

### mDNS peer discovery

On a local network, daemons find each other automatically — no `--join` needed for discovery. Hush advertises `_hush._tcp.local.` and merges discovered peers into the gossip mesh.

mDNS is enabled by default. To disable (e.g. if multicast is restricted on your network):

```sh
hush --no-mdns
```

> mDNS handles LAN discovery only. Cross-subnet and remote peers still require `--join` + `--join-token` for enrollment (mTLS cert issuance).

---

### P2P upgrades

Upgrades flow through the gossip mesh — no GitHub access needed on receiving machines.

1. Build and install the new binary on one machine:
   ```sh
   cd hush && make install
   ```
2. Restart that daemon with `--auto-upgrade`. Within one gossip round (~30 seconds), it streams the new binary to each older peer over TLS. Each peer replaces its binary and restarts automatically.

`KeepAlive: true` in the launchd plist ensures launchd restarts the daemon after the self-upgrade exit.

---

### Responsive project cards

Project cards render in three sizes based on available width:

| Variant | When used | Shows |
|---|---|---|
| Full | Half-width or wider | All details, action buttons, queued task list |
| Quarter | Quarter-width column | Name + dot + status pill + breadcrumb + queue badge |
| Minimal | Sidebar list | Name + dot only |

---

## CLI reference

```
hush [OPTIONS] [COMMAND]

Commands:
  invite    Generate a join token for enrolling a new machine into the mesh
  upgrade   Pull a newer binary from a peer (manual trigger)
  trust     Manage the local CA used for TLS certificates

Options:
  -p, --port <PORT>              Port to listen on [default: 9111]
      --bind <ADDR>              Bind address [default: 127.0.0.1]
                                 Use 0.0.0.0 for multi-machine / Tailscale access
      --state-file <PATH>        State file [default: ~/.hush/state.json]
      --machine-name <NAME>      Label shown in the UI (default: hostname)
      --advertise-url <URL>      WebSocket URL peers should dial to reach this daemon
                                 Required for peer discovery (e.g. wss://host:9111/ws)
      --join <URL>               Peer URL to enroll from (use the /peer endpoint)
                                 e.g. wss://host:9111/peer
      --join-token <TOKEN>       Join token from `hush invite` on the CA machine
      --auto-upgrade             Automatically push this binary to older peers
      --no-mdns                  Disable mDNS peer discovery on LAN
      --tls-dir <PATH>           Directory for TLS CA and leaf cert [default: ~/.hush/]
  -h, --help                     Print help

Environment variables:
  HUSH_CA_PASSPHRASE             Passphrase for the encrypted CA private key.
                                 Required when stdin is not a TTY (launchd, systemd, CI).
                                 Set to an empty string if you chose no passphrase on first run.
```

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
  <key>EnvironmentVariables</key>
  <dict>
    <key>HUSH_CA_PASSPHRASE</key>
    <string>YOUR_PASSPHRASE</string>
  </dict>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/YOUR_USERNAME/.local/bin/hush</string>
    <string>--bind</string>
    <string>0.0.0.0</string>
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

## Testing two daemons on one machine

```sh
# Terminal 1 — CA machine
HUSH_CA_PASSPHRASE=test hush \
  --port 9111 \
  --machine-name laptop \
  --advertise-url wss://localhost:9111/ws

# Generate join token
hush invite
# → hush-join-XXXX-XXXX

# Terminal 2 — joining machine
HUSH_CA_PASSPHRASE=test hush \
  --port 9112 \
  --machine-name studio \
  --state-file ~/.hush/state-studio.json \
  --tls-dir ~/.hush/ \
  --advertise-url wss://localhost:9112/ws \
  --join wss://localhost:9111/peer \
  --join-token hush-join-XXXX-XXXX
```

`--tls-dir ~/.hush/` points both daemons at the same CA so they trust each other's leaf certs without a separate trust-install step.

Open **https://localhost:9111** — `studio` appears automatically within one gossip round.

---

## How it works

```
Browser (any device)
  └── WebSocket per daemon ──► hush (machine A)  ◄── hush-hook shim
                          └──► hush (machine B)  ◄── hush-hook shim
```

- Each `hush` daemon owns its pty sessions, project registry, and state file (`~/.hush/state.json`).
- The browser namespaces IDs as `machineId:worktreeId` so projects from different machines never collide.
- Daemons gossip peer lists every 30 seconds — adding one daemon seeds the whole mesh.
- `hush-hook` is a shim invoked by Claude Code's hook system on lifecycle events (`SessionStart`, `Stop`, `Notification`, etc.). It writes structured JSON to a Unix socket so the daemon tracks status without parsing terminal output.
- Pty sessions survive browser disconnects. Reconnect anytime; scrollback replays automatically.
- P2P upgrades stream the binary over the same TLS WebSocket used for pty data. Upgrade tarballs are signed with the mesh CA key and verified on receipt.
- Each daemon has a leaf TLS cert signed by the mesh CA. `hush invite` issues a join token; the joining machine POSTs to `/join`, receives a signed cert, and joins the mesh. The CA private key never leaves the CA-origin machine.
- mDNS (`_hush._tcp.local.`) handles LAN peer discovery automatically. Enrollment (cert signing) still requires `hush invite`.

---

## Development

```sh
# Start the daemon (debug build, auto-reloads on save)
cd daemon && cargo run

# Start the UI dev server (hot reload)
cd ui && npm run dev
```

The UI dev server runs on `http://localhost:5173` and proxies the daemon WebSocket at `wss://localhost:9111/ws`.

**Optional: AI-powered command bar.** The command bar uses regex parsing by default. To enable natural language intent classification (downloads a ~300MB model on first load):

```sh
VITE_ENABLE_AI_INTENT=true npm run dev
```

Run `make hooks` once after cloning to install the pre-commit build check.

---

## Troubleshooting

See [docs/DEBUGGING.md](docs/DEBUGGING.md) for a full guide covering:

- Startup failures (UTF-8 errors, passphrase prompts blocking the server, empty passphrase behaviour)
- Multi-machine connectivity checklist
- macOS firewall issues
- SIGTERM / daemon lifecycle
- Release and CI failures

---

## License

MIT
