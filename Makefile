.PHONY: hooks check check-daemon check-ui build build-ui build-daemon install test test-daemon test-ui clean

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

# ── Test ─────────────────────────────────────────────────────────────────────

test: test-daemon test-ui test-acceptance

test-daemon:
	cd daemon && cargo test

test-ui:
	cd ui && npm test

test-acceptance:
	@echo "Building daemon (debug)..."
	cd daemon && cargo build 2>&1 | tail -3
	@echo "Running acceptance tests..."
	cd tests && node run_tests.mjs

# ── Clean ────────────────────────────────────────────────────────────────────

clean:
	cd daemon && cargo clean
	cd ui && rm -rf dist node_modules/.vite

