# Changelog

All notable changes to this project will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/). Versions follow [Semantic Versioning](https://semver.org/).

---

## [0.13.0] — 2026-04-16

### Added
- `hush invite` subcommand — generates a short-lived join token (`hush-join-XXXX-XXXX`) for enrolling a new machine into the mesh
- `--join-token` flag — new machines POST to `/join` on an existing peer, receive a CA-signed leaf cert, and start securely
- `/join` HTTP endpoint — validates join token and issues a leaf cert signed by the mesh CA
- `/config/local` endpoint — returns auth token only to loopback (`127.0.0.1`/`::1`) clients; `/config` no longer leaks the token to arbitrary HTTP clients
- mTLS for peer connections — outbound peer dials present the local leaf cert as TLS client identity; server verifies against mesh CA via `WebPkiClientVerifier`
- Transfer integrity — SHA-256 of streamed bytes (working dir + history) is signed with the CA key and verified by the destination before applying
- Separate `/peer` WebSocket endpoint for daemon-to-daemon traffic; browser clients remain on `/ws` with token auth

### Changed
- CA private key is no longer broadcast over the wire — gossip only distributes the public CA cert; the key stays on the CA-origin machine
- Unsigned peer upgrades are now rejected (previously accepted with a warning)
- Peer daemons dial `/peer` instead of `/ws`

### Security
- Eliminated CA private key exfiltration via gossip
- Auth token no longer served to unauthenticated remote clients
- Transfer payloads are signed and verified end-to-end

## [0.12.0] — 2025-04-15

### Added
- Restart Claude Code session button in terminal panel header
- Improved demo GIF with visible cursor, Claude Code output, shell footers, and multi-shell stacking

### Fixed
- CODE_OF_CONDUCT.md placeholder contact method filled in
- HTML title changed from "ui" to "Hush"
- Disconnected screen path corrected to `cd hush/daemon`

## [0.11.0] — 2025-04-14

### Added
- Multi-shell terminals — N independent shell ptys per worktree with stacked footer UI
- Ambient terminal awareness — last pty line displayed on dot grid, shell alive indicator
- Demo automation pipeline (`make demo`: Playwright + ffmpeg GIF recording)

### Changed
- AI intent model (Qwen2.5-0.5B) now optional, gated behind `VITE_ENABLE_AI_INTENT=true`
- Browser is lightweight by default (regex-only command bar, no WASM fetch)

### Fixed
- Auto-tidy dropped on panel z-index raise (store spread bug)
- ANSI strip regex updated to handle private-mode sequences (ESC[?25h)

### Added (open-source readiness)
- LICENSE (MIT), CONTRIBUTING.md, SECURITY.md, CODE_OF_CONDUCT.md
- CHANGELOG.md, release checklist, GitHub issue/PR templates, CODEOWNERS
- CI pipeline (check, build, ui, security audit)

## [0.10.0] — 2025-04-08

### Added
- Auto-tidy canvas — grid layout auto-arranges when projects are added/removed
- Restore Shift+Enter kitty keyboard sequence for Claude Code multi-line input

## [0.9.4] — 2025-04-06

### Fixed
- P2P upgrade fallback for system-directory installs (e.g. `/usr/local/bin/`)
- UX additions for upgrade flow

## [0.9.1] — 2025-04-05

### Fixed
- Pty session resume bug fixes after transfer
- Guard `--continue` behind history-dir existence check
- Use `--continue` for normal pty spawns, `--resume` only after transfer

## [0.9.0] — 2025-04-04

### Added
- Live worktree transfer between daemons — move a worktree from one machine to another
- P2P daemon upgrades via gossip — build once, propagate to the whole mesh

### Changed
- Switch intent model from Gemma 4 to Qwen2.5-0.5B for faster classification

## [0.8.0] — 2025-03-30

### Added
- Daemon portrait panel — "Portrait of the machine" detail view
- Gemma 4 in-browser natural language intent classification for command bar

### Fixed
- Daemon panel z-index and full-screen backdrop blur

## [0.7.3] — 2025-03-28

### Fixed
- GitHub release download compatibility (`--clobber` flag removed)

## [0.7.2] — 2025-03-27

### Added
- Memory pressure monitor with UI banner (warns at <25%, critical at <10%)

### Fixed
- Shift+Enter stale closure in TerminalPane

## [0.7.1] — 2025-03-26

### Fixed
- Startup panic from missing rustls crypto provider
- Added daemon smoke test to pre-commit hook

## [0.7.0] — 2025-03-25

### Changed
- Rewrite `hush upgrade` to shell out to `gh` CLI for private repo support

### Added
- `dangerously-skip-permissions` mode for worktree sessions

## [0.5.0] — 2025-03-20

### Added
- Local CA for zero-click browser TLS trust (`hush trust`)
- Self-signed CA generation, leaf cert signing, OS trust store integration
