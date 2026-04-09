#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOK="$REPO_ROOT/.git/hooks/pre-commit"
ln -sf "$REPO_ROOT/scripts/pre-commit" "$HOOK"
echo "Installed pre-commit hook → $HOOK"
