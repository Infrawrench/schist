#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
profile=web
if [[ "${1:-}" == --debug ]]; then profile=dev; fi
target=wasm32-unknown-unknown
out=dist/library/wasm
locked=$(python3 - <<'PY'
import tomllib
with open('Cargo.lock', 'rb') as f:
    print(next(p['version'] for p in tomllib.load(f)['package'] if p['name'] == 'wasm-bindgen'))
PY
)
command -v wasm-bindgen >/dev/null || { echo "Install wasm-bindgen-cli $locked" >&2; exit 1; }
[[ "$(wasm-bindgen --version)" == "wasm-bindgen $locked" ]] || {
  echo "wasm-bindgen-cli must match Cargo.lock ($locked)" >&2; exit 1;
}
rustup target list --installed | grep -qx "$target" || rustup target add "$target"
./tools/library-cargo.sh build --lib --target "$target" --profile "$profile"
artifact_profile=$profile
if [[ "$profile" == dev ]]; then artifact_profile=debug; fi
mkdir -p "$out"
wasm-bindgen --target web --out-name schist --out-dir "$out" \
  "${SCHIST_LIBRARY_TARGET_DIR:-target/library}/$target/$artifact_profile/schist.wasm"
mkdir -p dist/library/node
wasm-bindgen --target nodejs --out-name schist --out-dir dist/library/node \
  "${SCHIST_LIBRARY_TARGET_DIR:-target/library}/$target/$artifact_profile/schist.wasm"
printf '%s\n' '{"type":"commonjs"}' > dist/library/node/package.json
cp include/schist.h dist/library/
cp LICENSE web/fonts/LICENSE-IBMPlexSans.txt "$out/"
cp LICENSE web/fonts/LICENSE-IBMPlexSans.txt dist/library/node/
echo "Built $out/schist.js, schist.d.ts and schist_bg.wasm"
