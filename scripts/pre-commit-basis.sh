#!/usr/bin/env bash
# Git pre-commit hook that runs `basis check` and blocks the commit on
# violations. The whole point of basis being a compiler is that
# violations are errors, not warnings — so they cannot be allowed to
# accumulate silently between commits.
#
# Resolution order for the basis binary:
#   1. $BASIS_BIN if set
#   2. `basis` on $PATH
#   3. ./basis-cli/target/release/basis(.exe) relative to the repo root
#
# Resolution for the spec:
#   1. $BASIS_SPEC if set
#   2. ./basis.yaml in the repo root (default)
#
# Skip with: `git commit --no-verify` (do not make a habit of this).

set -eu

REPO_ROOT="$(git rev-parse --show-toplevel)"
SPEC="${BASIS_SPEC:-${REPO_ROOT}/basis.yaml}"

if [ ! -f "$SPEC" ]; then
    # No spec → nothing to govern. Don't break commits in repos that
    # haven't adopted basis yet.
    exit 0
fi

# Resolve the binary.
if [ -n "${BASIS_BIN:-}" ]; then
    BIN="$BASIS_BIN"
elif command -v basis >/dev/null 2>&1; then
    BIN="basis"
elif [ -x "${REPO_ROOT}/basis-cli/target/release/basis.exe" ]; then
    BIN="${REPO_ROOT}/basis-cli/target/release/basis.exe"
elif [ -x "${REPO_ROOT}/basis-cli/target/release/basis" ]; then
    BIN="${REPO_ROOT}/basis-cli/target/release/basis"
else
    echo "pre-commit-basis: cannot find 'basis' binary." >&2
    echo "  Set BASIS_BIN, install basis on PATH, or run \`cargo build --release -p basis-cli\`." >&2
    # Default-allow on missing tooling — better to ship a commit than
    # to brick the workflow on a setup hiccup. The cost of a missed
    # check is bounded by the next CI run; the cost of an unbreakable
    # workflow is unbounded.
    exit 0
fi

echo "pre-commit-basis: $BIN check --spec $SPEC"
if ! "$BIN" check --spec "$SPEC" "$REPO_ROOT"; then
    echo "" >&2
    echo "pre-commit-basis: basis check FAILED — commit blocked." >&2
    echo "  Fix the violations above, or commit with --no-verify if you" >&2
    echo "  understand exactly why the violation is acceptable." >&2
    exit 1
fi
