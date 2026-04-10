#!/usr/bin/env bash
# Install the basis pre-commit hook into this repo's .git/hooks/.
# Idempotent: re-running just refreshes the symlink/copy.
#
# We copy rather than symlink so the hook keeps working if the
# scripts/ directory is moved or the symlink target disappears.

set -eu

REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOKS_DIR="${REPO_ROOT}/.git/hooks"
SOURCE="${REPO_ROOT}/scripts/pre-commit-basis.sh"
TARGET="${HOOKS_DIR}/pre-commit"

if [ ! -f "$SOURCE" ]; then
    echo "install-hooks: $SOURCE not found." >&2
    exit 1
fi

mkdir -p "$HOOKS_DIR"

if [ -f "$TARGET" ] && ! grep -q "pre-commit-basis" "$TARGET" 2>/dev/null; then
    BACKUP="${TARGET}.backup-$(date +%Y%m%d-%H%M%S)"
    echo "install-hooks: existing pre-commit hook found, backing up to $BACKUP"
    mv "$TARGET" "$BACKUP"
fi

cp "$SOURCE" "$TARGET"
chmod +x "$TARGET"
echo "install-hooks: installed pre-commit-basis hook at $TARGET"
