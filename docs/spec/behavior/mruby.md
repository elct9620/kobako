# mruby guest

The state every invocation starts from, and what a raise inside a capability gem's frame costs.

### Why these scenarios

Two invocations of one artifact begin from the same interpreter state, not merely from a clean one. The heap-slot replay settles the difference. An interpreter that reset everything correctly but started somewhere new each time would pass every leak test. It would still fail this one.

A capability gem servicing one guest operation calls back into guest code. The raise that call may produce has to stay a guest exception. The two coercion entries are witnessed separately because they are separate frames. The recovery scenario tells a guest error apart from a retired Sandbox.

The boot state may be computed at build time, and per-invocation resources may be provisioned ahead of demand. Both are stated to be unobservable, so neither is a scenario. The reproducible-build check holds the baking end. Leak-freedom between invocations is per-invocation isolation and belongs with the Sandbox behaviors, whose witnesses cite it alongside this one.

The default Guest Binary recognises one flag, which the file scenarios cover, so only the unrecognised paths are scenarios here. A critical flag refuses, because a restriction must never lapse silently.

A host may keep the guest's compiled snippets and replay them in place of the source. That is safe only if the bytecode replays as the source would. So the scenarios pin what is defined, how a raise is attributed, and that compiling runs nothing. Identical bytes let two compiles of one Sandbox stand in for each other.

## Includes

- `test/e2e/test_canonical_boot.rb`
- `test/e2e/test_capability_exception_safety.rb`
- `test/e2e/test_guest_string_cap.rb`
- `test/e2e/test_integer_range.rb`
- `test/e2e/regexp/test_raising_lookup.rb`
- `test/e2e/json/test_raising_hook.rb`
- `wasm/kobako-mruby/src/runtime.rs`
- `wasm/kobako-mruby/src/flows/compile.rs`
- `crates/kobako-wasmtime/tests/integration/preamble_flags.rs`
- `crates/kobako-wasmtime/tests/integration/snippet_compile.rs`

## `MR-001` Two invocations begin from the same state, not merely a clean one

| Step | Statement |
| --- | --- |
| Given | a Sandbox that has evaluated `Object.new.object_id` once |
| When | the same source is evaluated again |
| Then | the same object id comes back |

## `MR-002` A constant an invocation defines is gone at the next entry

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose evaluation defined a constant |
| When | a later invocation asks whether that constant is defined |
| Then | it is not |

## `MR-003` A raise in an output coercion is the guest's error, not the Sandbox's

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `to_s` raises |
| Given | an instance of it passed to `$stdout.puts` |
| When | the invocation runs |
| Then | it fails as a Sandbox failure rather than as a trap |

## `MR-004` The guest's own message survives the coercion frame

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `to_s` raises |
| Given | an instance of it passed to `$stdout.puts` |
| When | the invocation runs |
| Then | the raised error carries the guest exception's message |

## `MR-005` The second coercion entry answers like the first

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `inspect` raises |
| Given | an instance of it passed to `p` |
| When | the invocation runs |
| Then | it fails as a Sandbox failure rather than as a trap |

## `MR-006` The guest's own message survives the inspect frame

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `inspect` raises |
| Given | an instance of it passed to `p` |
| When | the invocation runs |
| Then | the raised error carries the guest exception's message |

## `MR-007` A raising callback costs the Sandbox nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation failed on a raising output coercion |
| When | a later invocation evaluates ordinary guest source |
| Then | it answers its value |

## `MR-008` The interpreter bounds a String below the message cap

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose memory budget is far above 1 MiB |
| When | guest code builds a String of 1 MiB |
| Then | an `ArgumentError` the guest can rescue is raised |

## `MR-009` The permissive profile starts every invocation from the same state

| Step | Statement |
| --- | --- |
| Given | a Sandbox built under the permissive profile |
| When | two invocations each allocate their first object |
| Then | both land on the same heap slot, as under the hermetic profile |

## `MR-010` The default Guest Binary reaches no clock or entropy

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the default Guest Binary |
| When | guest code looks for time, sleep, or randomness |
| Then | none of them is defined |

## `MR-011` The caller of an output method can rescue a raising coercion

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code rescues around an output call whose argument's coercion raises |
| Then | the rescue receives that raise and the invocation answers normally |

## `MR-012` A raising Hash lookup during substitution stays a guest exception

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code rescues around a substitution whose Hash replacement's lookup raises |
| Then | the rescue receives that raise and the invocation answers normally |

## `MR-013` A raising serialization hook stays a guest exception

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code rescues around a generation whose serialization hook raises |
| Then | the rescue receives that raise and the invocation answers normally |

## `MR-014` A shell installing no gems boots a bridge-only guest

| Step | Statement |
| --- | --- |
| Given | a guest shell whose gem hook installs nothing |
| When | the interpreter boots |
| Then | it boots |

## `MR-015` A String a byte under the interpreter's bound is built

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose memory budget is far above 1 MiB |
| When | guest code builds a String a byte shorter than 1 MiB |
| Then | it is built in full |

## `MR-016` A shell's gem hook runs with the bridge already in place

| Step | Statement |
| --- | --- |
| Given | a guest shell whose gem hook installs nothing |
| When | the interpreter boots |
| Then | the hook finds the bridge already installed |

## `MR-017` A critical flag the guest does not recognise refuses the invocation

| Attribute | Value |
| --- | --- |
| relates | `WE-078` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the default Guest Binary and a critical flag it does not recognise |
| When | an invocation runs |
| Then | it fails as the Sandbox's unsupported-flag failure before any script runs |

## `MR-018` An advisory flag the guest does not recognise is skipped

| Attribute | Value |
| --- | --- |
| relates | `MR-017`, `WE-077` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the default Guest Binary and an advisory flag it does not recognise |
| When | an invocation runs |
| Then | it answers as it would without the flag |

## `MR-019` Compiled snippets replay what their source defines

| Attribute | Value |
| --- | --- |
| relates | `S-055` |

| Step | Statement |
| --- | --- |
| Given | source snippets compiled by the guest |
| When | their bytecode is replayed in order in another interpreter |
| Then | it defines what the source defined |

## `MR-020` Compiled snippets keep their name in backtraces

| Attribute | Value |
| --- | --- |
| relates | `MR-019`, `S-088` |

| Step | Statement |
| --- | --- |
| Given | a compiled snippet defining a method that raises |
| When | the replayed method raises |
| Then | its backtrace names the snippet and the line, as the source replay does |

## `MR-021` Compiling runs nothing in the snippet

| Step | Statement |
| --- | --- |
| Given | a snippet that raises when it runs |
| When | the guest compiles it |
| Then | it compiles |

## `MR-022` One snippet that does not compile fails the whole compile

| Attribute | Value |
| --- | --- |
| relates | `MR-019`, `S-084` |

| Step | Statement |
| --- | --- |
| Given | snippets of which one does not parse |
| When | the guest compiles them |
| Then | the compile fails as a syntax error naming that snippet, with no bytecode |

## `MR-023` The same snippets compile to the same bytes

| Step | Statement |
| --- | --- |
| Given | the same snippets |
| When | two fresh interpreters each compile them |
| Then | both answer byte-identical bytecode |

## `MR-024` The default Guest Binary's compiled snippets replay in a later invocation

| Attribute | Value |
| --- | --- |
| refines | `MR-019` |
| relates | `S-055` |

| Step | Statement |
| --- | --- |
| Given | the default Guest Binary's bytecode for a source snippet |
| When | a later invocation is sent that bytecode as its snippet |
| Then | the invocation reaches what the source defined |

## `MR-025` A critical flag the guest does not recognise leaves nothing compiled

| Attribute | Value |
| --- | --- |
| relates | `MR-017`, `MR-022` |

| Step | Statement |
| --- | --- |
| Given | a Preamble carrying a critical flag the default Guest Binary does not recognise |
| When | the guest compiles its snippets |
| Then | nothing is compiled |

## `MR-026` An integer literal past 64 bits stays exact inside the guest

| Attribute | Value |
| --- | --- |
| relates | `CD-053` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes an integer literal past the signed 64-bit range |
| Then | it reads as that exact Integer |
