.PHONY: hooks check check-daemon check-ui build build-ui build-daemon install demo

hooks:
	./scripts/install-hooks.sh

check: check-daemon check-ui

check-daemon:
	cd daemon && cargo check --all-targets

check-ui:
	cd ui && node_modules/.bin/tsc --noEmit

# ── Build ─────────────────────────────────────────────────────────────────────

build: build-ui build-daemon

build-ui:
	cd ui && npm run build

build-daemon:
	cd daemon && cargo build --release

# ── Install ───────────────────────────────────────────────────────────────────

install: build
	mkdir -p ~/.local/bin ~/.hush/ui
	cp daemon/target/release/hush daemon/target/release/hush-hook ~/.local/bin/
	cp -r ui/dist/* ~/.hush/ui/
	@echo ""
	@echo "Installed to ~/.local/bin/hush and ~/.hush/ui/"
	@echo "Make sure ~/.local/bin is on your PATH, then run: hush"

# ── Demo GIF ──────────────────────────────────────────────────────────────────

demo: build-daemon
	cd ui && npx playwright install chromium --with-deps 2>/dev/null || true
	node scripts/record-demo.mjs
