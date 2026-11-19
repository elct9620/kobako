#!/bin/bash
# pre-push: run the tests only Linux compiles — the Mount differential probe
# against wasmtime-wasi — in a Linux container, as CI runs on ubuntu, since
# a macOS gate never builds them. `rake hooks:install` registers it.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"

# Nothing to run in where docker is absent or stopped.
docker info >/dev/null 2>&1 || {
  echo "[linux-tests] docker unavailable — skipping the Linux-only tests" >&2
  exit 0
}

log="${TMPDIR:-/tmp}"
log="${log%/}/kobako-linux-tests.log"

# Named volumes keep the target and the registry between pushes, so a
# later run compiles only what changed.
toolchain=$(sed -nE 's/^channel *= *"([^"]+)".*/\1/p' "$root/rust-toolchain.toml")
echo "[linux-tests] running the Linux-only tests in rust:${toolchain}-slim" >&2
if ! docker run --rm -v "$root":/src:ro -v kobako-linux-target:/target \
  -v kobako-linux-cargo:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/target \
  -w /src/crates "rust:${toolchain}-slim" \
  cargo test -p kobako-wasmtime --lib differential >"$log" 2>&1; then
  grep -E 'FAILED|panicked|left:|right:|error' "$log" | head -n 40 >&2
  echo "[linux-tests] failed; full output: $log" >&2
  exit 1
fi
