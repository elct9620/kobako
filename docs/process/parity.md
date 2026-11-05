# Host Parity

The Ruby gem (`lib/`) and the Rust SDK (`crates/kobako`) are two
Frontends over one wire, contract, and Guest Binary. Their APIs stay
idiomatic per language, while their host-observable behavior must never
drift. The differential parity harness turns drift on either side into a
failing comparison rather than a report from a Host App.

```
                 scenario (pure data)
                 /                  \
     Ruby executor                  Rust runner
     Kobako::Sandbox                kobako::Sandbox
                 \                  /
                Guest Binary (a)
                 \                  /
       raw observables -> normalize -> assert equal
```

- (a) Both sides run the same `data/kobako.wasm` with the same invocations.

## Harness Shape

One declarative scenario runs through both Frontends against the same
`data/kobako.wasm`, and each side reports what a host could observe.

| Side | Lives in | Drives |
|---|---|---|
| Ruby executor | `test/support/parity/ruby_executor.rb` | `Kobako::Sandbox` |
| Rust executor | `test/support/parity/rust_executor.rb` | the Rust runner |
| Rust runner | `crates/kobako-parity` | `kobako::Sandbox` |

The two must match field for field after normalization. Host-generated
wording and raw timing legitimately differ, so they stay
diagnostic-only; `test/support/parity/case.rb` owns that normalization.
The suite rides `rake test`. Its families skip on a checkout without
cargo, and fail under CI instead.

## Scenarios

A scenario is pure data, and every part of it draws on a closed set that
grows append-only with the corpus.

| Part | Holds |
|---|---|
| options | caps and the requested Posture |
| services | Service stubs and their Exposure |
| preloads | source or bytecode snippets |
| flags | Flags, each advisory or critical |
| extensions | Extensions to install |
| mounts | a Tmpfs seeded with files, its guest path and access |
| invocations | the verbs run in order |

Both executors interpret the same closed sets, so a new member lands on
both sides at once.

An `undefined` or `argument` fault must arise from the scenario's shape on
both sides, never from a stub declaration. Scenarios set the Exposure of
bound Services only, so the Exposure of a Handle target stays pinned by
each Frontend's dispatch unit tests.

## Handle Comparison

Capability Handles compare by the host object they name, not by id, so a
raw Handle id never appears in an observable.

```
Handle (a) --> Ruby (b) --+
           --> Rust (c) --+--> label (d)
```

- (a) An opaque stub or a `run` argument crosses as a Handle.
- (b) Decoding restores the host object itself.
- (c) `Execution::resolve` finds the object, matched with `Arc::ptr_eq`.
- (d) The observable is `{"t": "opaque", "label": ...}`.

An opaque stub or a `run` argument is a labeled non-wire host object.
The `run` verb carries tagged `args` and `kwargs`, exercising the
auto-wrap in both positions.

## Frontend Vocabulary

The [glossary](../spec/glossary.md) names each concept once, and each
Frontend reifies it under its own language's names. The surface a
Service author touches keeps the guest-visible word (`block`), while the
reified machinery carries the concept's own name.

| Concept | Ruby frontend | Rust SDK |
|---|---|---|
| Receiver | any object, through methods its class defines | the `Receiver` trait, for Services and Handle objects |
| Service | any object bound via `bind(path, object)`, duck-typed | a `Receiver` bound via `Sandbox::bind(path, object)` |
| Yielder | `Kobako::Transport::Yielder`, internal, in the `&block` slot | `kobako::Yielder`, the `block` parameter of `Receiver::call` |
| Block | never crosses; only the `block_given` flag travels | same; the wire contract is shared |
| Execution | `Kobako::Execution`, from `#eval` / `#run` | `kobako::Execution`, from `eval` / `run` |

An SDK `Receiver` whose `respond_to_guest` denies every name is opaque.
The Ruby Yielder stays internal so a Service method takes an ordinary
block ([transport-yield](../spec/behavior/transport-yield.md)). An SDK
Receiver calls its Yielder directly.

## Error Model

The error model is the Frontends' one lasting asymmetry: the Ruby host
raises a taxonomy error, while the SDK keeps failure a value.

| Run | Ruby frontend | Rust SDK |
|---|---|---|
| reached the guest, succeeded | returns the `Execution` | `Ok(Execution)`, `payload()` is `Ok` |
| reached the guest, failed | raises a taxonomy error | `Ok(Execution)`, `payload()` is `Err` |
| never started | raises | the outer `Err` |

Under the `msgpack` feature, the SDK's `value()` decodes the payload and
keeps the same `Ok` or `Err`. What each run produces is specified in
[sandbox](../spec/behavior/sandbox.md). The harness compares value,
captures, usage, and failure attribution after normalization, so the
raise-versus-return spelling never surfaces as drift.

## Compared Set

The behavior specification declares the compared set, so a behavior
joins it by declaring a scenario and claiming it from the case that runs
it.

```
scenario (a) --> parity case (b) --> sumi verify (c)
```

- (a) Its When step reads "both frontends run it".
- (b) Only a case in `test/parity/` witnesses it.
- (c) It fails once that case stops claiming the scenario.

### Unstageable Behavior

A behavior that no scenario can stage on both Frontends is pinned by each
Frontend on its own. Its owning feature says which case applies.

| Situation | Parity scenario |
|---|---|
| both Frontends run it | claimed by a case in `test/parity/` |
| they agree, but no scenario can stage it | declared `unverifiable` |
| they differ, or only one has it | none; each Frontend pins its own |

## Excluded Behavior

These categories stay outside the compared set, each pinned where its
behavior is decided.

| Category | Covers | Pinned by |
|---|---|---|
| Language surface | setup validation, pre-flight refusals, `Kobako::Pool`, option readers, construction failures, result shape | each Frontend, in its own idiom |
| Guest-internal | Regexp, JSON, guest proxies, guest methods a capability gem calls back | guest E2E suites and codec oracles |
| Unshipped codec | a payload position the guest's own codec does not serve | `wasm/kobako-mruby`'s refusal table |
| Wire corners | malformed envelopes and outcome bytes with no deterministic trigger | nothing yet |

Every codec kobako ships serves every position, so a refusing codec
shipping here moves that row into the compared set. Wire corners are
revisited if a legitimate trigger appears; parallel fixture guests stay
off the table.

## Split Features

Some features are compared in part, with only their spelling left to
each Frontend.

| Feature | Compared | Left to each Frontend |
|---|---|---|
| Seal | its timing | its spelling |
| Extensions | composition, backend resolution | the dependency assertion, install-error shapes |
| Posture | the requested Posture | the floor-refusal spelling |
| Mount | what the guest reads and its refusals | path validation, a Mount's Policy |
