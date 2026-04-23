# Hush Debugging Guide

Hard-won lessons from running Hush in the wild. When something goes wrong, start here.

---

## Startup issues

### "stream did not contain valid UTF-8" on startup

**Cause:** The `ca.key` file is encrypted (HKEK binary format) but the binary being run is an old build that predates CA key encryption (pre-v0.14.0). It tries to parse the binary blob as UTF-8 PEM and fails.

**Fix:** Rebuild the binary.

```sh
cd daemon && cargo build --release
```

---

### "CA key is encrypted but HUSH_CA_PASSPHRASE is not set and stdin is not a TTY"

**Cause:** Running as a background process (launchd, systemd, tmux without TTY) without setting the passphrase env var.

**Fix:** Set `HUSH_CA_PASSPHRASE` in the environment before starting.

```sh
HUSH_CA_PASSPHRASE=<your-passphrase> hush ...
```

For launchd, add to the plist `EnvironmentVariables` dict. See README.

---

### Daemon starts (gossip logs appear) but port 9111 never opens

**Cause:** The main thread is blocked at the `Create passphrase for CA key:` prompt mid-startup. Background tokio tasks (gossip, mDNS) run fine but the HTTP server is spawned after TLS loads, so it never starts.

**Symptom:** `curl https://localhost:9111/health` → connection refused. Gossip logs keep appearing.

**Fix:** Look at the terminal where hush is running — there will be an unanswered passphrase prompt. Type a passphrase and press Enter.

**Better fix:** Use `HUSH_CA_PASSPHRASE` so it never prompts:

```sh
HUSH_CA_PASSPHRASE=mypassphrase hush --bind 0.0.0.0 ...
```

---

### Empty passphrase (just pressing Enter)

This is valid — the CA key will be encrypted with an empty string. It means no passphrase is needed on restart but the key is still in HKEK binary format (not plaintext). Set `HUSH_CA_PASSPHRASE=""` or just press Enter on subsequent prompts.

---

## Multi-machine issues

### Remote machine not appearing in UI

Work through this checklist in order:

**1. Is the remote daemon running?**
```sh
pgrep -la hush
```

**2. Is port 9111 reachable from the local machine?**
```sh
curl -sk --max-time 5 https://<remote-ip>:9111/health
```
- Exit code 0 + "ok" → daemon is up and reachable
- Exit code 7 → connection refused (daemon not running, or passphrase prompt blocking startup)
- Exit code 28 → timeout (firewall or wrong IP)

**3. Is Tailscale up on both machines?**
```sh
ping -c 2 <remote-tailscale-ip>
```

**4. Was the remote machine enrolled with `hush invite`?**

mDNS handles LAN *discovery* but NOT enrollment. The remote machine needs a signed TLS cert from the CA machine. Without `--join` + `--join-token`, the remote daemon starts in isolation and gossip connections will fail TLS.

```sh
# On CA machine
hush invite
# → hush-join-XXXX-XXXX (valid 10 min)

# On remote machine
hush --bind 0.0.0.0 \
     --advertise-url wss://$(tailscale ip -4):9111/ws \
     --machine-name <name> \
     --join wss://<ca-machine-tailscale-ip>:9111/peer \
     --join-token hush-join-XXXX-XXXX
```

**5. Did the remote machine complete its CA key passphrase prompt?**

After joining, the remote machine migrates its received CA key to encrypted format and prompts for a passphrase. If this prompt is unanswered, the HTTP server never starts (see above).

**6. Is the remote daemon's URL added in the browser?**

The browser does NOT auto-connect to gossip peers (those are daemon-to-daemon Tailscale addresses). Use the command bar `+ daemon` and enter `https://<remote-tailscale-ip>:9111`.

---

### macOS firewall blocking inbound connections

If `curl https://<remote-ip>:9111/health` times out (exit 28) but ping works, macOS firewall may have denied `hush` from accepting inbound connections when it first prompted.

Check: System Settings → Network → Firewall → look for hush entry.

Fix via CLI:
```sh
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add /path/to/hush
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp /path/to/hush
```

---

### "gossip round complete: contacted [laptop, laptop]" — duplicate entries

**Cause:** The peer list has two entries for the same machine (one from `--join`, one from mDNS discovery). Cosmetic only — gossip deduplicates by machine_id on write, but the display shows both contact attempts.

Not a problem in practice.

---

## Daemon lifecycle

### SIGTERM not killing the daemon (pre-v0.14.0)

Old builds only handled SIGINT (Ctrl+C). `kill <pid>` sends SIGTERM which was ignored.

**Fix:** `kill -9 <pid>` on old builds. v0.14.0+ handles SIGTERM correctly.

---

### Checking what peers the daemon knows about

```sh
cat ~/.hush/state.json | python3 -m json.tool | grep -A 5 '"peers"'
```

Note: state.json is written periodically, not on every peer update. The in-memory state may be more current.

---

## Release / build issues

### Release workflow fails with 403 "Resource not accessible by integration"

**Cause:** The release workflow was missing `permissions: contents: write`. Fixed in v0.14.0.

---

### Pre-commit smoke test fails: "CA key is encrypted but HUSH_CA_PASSPHRASE is not set"

**Cause:** The smoke test started the daemon without a `--tls-dir` pointing to a temp directory, so it used the real `~/.hush/tls/ca.key` which is encrypted.

**Fix (already applied):** The pre-commit script passes `HUSH_CA_PASSPHRASE=smoke-test` and `--tls-dir $tmpdir/tls` so the smoke test always generates a fresh CA.

---

### cargo audit fails: RUSTSEC advisory

Run `cargo update -p <crate>` to bump to the patched version, then commit the updated `Cargo.lock`.

```sh
cd daemon && cargo update -p rustls-webpki
```
