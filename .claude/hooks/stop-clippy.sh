#!/bin/bash
# Stop: clippy gate over every workspace — host ext, wasm sub-workspace,
# and the standalone kobako-baker — plus the wasm32-wasip1 cross check
# when the target toolchain and the Stage B archive are both present.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:?}"

clippy() {
  local label="$1"
  shift
  if ! cargo clippy "$@" -q -- -D warnings >&2; then
    echo "[clippy:$label] warnings found" >&2
    exit 2
  fi
}

clippy host --manifest-path "$root/Cargo.toml" --workspace --all-targets
clippy wasm --manifest-path "$root/wasm/Cargo.toml" --workspace --all-targets
clippy baker --manifest-path "$root/wasm/kobako-baker/Cargo.toml" --all-targets

if rustc --target wasm32-wasip1 --print sysroot >/dev/null 2>&1 \
  && [ -f "$root/vendor/mruby/build/wasi/lib/libmruby.a" ]; then
  MRUBY_LIB_DIR="$root/vendor/mruby/build/wasi/lib" \
    WASI_SDK_PATH="$root/vendor/wasi-sdk" \
    clippy wasm32 --target wasm32-wasip1 --manifest-path "$root/wasm/Cargo.toml" --workspace
fi
