# Hush UI

React + TypeScript + Vite browser app for the Hush daemon. Displays a spatial dot grid of Claude Code sessions, embedded terminals, and project management.

## Stack

| What | Tech |
|---|---|
| Framework | React 19 + TypeScript |
| Build | Vite |
| Styling | Tailwind CSS + shadcn/ui components |
| Terminal | xterm.js + xterm-addon-fit |
| State | Zustand (persist middleware for layout prefs) |
| Tests | Vitest (unit) + Playwright (E2E) |
| AI intent | Optional on-device Qwen2.5 via WebGPU (opt-in) |

## Development

```bash
npm install
npm run dev        # Vite dev server on :5173, proxies /ws to daemon on :9111
```

The daemon must be running (`hush` or `make build-daemon && ./target/debug/hush`).

## Build

```bash
npm run build      # TypeScript check + Vite production build → dist/
```

`make install` in the repo root builds both the daemon and UI, then installs everything to `~/.local/bin/` and `~/.hush/ui/`.

## Tests

```bash
npm test           # Vitest unit tests (store, protocol, ptyBus)
npx playwright test  # E2E tests (requires built daemon + UI)
```

## Key directories

```
src/
  components/
    Canvas/         — Free-form canvas with draggable PanelFrame windows
    DotGrid/        — Spatial Voronoi dot grid (main view)
    Layout/         — TopBar, CommandBar (intent parsing)
    ProjectCard/    — Status card with action buttons
    Terminal/       — xterm.js wrapper
  lib/
    intent.ts       — Command bar verb parser (regex + optional AI fallback)
    protocol.ts     — TypeScript types mirroring daemon's protocol.rs
    status.ts       — Status color/label mapping
    ptyBus.ts       — In-process pub/sub for pty data
  store/
    index.ts        — Zustand store + all WebSocket message handlers
    types.ts        — Store state shape
e2e/
  app.spec.ts       — Playwright end-to-end tests
```

## Design invariants

- No border-radius anywhere
- Font-weight 400 maximum
- No shadows, no gradients
- Terminal IS the chat — no custom chat renderer

See the root `CLAUDE.md` for the full architecture and invariant list.
