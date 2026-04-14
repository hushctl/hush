# Draft bug report: MCP OAuth "Press Enter to continue" ignores keyboard input

Intended as a follow-up comment on:
- https://github.com/anthropics/claude-code/issues/42707 (primary)
- https://github.com/anthropics/claude-code/issues/45875 (duplicate family)

---

## Summary

After completing OAuth in the browser for a remote MCP server, the "Press Enter to continue" modal in Claude Code does not respond to Enter, Esc, or any other key. Ctrl+C is the only escape. Reproduces in iTerm2 and Terminal.app. Byte-level packet capture shows Enter *is* being delivered to Claude Code as `\r` (`0x0D`) — the process receives it and ignores it. The bug is not in the host terminal.

## Environment

- Claude Code: **2.1.107** (current latest as of 2026-04-14)
- Platform: macOS (darwin)
- MCP server: Kinobi (`claude.ai/kite`-style remote OAuth)
- Terminals reproduced in: iTerm2 3.6.8, Terminal.app

## Reproduction

1. `claude --dangerously-skip-permissions`
2. `/mcp` → select Kinobi → follow OAuth link to browser
3. Complete auth in browser
4. Return to terminal — modal shows "Press Enter to continue"
5. Press Enter — nothing happens
6. Press Esc — nothing happens
7. Only Ctrl+C escapes

## Evidence: bytes reach the process

I captured the raw bytes flowing from iTerm2 into Claude Code's controlling pty using a small pty-wrapper that forwards stdin to the child while logging every byte with a timestamp. The capture point sits between the terminal emulator and Claude Code.

### iTerm2 capture

Timestamps are unix-seconds-with-ms, relative-zeroed at `1776173545`. Annotations added for clarity.

```
+0.52    IN hex=1b503e7c695465726d3220332e362e381b5c   # DCS reply: "iTerm2 3.6.8" (answer to DA2)
+0.53    IN hex=1b5b3f36343b313b323b343b363b31373b... # CSI ?64;1;2;4;6;17;18;21;22;52c (DA)
+2.92    IN hex=1b5b4f                                 # focus-out  (user clicked browser)
+19.71   IN hex=1b5b49                                 # focus-in   (user returned after OAuth)
+22.90   IN hex=2f  6d  63  70                         # "/mcp"
+23.90   IN hex=0d                                     # Enter — opens the MCP menu (works)
+24.79,+24.97,+25.13  hex=1b5b42                       # Down arrow × 3
+25.48   IN hex=0d                                     # Enter — selects Kinobi, opens browser
+26.26   IN hex=0d                                     # (extra Enter while modal still loading)
+26.38   IN hex=1b5b4f                                 # focus-out (browser auth)
+27.45   IN hex=1b5b49                                 # focus-in  (user returned)
+35.56   IN hex=0d                                     # Enter on "Press Enter to continue" — IGNORED
+39.02   IN hex=0d                                     # Enter                                  — IGNORED
+39.51   IN hex=0d                                     # Enter                                  — IGNORED
+39.78   IN hex=0d                                     # Enter                                  — IGNORED
+40.26   IN hex=1b                                     # Esc                                    — IGNORED
+40.74   IN hex=1b                                     # Esc                                    — IGNORED
+41.32   IN hex=1b                                     # Esc                                    — IGNORED
+63.20   IN hex=1b5b32373b353b39397e                   # CSI 27;5;99~ (Ctrl+C via modifyOtherKeys)  — escape
```

The bytes Claude Code is receiving for "Enter" are `0x0D` — a plain carriage return, exactly what a POSIX terminal delivers for Enter in raw mode. Esc is `0x1B`. Focus-reporting (`CSI O` / `CSI I`) was correctly emitted around the OAuth redirect.

Native Claude Code installed via npm and run directly inside iTerm2 (no wrapper, no intermediate pty) reproduces the same hang. The OAuth-ignore-Enter behavior is independent of the host terminal.

## What this rules out

- ❌ Kitty keyboard protocol divergence — iTerm sent plain `\r`, same as every other terminal tested.
- ❌ `ICRNL` / raw-mode translation — bytes verified at the pty master fd.
- ❌ Terminal identity gating — `TERM_PROGRAM`, DA/DA2 responses all present in iTerm capture.
- ❌ Focus reporting — `CSI [O` / `CSI [I` were delivered correctly around OAuth.
- ❌ Third-party terminal bugs — reproduced in native iTerm2 and Terminal.app.

## Hypotheses

The process reads `\r` but the modal's input handler does not advance. Likely causes:

1. The Ink component rendering "Press Enter to continue" is not mounted / not the active `useInput` consumer when the OAuth callback returns. The browser redirect might complete after the modal has been rendered but before the input hook runs (race between the OAuth HTTP server's `onComplete` and the Ink focus stack).
2. The OAuth callback server's `process.stdin.resume()` or equivalent is not being called, leaving stdin paused at the kernel level — but this would also block Ctrl+C, which does work, so less likely.
3. State-machine bug: the modal's internal state is stuck in a "waiting for OAuth" phase and the "OAuth complete, await keypress" transition never fires, so keypresses match no handler.

## Reproducibility

100% — hits every time on this machine, and separately reported by multiple users on #42707 / #45875 / #45268 / #45220.

## Ask

Please prioritize — every MCP server using OAuth through the `claude.ai` proxy is currently unusable, and there is no keyboard-level workaround. The byte-level evidence here should make the fix localized to the Ink prompt's input hook around the OAuth-complete transition.

Happy to share raw capture logs if useful.
