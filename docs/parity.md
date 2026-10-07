# Host Parity

The Ruby gem (`lib/`) and the Rust host SDK (`crates/kobako`) are two
frontends over one wire, contract, and Guest Binary. Their APIs stay
idiomatic per language, while their host-observable behavior must never
drift. The differential parity harness turns drift on either side into a
failing comparison rather than a report from an embedder.

```
                 scenario (pure data)
                 /                  \
     Ruby executor                  Rust runner
     Kobako::Sandbox                kobako::Sandbox
                 \                  /
          same data/kobako.wasm, same invocations
                 \                  /
       raw observables -> normalize -> assert equal
```

## Harness Shape

One declarative scenario runs through both frontends against the same
`data/kobako.wasm`, and each side reports what a host could observe.

| Side | Lives in | Assembles |
|---|---|---|
| Ruby executor | `test/support/parity/ruby_executor.rb` | `Kobako::Sandbox` |
| Rust runner | `crates/kobako-parity` | `kobako::Sandbox`, over the CargoOracle framed protocol |

The two must match field for field after normalization. Host-generated
wording and raw timing legitimately differ, so they stay
diagnostic-only; `test/support/parity/case.rb` owns that normalization.
The suite rides `rake test`, and its families skip on a checkout without
cargo.

## Scenarios

A scenario is pure data, and every part of it draws on a closed set that
grows append-only with the corpus.

| Part | Holds |
|---|---|
| options | caps and the isolation posture |
| services | Service stubs and their narrowing |
| preloads | source or bytecode snippets |
| extensions | Extensions to install |
| invocations | the verbs run in order |

`test/support/parity/scenario.rb` defines the shape, and each executor
interprets the sets: `sandbox_builder.rb` on the Ruby side,
`crates/kobako-parity/src/main.rs` on the Rust side. A new member lands on
both sides at once.

An `undefined` or `argument` fault must arise from the scenario's shape on
both sides, never from a stub declaration. Scenarios narrow bound Services
only, so the narrowing of a Handle target stays pinned by each frontend's
dispatch unit tests.

## Handle Comparison

Capability Handles compare by identity, not id, so a raw Handle id never
appears in an observable.

```
opaque stub / run argument ──> crosses as a Handle
  Ruby: label read off the restored object        ─┐
  Rust: Handle -> Sandbox table -> object's label ─┴─> {"t": "opaque", "label": …}
```

An opaque stub or a `run` argument is a labeled non-wire host object.
The `run` verb carries tagged `args` and `kwargs`, exercising the
auto-wrap in both positions.

## Frontend Vocabulary

The [glossary](spec/glossary.md) names each concept once, and each
frontend reifies it under its own language's names. The surface a
Service author touches keeps the guest-visible word (`block`), while the
reified machinery carries the concept's own name.

| Concept | Ruby frontend | Rust SDK |
|---|---|---|
| Receiver | any object, through methods its class defines | the `Receiver` trait, for Services and Handle objects |
| Service | any object bound via `bind`, duck-typed | a `Receiver` bound via `Sandbox::bind` |
| Bound constant | `bind(path, object)` on the `Sandbox` | `Sandbox::bind(path, object)` |
| Yielder | `Kobako::Transport::Yielder`, internal, in the `&block` slot | `kobako::Yielder`, the `block` parameter of `Receiver::call` |
| Block | never crosses; only the `block_given` flag travels | same; the wire contract is shared |
| Execution | `Kobako::Execution`, from `#eval` / `#run` | `kobako::Execution`, from `eval` / `run` |

An SDK `Receiver` whose `respond_to_guest` denies every name is opaque.
The Ruby Yielder stays internal so a Service method takes an ordinary
block ([transport-yield](spec/behavior/transport-yield.md)). The SDK
yield site calls the Yielder directly, through `call_payload` or the
`msgpack` feature's `call_values`.

## Error Model

The error model is the frontends' one lasting asymmetry: the Ruby host
raises a taxonomy error, while the SDK keeps failure a value.

| Run | Ruby frontend | Rust SDK |
|---|---|---|
| reached the guest, succeeded | returns the `Execution` | `Ok(Execution)`, `value` is `Ok` |
| reached the guest, failed | raises a taxonomy error | `Ok(Execution)`, `value` is `Err` |
| never started | raises | the outer `Err` |

What each run produces is specified in
[sandbox](spec/behavior/sandbox.md). The harness compares value,
captures, usage, and failure attribution after normalization, so the
raise-versus-return spelling never surfaces as drift.

## Compared Set

The behavior specification declares the compared set, so a behavior
joins it by declaring a scenario and claiming it from the case that runs
it.

```
scenario  When: "both frontends run it"
   └─ witnessed only by a parity case in test/parity/
        └─ sumi verify fails once that case stops claiming it
```

### Unstageable Behavior

A behavior that no scenario can stage on both frontends is pinned by each
frontend on its own. Its owning feature says which case applies.

| Situation | Parity scenario |
|---|---|
| both frontends run it | claimed by a case in `test/parity/` |
| they agree, but no scenario can stage it | declared `unverifiable` |
| they differ, or only one has it | none; each frontend pins its own |

## Excluded Behavior

These categories stay outside the compared set, each pinned where its
behavior is decided.

| Category | Covers | Pinned by |
|---|---|---|
| Language surface | setup validation, pre-flight refusals, `Kobako::Pool`, option readers, construction failures, result shape | each frontend, in its own idiom |
| Guest-internal | Regexp, JSON, guest proxies, capability callbacks, unrepresentable-integer guest entry | guest E2E suites and codec oracles |
| Unshipped codec | a payload position the guest's own codec does not serve | `wasm/kobako-mruby`'s refusal table |
| Wire corners | malformed envelopes and outcome bytes with no deterministic trigger | nothing yet |

Every codec kobako ships serves every position, so a refusing codec
shipping here moves that row into the compared set. Wire corners are
revisited if a legitimate trigger appears; parallel fixture guests stay
off the table.

## Split Features

Some features are compared in part, with only their spelling left to
each frontend.

| Feature | Compared | Left to each frontend |
|---|---|---|
| Seal | its timing | its spelling |
| Extensions | composition, backend resolution | the dependency assertion, install-error shapes |
| Isolation posture | the requested posture | the floor-refusal spelling |
