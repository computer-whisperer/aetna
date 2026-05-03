#!/usr/bin/env bash
#
# Build the aetna-web wasm bundle and (optionally) serve it locally.
#
# Mirrors whisper-agent's wasm-pack invocation. Output lands in
# crates/aetna-web/pkg/ — the same path the assets/index.html
# script tag imports from (`/pkg/aetna_web.js` when the static
# server's root is crates/aetna-web/).
#
# Usage:
#   tools/build_web.sh             # release build (default — dev wasm is too
#                                  # slow under our prepare/text path)
#   tools/build_web.sh --serve     # build + serve at http://127.0.0.1:8083/
#   tools/build_web.sh --dev       # unoptimized build (faster compile, slower run)
#
# Requires: wasm-pack (cargo install wasm-pack), and python3 for --serve.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SERVE=0
PROFILE=release
for arg in "$@"; do
    case "$arg" in
        --serve)   SERVE=1 ;;
        --release) PROFILE=release ;;
        --dev)     PROFILE=dev ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown arg: $arg (try --help)" >&2; exit 2 ;;
    esac
done

if [[ "$PROFILE" == "release" ]]; then
    PROFILE_FLAG="--release"
else
    PROFILE_FLAG="--dev"
fi

echo "==> building aetna-web (wasm, $PROFILE)"
cd "$REPO_ROOT"
# `--target web` produces the same module shape as whisper-agent uses:
# pkg/aetna_web.js exposes `default` (init) which returns a Promise; the
# index.html harness imports and calls it.
wasm-pack build crates/aetna-web --target web "$PROFILE_FLAG"

echo
echo "==> wasm bundle written to crates/aetna-web/pkg/"

if [[ "$SERVE" -eq 1 ]]; then
    echo "==> serving crates/aetna-web/ on http://127.0.0.1:8083/"
    echo "    open http://127.0.0.1:8083/assets/index.html"
    cd "$REPO_ROOT/crates/aetna-web"
    exec python3 -m http.server 8083
fi
