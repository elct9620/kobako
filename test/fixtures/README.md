# Test fixtures

Static binaries the test suite reads via `File.binread` / `Kobako::Sandbox.new(wasm_path:)`. **Do not regenerate automatically.** Each blob is intentionally frozen so future mruby / wasi-sdk / kobako changes that shift the bytecode layout, RITE header, or guest ABI surface as test failures here instead of slipping into production. If a fixture genuinely needs to track upstream (rare), update it by hand using the recipes below and note the bump in the commit message.

## `minimal.wasm`

Minimal `wasm32-wasip1` Reactor module that exposes `__kobako_eval` / `__kobako_run` as no-op stubs and omits the `__kobako_abi_version` export — the frozen witness for the `docs/behavior.md` E-42 absent-export branch (`Kobako::Sandbox.new` raises `Kobako::SetupError`). It deliberately has no in-repo source: a buildable fixture crate would violate the "no parallel fixture-driven wasm crates" convention, so any replacement is hand-authored (the `.wat` fixtures below show the text-format route).

## `minimal_abi_ok.wat` / `minimal_abi_mismatch.wat`

Hand-written text-format modules around the B-40 construction-time ABI version check; the ext's wasmtime `wat` feature loads them through the same `wasm_path:` path as binary artifacts. `minimal_abi_ok.wat` reports the current ABI version plus the `minimal.wasm` no-op stubs — the construction stand-in for tests that never invoke end-to-end (update its `i32.const` by hand on an ABI version bump). `minimal_abi_mismatch.wat` reports `9999` — the E-42 mismatch branch, deterministic regardless of future bumps (same convention as `snippet_wrong_version.mrb`).

## `snippet_*.{rb,mrb}` — `#preload(binary:)` fixtures

Each fixture exercises one path of `docs/behavior.md` B-32 / E-36 / E-37 / E-38 through the real `data/kobako.wasm`. Two are compiled from the matching `.rb` source; the rest are byte-level derivatives of `snippet_answers.mrb`. The recipes below assume `mrbc` is the host-target build from `vendor/mruby/build/host/bin/mrbc` (produced by the same vendored mruby tree as `libmruby.a`).

### `snippet_answers.mrb` — happy-path bytecode

Source: [`snippet_answers.rb`](snippet_answers.rb) (`ANSWERS = 42`). Compiled with `-g` so the IREP carries a `debug_info` section — the canonical-name path B-32 expects.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_answers.mrb test/fixtures/snippet_answers.rb
```

### `snippet_raise_boom.mrb` — E-36 binary form (top-level raise after clean load)

Source: [`snippet_raise_boom.rb`](snippet_raise_boom.rb) (`raise "boom from snippet"`). Compiled with `-g`.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_raise_boom.mrb test/fixtures/snippet_raise_boom.rb
```

### `snippet_no_debug.mrb` — B-32 stripped-bytecode acceptance

Same `ANSWERS = 42` source as `snippet_answers.mrb`, compiled **without** `-g`. The IREP omits `debug_info`; per the relaxed B-32 the guest still loads it and the snippet contributes top-level effects.

```sh
vendor/mruby/build/host/bin/mrbc -o test/fixtures/snippet_no_debug.mrb test/fixtures/snippet_answers.rb
```

### `snippet_wrong_version.mrb` — E-37 (RITE version mismatch)

Byte-level patch of `snippet_answers.mrb`: copy the blob, then overwrite the 4-byte RITE format version at offset 4 from `0400` to `9999`. The patched version must not match `RITE_BINARY_FORMAT_VER` in `vendor/mruby/include/mruby/dump.h`; `9999` keeps the failure deterministic regardless of future mruby version bumps.

```sh
cp test/fixtures/snippet_answers.mrb test/fixtures/snippet_wrong_version.mrb
printf '9999' | dd conv=notrunc of=test/fixtures/snippet_wrong_version.mrb bs=1 seek=4
```

### `snippet_corrupt.mrb` — E-38 (corrupt body / non-RITE input)

Header-prefix truncation of `snippet_answers.mrb`: keep the first 30 bytes — enough to pass the 4-byte `RITE` ident check and the 4-byte format-version check, but short enough that the IREP section parse inside `mrb_read_irep_buf` fails.

```sh
dd if=test/fixtures/snippet_answers.mrb of=test/fixtures/snippet_corrupt.mrb bs=1 count=30
```
