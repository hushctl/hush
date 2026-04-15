# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability in Hush, **do not open a public issue.**

Instead, use one of these methods:

1. **GitHub private vulnerability reporting** — go to the Security tab of this repository and click "Report a vulnerability."
2. **Email** — contact the maintainer directly.

Include:
- Description of the vulnerability
- Steps to reproduce
- Impact assessment (what an attacker could do)
- Suggested fix if you have one

You will receive a response within 48 hours acknowledging the report.

## Scope

Security-sensitive areas of Hush include:

- **TLS and certificate handling** (`daemon/src/tls.rs`, `daemon/src/trust.rs`) — CA generation, leaf cert signing, certificate distribution via gossip
- **Peer-to-peer communication** (`daemon/src/gossip.rs`, `daemon/src/peer_upgrade.rs`, `daemon/src/transfer.rs`) — gossip protocol, binary upgrades, worktree transfers
- **WebSocket message handling** (`daemon/src/ws.rs`) — all client messages, especially those that write to disk or execute processes
- **Pty management** (`daemon/src/pty.rs`) — process spawning, environment injection
- **File I/O from external input** — paste_image (base64 → disk), tar extraction (peer upgrades/transfers), state file parsing

## Current known limitations

- **No authentication between peers.** Hush trusts the network layer (Tailscale, VPN, or LAN). Do not expose Hush daemons to the public internet without a VPN or tunnel.
- **CA private key is shared across the mesh** via gossip. This is by design for zero-config setup on private networks. On untrusted networks, this would allow a MITM to sign certificates.
- **Daemon-to-daemon gossip skips TLS certificate verification** (`danger_accept_invalid_certs`). Peer authenticity comes from the network layer, not TLS.

These are acceptable trade-offs for private networks but would need to be addressed before any public-internet deployment.
