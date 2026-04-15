## What does this PR do?

<!-- One or two sentences. Link to an issue if one exists. -->

## Why?

<!-- What problem does this solve? What motivated the change? -->

## How to test

<!-- Steps a reviewer can follow to verify the change works. -->

## Security checklist

- [ ] No secrets, credentials, or API keys in the diff
- [ ] No new `unsafe` blocks in Rust (or justified in comments if unavoidable)
- [ ] No new `dangerouslySetInnerHTML`, `eval`, or raw HTML injection in the UI
- [ ] Network-facing changes (WebSocket messages, TLS, gossip) have been reviewed for injection or spoofing risks
- [ ] File paths from external input are sanitized (no path traversal)
- [ ] Base64/binary input is validated before writing to disk

## Quality checklist

- [ ] `make check` passes (cargo check + tsc)
- [ ] Tests pass (`cd tests && node run_tests.mjs`)
- [ ] No new warnings in `cargo check` or `cargo clippy`
- [ ] Changes follow existing patterns in CLAUDE.md
- [ ] PR is focused — one logical change, not a bundle
