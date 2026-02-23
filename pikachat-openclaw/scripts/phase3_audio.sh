#!/usr/bin/env bash
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

REPO_ROOT="$(git rev-parse --show-toplevel)"

FRAMES="${FRAMES:-50}"

# Note: `pikachat-openclaw/` is not a standalone Cargo workspace in this monorepo.
# Always run pikachat via the repo-root Cargo workspace.
cargo run --manifest-path "${REPO_ROOT}/Cargo.toml" -p pikachat -- scenario audio-echo --frames "${FRAMES}"
