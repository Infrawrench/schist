#!/usr/bin/env bash
# Isolated build directory/configuration: never change the desktop's globals by
# Cargo feature unification when both the desktop and library are built together.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${SCHIST_LIBRARY_TARGET_DIR:-target/library}"
export RUSTFLAGS="${RUSTFLAGS:-} --cfg schist_library --check-cfg=cfg(schist_library)"
if [[ " $* " == *" wasm32-unknown-unknown "* ]]; then
  export RUSTFLAGS="$RUSTFLAGS --cfg getrandom_backend=\"wasm_js\""
fi
# Shared libraries ship without the desktop's crash-reporting DWARF payload.
export CARGO_PROFILE_RELEASE_DEBUG="${CARGO_PROFILE_RELEASE_DEBUG:-0}"
command=$1
shift
exec "${CARGO:-cargo}" "$command" -p schist-library "$@"
