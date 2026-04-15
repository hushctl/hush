# Changelog

All notable changes to this project will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/). Versions follow [Semantic Versioning](https://semver.org/).

---

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
