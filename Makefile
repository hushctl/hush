.PHONY: hooks check check-daemon check-ui

hooks:
	./scripts/install-hooks.sh

check: check-daemon check-ui

check-daemon:
	cd daemon && cargo check --all-targets

check-ui:
	cd ui && node_modules/.bin/tsc --noEmit
