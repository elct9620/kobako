# Test fixtures

Static binaries the test suite reads via `File.binread` / `Kobako::Sandbox.new(wasm_path:)`. **Do not regenerate automatically.** Each blob is intentionally frozen so future mruby / wasi-sdk / kobako changes that shift the bytecode layout, RITE header, or guest ABI surface as test failures here instead of slipping into production. If a fixture genuinely needs to track upstream (rare), update it by hand using the recipes below and note the bump in the commit message.

## `minimal.wasm`

Minimal `wasm32-wasip1` Reactor module that exposes `__kobako_eval` / `__kobako_run` as no-op stubs and omits the `__kobako_abi_version` export — the frozen stand-in for a guest missing the ABI version export (`Kobako::Sandbox.new` raises `Kobako::SetupError`). It deliberately has no in-repo source: a buildable fixture crate would violate the "no parallel fixture-driven wasm crates" convention, so any replacement is hand-authored (the `.wat` fixtures below show the text-format route).

## `minimal_abi_ok.wat` / `minimal_abi_mismatch.wat`

Hand-written text-format modules around the construction-time ABI version check; the ext's wasmtime `wat` feature loads them through the same `wasm_path:` path as binary artifacts. `minimal_abi_ok.wat` reports the current ABI version plus the `minimal.wasm` no-op stubs — the construction stand-in for tests that never invoke end-to-end (update its `i32.const` by hand on an ABI version bump). `minimal_abi_mismatch.wat` reports `9999` — the mismatch branch, deterministic regardless of future bumps (same convention as `snippet_wrong_version.mrb`).

## `minimal_alloc_zero.wat`

Hand-written text-format module that passes the ABI version check but whose `__kobako_alloc` always returns `0` — the frozen stand-in for a failed guest allocation: the host cannot reserve guest memory for the Run envelope, a runtime-intact failure surfacing as `Kobako::SandboxError` (never a trap; the guest entry point is never reached). Update its `i32.const` ABI version by hand on a bump, same as `minimal_abi_ok.wat`.

## `minimal_unlinkable.wat` / `minimal_uninstantiable.wat`

Hand-written text-format modules that pass the ABI version check yet cannot become a runtime. Each refuses at one step, so construction failing there is what a test observes.

| Fixture | Refused at |
|---|---|
| `minimal_unlinkable.wat` | link — it imports a function no host provides |
| `minimal_uninstantiable.wat` | instantiation — its start function traps |

## `minimal_trap_after_result.wat`

`minimal_null_guest.wat` with both entry points replaced by `unreachable`. Its Outcome buffer still holds a well-formed nil Result, so a host that read the buffer after a trap would answer nil where the invocation trapped. Update its `i32.const` ABI version by hand on a bump.

```wat
(func (export "__kobako_eval") unreachable)
```

## `minimal_unframed_yield.wat`

Hand-written text-format module whose one dispatch asks the host to yield, then answers that yield with bytes the envelope cannot frame — which the real Guest Binary never does. `#eval` answers with no bytes, `#run` with a tag outside the live set. Update its `i32.const` ABI version by hand on a bump.

## `minimal_null_guest.wat`

Hand-written text-format module that satisfies the whole invocation ABI and does nothing else: both entry points ignore their input and `__kobako_take_outcome` answers a constant nil Result (`0x01 0xc0` — the fixed layout's result tag, then the payload codec's nil). Unlike the fixtures above it exists for measurement rather than for a behaviour branch: `benchmark/host_invocation.rb` drives it so the host's per-invocation cost is the total rather than a subtraction of two near-equal numbers. `test/e2e/sandbox/test_null_guest.rb` keeps it honest. Update its `i32.const` ABI version by hand on a bump, same as `minimal_abi_ok.wat`.

## `snippet_*.{rb,mrb}` — `#preload(binary:)` fixtures

Each fixture exercises one bytecode-preload path of [`sandbox.md`](../../docs/spec/behavior/sandbox.md) through the real `data/kobako.wasm`. Those with a matching `.rb` source are compiled from it; the rest are byte-level derivatives of `snippet_answers.mrb`. The recipes below assume `mrbc` is the host-target build from `vendor/mruby/build/host/bin/mrbc` (produced by the same vendored mruby tree as `libmruby.a`).

### `snippet_answers.mrb` — happy-path bytecode

Source: [`snippet_answers.rb`](snippet_answers.rb) (`ANSWERS = 42`). Compiled with `-g` so the IREP carries a `debug_info` section — the happy path of a bytecode preload.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_answers.mrb test/fixtures/snippet_answers.rb
```

### `snippet_raise_boom.mrb` — top-level raise after a clean load

Source: [`snippet_raise_boom.rb`](snippet_raise_boom.rb) (`raise "boom from snippet"`). Compiled with `-g`.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_raise_boom.mrb test/fixtures/snippet_raise_boom.rb
```

### `snippet_raise_boom_no_debug.mrb` — a raise with no frames to report

Same source as `snippet_raise_boom.mrb`, compiled **without** `-g`, so the failure keeps its class, message and origin while the snippet's frames are absent.

```sh
vendor/mruby/build/host/bin/mrbc -o test/fixtures/snippet_raise_boom_no_debug.mrb test/fixtures/snippet_raise_boom.rb
```

### `snippet_raise_script_error.mrb` / `snippet_raise_not_implemented.mrb` — the class a structural failure carries

Sources: [`snippet_raise_script_error.rb`](snippet_raise_script_error.rb) raises `ScriptError` itself — the class a load answers for a blob that fails its structural check, so it reads as a structural failure; [`snippet_raise_not_implemented.rb`](snippet_raise_not_implemented.rb) raises the `NotImplementedError` subclass, which keeps its own name. Both compiled with `-g`.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_raise_script_error.mrb test/fixtures/snippet_raise_script_error.rb
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_raise_not_implemented.mrb test/fixtures/snippet_raise_not_implemented.rb
```

### `snippet_no_debug.mrb` — stripped-bytecode acceptance

Same `ANSWERS = 42` source as `snippet_answers.mrb`, compiled **without** `-g`. The IREP omits `debug_info`; the guest still loads it and the snippet contributes top-level effects.

```sh
vendor/mruby/build/host/bin/mrbc -o test/fixtures/snippet_no_debug.mrb test/fixtures/snippet_answers.rb
```

### `snippet_wrong_version.mrb` — RITE version mismatch

Byte-level patch of `snippet_answers.mrb`: copy the blob, then overwrite the 4-byte RITE format version at offset 4 from `0400` to `9999`. The patched version must not match `RITE_BINARY_FORMAT_VER` in `vendor/mruby/include/mruby/dump.h`; `9999` keeps the failure deterministic regardless of future mruby version bumps.

```sh
cp test/fixtures/snippet_answers.mrb test/fixtures/snippet_wrong_version.mrb
printf '9999' | dd conv=notrunc of=test/fixtures/snippet_wrong_version.mrb bs=1 seek=4
```

### `snippet_corrupt.mrb` — corrupt body / non-RITE input

Header-prefix truncation of `snippet_answers.mrb`: keep the first 30 bytes — enough to pass the 4-byte `RITE` ident check and the 4-byte format-version check, but short enough that the IREP section parse inside `mrb_read_irep_buf` fails.

```sh
dd if=test/fixtures/snippet_answers.mrb of=test/fixtures/snippet_corrupt.mrb bs=1 count=30
```
