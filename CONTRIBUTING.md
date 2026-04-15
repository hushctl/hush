# Contributing to Hush

Thanks for your interest in contributing. This guide gets you from zero to an open PR in under 10 minutes.

Please read our [Code of Conduct](CODE_OF_CONDUCT.md) before participating.

---

## Quick setup

```sh
git clone https://github.com/nicholasgasior/hush
cd hush

# Build everything
make install

# Run the daemon (debug build, faster iteration)
cd daemon && cargo run &

# Run the UI dev server (hot reload)
cd ui && npm run dev
```

Prerequisites: Rust (via [rustup](https://rustup.rs/)), Node.js 18+, [Claude Code CLI](https://claude.ai/code).

---

## Making changes

1. **Fork and branch.** Create a feature branch from `main`:
   ```sh
   git checkout -b my-feature
   ```

2. **Make your changes.** Read `CLAUDE.md` first — it documents core invariants that must not be violated without explicit discussion.

3. **Check your work:**
   ```sh
   make check                      # cargo check + tsc
   cd daemon && cargo clippy        # lint for common mistakes
   cd tests && node run_tests.mjs   # integration tests
   ```

4. **Open a PR.** Fill out the PR template — especially the security checklist.

---

## What makes a good PR

- **Small and focused.** One logical change per PR. A bug fix and a refactor are two PRs.
- **Descriptive title.** "Fix gossip crash when peer URL is empty" not "Fix bug".
- **Explain why.** The diff shows *what* changed; the description explains *why*.
- **Tests included.** If you're adding behavior, add a test. If you're fixing a bug, add a test that would have caught it.

---

## Code review process

Every PR goes through review before merging:

1. **CI must pass.** `cargo check`, `cargo clippy`, `cargo test`, and TypeScript checks run automatically. Broken CI = no review.

2. **Security review.** Every PR is reviewed for security implications. The PR template includes a security checklist — fill it out honestly. Pay special attention to:
   - Network-facing code (WebSocket handlers, gossip protocol, TLS)
   - File I/O from external input (path traversal, symlink attacks)
   - Binary data handling (base64 decode, tar extraction, peer upgrades)
   - Anything that touches `~/.hush/tls/` (certificate material)

3. **Maintainer review.** A maintainer reviews for correctness, complexity, and consistency with existing patterns. The standard is [Google's eng-practices](https://google.github.io/eng-practices/review/reviewer/standard.html): "Does this PR improve the overall health of the codebase?" Not "is it perfect."

4. **Merge.** Squash-merge into `main` once approved.

Expect a response within 3 days. If your PR is stale for longer, ping in the comments.

---

## Security

If you find a security vulnerability, **do not open a public issue.** Email the maintainer directly or use GitHub's private vulnerability reporting. See [SECURITY.md](SECURITY.md) for details.

---

## Core invariants

These are documented in `CLAUDE.md` and must not be violated without discussion:

- **Terminal is the chat.** No custom chat renderer — xterm.js running a real `claude` pty is the conversation surface.
- **Status comes from hooks, not pty parsing.** The pty stream is opaque.
- **Command bar = workspace intent only.** It does not relay text to any worktree.
- **All sessions are daemon-spawned.** No hybrid model.
- **Local-first, no external database.**
- **Visual language: flat, square corners, font-weight 400, no gradients, no shadows.**

---

## Project structure

```
daemon/   — Rust daemon (axum + tokio): pty manager, hook listener, state, TLS, gossip
ui/       — React + xterm.js browser app
scripts/  — build helpers
tests/    — integration tests
Makefile  — build, hooks, release targets
```

---

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (MIT).
