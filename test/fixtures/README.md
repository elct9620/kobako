# Test fixtures

Static binaries the test suite reads via `File.binread` / `Kobako::Sandbox.new(wasm_path:)`. **Do not regenerate automatically.** Each blob is frozen, so an upstream shift surfaces here as a test failure instead of slipping into production.

| Upstream | What a shift would move |
|---|---|
| mruby | the bytecode layout and RITE header |
| wasi-sdk | the compiled module |
| kobako | the guest ABI surface |

If a fixture genuinely needs to track upstream (rare), update it by hand using the recipes below and note the bump in the commit message.

The hand-written `.wat` fixtures carry the ABI version as an `i32.const`. Update it by hand on a version bump, unless the fixture says otherwise.

## `minimal.wasm`

Minimal `wasm32-wasip1` Reactor module that exports only a `ping` function, so it lacks `__kobako_abi_version`. It is the frozen stand-in for a guest missing the ABI version export.

| Fact | Detail |
|---|---|
| construction | `Kobako::Sandbox.new` raises `Kobako::SetupError` |
| in-repo source | none, deliberately |
| replacement | hand-authored, by the `.wat` route below |

A buildable fixture crate would violate the "no parallel fixture-driven wasm crates" convention.

## `minimal_abi_ok.wat` / `minimal_abi_mismatch.wat`

Hand-written text-format modules around the construction-time ABI version check. The ext's wasmtime `wat` feature loads them through the same `wasm_path:` path as binary artifacts.

| Fixture | Reports | Stands in for |
|---|---|---|
| `minimal_abi_ok.wat` | the current ABI version, plus a no-op downcall | tests that never invoke end-to-end |
| `minimal_abi_mismatch.wat` | `9999` | the mismatch branch |

`9999` stays a mismatch regardless of future bumps, the same convention as `snippet_wrong_version.mrb`.

## `minimal_alloc_zero.wat`

Hand-written text-format module that passes the ABI version check but whose `__kobako_alloc` always returns `0`. It is the frozen stand-in for a failed guest allocation.

| Step | What happens |
|---|---|
| host reserves the downcall | the guest returns no memory |
| the failure surfaces | as `Kobako::SandboxError`, never a trap |
| the downcall | is never reached |

The runtime stays intact, which is why the failure is not a trap.

## `minimal_spinning_start.wat`

Hand-written text-format module whose start function never returns. It is the frozen stand-in for a Guest Binary that cannot finish being set up.

| Step | What happens |
|---|---|
| construction instantiates it | the start function spins |
| the setup deadline | fails construction as a `Kobako::SetupError` |
| any invocation | is never reached |

## `minimal_spinning_alloc.wat`

Hand-written text-format module that passes the ABI version check but whose `__kobako_alloc` never returns. It is the frozen stand-in for a guest that stalls while being set up.

| Step | What happens |
|---|---|
| host reserves the downcall | the guest spins |
| the deadline | stops it as the deadline's own trap |
| the downcall | is never reached |

## `minimal_unlinkable.wat` / `minimal_uninstantiable.wat`

Hand-written text-format modules that pass the ABI version check yet cannot become a runtime. Each refuses at one step, so construction failing there is what a test observes.

| Fixture | Refused at |
|---|---|
| `minimal_unlinkable.wat` | link — it imports a function no host provides |
| `minimal_uninstantiable.wat` | instantiation — its start function traps |

## `minimal_unframed_yield.wat`

Hand-written text-format module whose one dispatch asks the host to yield. It answers that yield with bytes the envelope cannot frame, which the real Guest Binary never does.

| Verb | The yield's answer |
|---|---|
| `#eval` | no bytes |
| `#run` | a tag outside the live set |

## `minimal_unframed_outcome.wat`

Hand-written text-format module whose downcall answers without an Outcome the envelope can frame — which the real Guest Binary never does. Each verb takes one of the two ways that leave no record.

| Verb | What the downcall answers |
|---|---|
| `#eval` | no bytes |
| `#run` | a tag outside the live set |

## `minimal_unsupported_guest.wat`

Hand-written text-format module that serves neither an invocation nor a block. The real Guest Binary serves both, so this stands in for one built without them.

| Verb | What it stages |
|---|---|
| `#eval` | the invocation answered as unsupported |
| `#run` | a call to `S#each` whose yield is answered as unsupported |

## `minimal_unknown_upcall.wat`

Hand-written text-format module that sends the host an upcall under an unassigned tag. Its `#eval` result reports the arm that came back, so a test reads the host's answer.

| The host answers | `#eval` returns |
|---|---|
| unsupported | `true` |
| anything else | `false` |

## `minimal_null_guest.wat`

Hand-written text-format module that satisfies the whole ABI and does nothing else. Its downcall ignores the request and answers a constant nil Result.

```
0x10 0xc0   the registry's ok tag, then the payload codec's nil
```

Unlike the fixtures above it exists for measurement rather than for a behaviour branch. `benchmark/host_invocation.rb` drives it, so the host's per-invocation cost is the total rather than a subtraction of two near-equal numbers. `test/e2e/sandbox/test_null_guest.rb` keeps it honest.

## `minimal_wide_outcome.wat`

Hand-written text-format module whose every downcall answers the same Array of five Integers, six values in all, without growing its memory.

```
0x10 0x95 0x00 0x00 0x00 0x00 0x00   ok, then [0, 0, 0, 0, 0]
```

A Sandbox whose memory cap allows fewer values must refuse that answer. The real Guest Binary cannot be made to send it, since encoding a value costs it more memory than the budget grants.

## `minimal_runaway_compile.wat`

Hand-written text-format module whose compile never finishes, so a cap has to stop it. The real Guest Binary compiles too quickly to reach one.

| Downcall | What it does |
|---|---|
| compile | grows memory by 16 MiB, then spins |
| any other | answers a nil Result at once |

## `minimal_compile_ambient.wat`

Hand-written text-format module whose compile answers the ambient bytes it read. Compiling runs no script, so the real Guest Binary cannot show which posture its compile ran at.

| Downcall | What it does |
|---|---|
| compile | answers one bytecode: the realtime clock, then 8 bytes of entropy |
| any other | answers a nil Result at once |

## `minimal_fsync.wat`

Hand-written text-format module that syncs a file of a Mount and answers the errno. The real Guest Binary imports no `fd_sync`, so no script can sync a file.

| Downcall | What it does |
|---|---|
| any | opens `note.txt` beneath the first preopen, syncs it, and answers the errno as an Integer |

## `minimal_moved_directory.wat`

Hand-written text-format module that looks up through a directory it holds open after the directory moved. The real Guest Binary holds no directory open, so no script can. Every downcall runs these steps beneath the first preopen.

1. Open `dir` as a directory.
2. Rename `dir` to `moved`.
3. Make another `dir`.
4. Stat `inner` through the descriptor from step 1, and answer its errno as an Integer.

## `snippet_*.{rb,mrb}` — `#preload(binary:)` fixtures

Each fixture exercises one bytecode-preload path of [`sandbox.md`](../../docs/spec/behavior/sandbox.md) through the real `data/kobako.wasm`.

| Fixture | Produced by |
|---|---|
| one with a matching `.rb` source | compiling that source |
| the rest | byte-level edits of `snippet_answers.mrb` |

The recipes below assume `mrbc` is the host-target build at `vendor/mruby/build/host/bin/mrbc`. The same vendored mruby tree produces `libmruby.a`.

### `snippet_answers.mrb` — happy-path bytecode

Source: [`snippet_answers.rb`](snippet_answers.rb) (`ANSWERS = 42`). Compiled with `-g` so the IREP carries a `debug_info` section — the happy path of a bytecode preload.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_answers.mrb test/fixtures/snippet_answers.rb
```

### `snippet_raise_boom.mrb` — clean-load raise

Source: [`snippet_raise_boom.rb`](snippet_raise_boom.rb) (`raise "boom from snippet"`), a top-level raise after a clean load. Compiled with `-g`.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_raise_boom.mrb test/fixtures/snippet_raise_boom.rb
```

### `snippet_raise_boom_no_debug.mrb` — frameless raise

Same source as `snippet_raise_boom.mrb`, compiled **without** `-g`, so the failure keeps its class, message and origin while the snippet's frames are absent.

```sh
vendor/mruby/build/host/bin/mrbc -o test/fixtures/snippet_raise_boom_no_debug.mrb test/fixtures/snippet_raise_boom.rb
```

### `snippet_raise_script_error.mrb` / `snippet_raise_not_implemented.mrb`

These two pin that a raise from loaded bytecode keeps its class. Both are compiled with `-g`.

| Source | Raises | Reads as |
|---|---|---|
| [`snippet_raise_script_error.rb`](snippet_raise_script_error.rb) | `ScriptError` itself | its own name |
| [`snippet_raise_not_implemented.rb`](snippet_raise_not_implemented.rb) | the `NotImplementedError` subclass | its own name |

A blob that fails its structural check also answers `ScriptError`, but never runs.

```sh
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_raise_script_error.mrb test/fixtures/snippet_raise_script_error.rb
vendor/mruby/build/host/bin/mrbc -g -o test/fixtures/snippet_raise_not_implemented.mrb test/fixtures/snippet_raise_not_implemented.rb
```

### `snippet_no_debug.mrb` — stripped-bytecode acceptance

Same `ANSWERS = 42` source as `snippet_answers.mrb`, compiled **without** `-g`. The IREP omits `debug_info`; the guest still loads it and the snippet contributes top-level effects.

```sh
vendor/mruby/build/host/bin/mrbc -o test/fixtures/snippet_no_debug.mrb test/fixtures/snippet_answers.rb
```

### `snippet_wrong_version.mrb` — version mismatch

Byte-level patch of `snippet_answers.mrb`: copy the blob, then overwrite the 4-byte RITE format version at offset 4 from `0400` to `9999`. The patched version must not match `RITE_BINARY_FORMAT_VER` in `vendor/mruby/include/mruby/dump.h`; `9999` keeps the failure deterministic regardless of future mruby version bumps.

```sh
cp test/fixtures/snippet_answers.mrb test/fixtures/snippet_wrong_version.mrb
printf '9999' | dd conv=notrunc of=test/fixtures/snippet_wrong_version.mrb bs=1 seek=4
```

### `snippet_corrupt.mrb` — corrupt body

Header-prefix truncation of `snippet_answers.mrb` that keeps the first 30 bytes, standing in for corrupt or non-RITE input. That passes the 4-byte `RITE` ident check and the 4-byte format-version check. It is short enough that the IREP section parse inside `mrb_read_irep_buf` fails.

```sh
dd if=test/fixtures/snippet_answers.mrb of=test/fixtures/snippet_corrupt.mrb bs=1 count=30
```

## `file_open_matrix.rb`

`file_open_matrix.rb` opens seeded entries with each mode, then reads, writes, and wraps them with `IO.new`. It runs as is on CRuby and in the guest. The two text files hold CRuby's answers on Linux, which the guest's must match or refuse by a rule.

| Fixture | CRuby ran over |
|---|---|
| `file_open_matrix_linux_rw.txt` | a read-write directory |
| `file_open_matrix_linux_ro.txt` | the same entries, mounted read-only |

Regenerate them after a change to the script, by seeding a directory as the script's header describes and running CRuby in a container:

```sh
docker run --rm -v "$PWD/test/fixtures/file_open_matrix.rb:/m.rb:ro" -v "$SEEDED:/data" \
  ruby:3.4-slim ruby -e 'ROOT = "/data"; puts eval(File.read("/m.rb"))' > test/fixtures/file_open_matrix_linux_rw.txt
```

Mount `$SEEDED` with `:ro` for the read-only answers.
