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

## Harness Mechanism

One declarative scenario of caps, Service stubs, and invocations runs
through both frontends against the same `data/kobako.wasm`.

| Side | Lives in | Assembles |
|---|---|---|
| Ruby executor | `test/support/parity/ruby_executor.rb` | `Kobako::Sandbox` |
| Rust runner | `crates/kobako-parity` | `kobako::Sandbox`, over the CargoOracle framed protocol |

Each side emits raw observables per invocation: neutral status, tagged
value, capture bytes and truncation predicates, and usage. The test
asserts equality after normalization in `test/support/parity/case.rb`,
where host-generated `message` wording and raw usage numbers are
diagnostic-only. The suite rides `rake test`; on a checkout without
cargo the families skip.

## Scenario Vocabulary

A scenario draws on closed sets that grow append-only with the corpus.

| Set | Members |
|---|---|
| Stub behaviors | `echo`, `echo_positional`, `value`, `raise`, `yield_each`, `opaque`, `read_label` |
| Invocation verbs | `eval`, `run`, `late_bind` |
| Preload kinds | `source`, `bytecode` |
| Service option | `exposed`, the `respond_to_guest?` narrowing both stubs enforce |

An `undefined` or `argument` fault must arise from the scenario's shape
on both sides, never from a stub declaration. `echo_positional` declares
a positional-only signature, so kwargs on the wire fail its binding on
both sides. Scenarios narrow bound Services only: opaque stubs expose
just `label`, so the Handle-target narrowing stays pinned by each
frontend's dispatch unit tests.

## Handle Comparison

Capability Handles compare by identity, not id, so a raw Handle id never
appears in an observable.

```
opaque stub / run argument ──> crosses as a Handle
  Ruby: label read off the restored object        ─┐
  Rust: Handle -> Sandbox table -> object's label ─┴─> {"t": "opaque", "label": …}
```

An `opaque` stub or a `run` argument is a labeled non-wire host object.
The `run` verb carries tagged `args` and `kwargs`, exercising the
auto-wrap in both positions.

## Frontend Vocabulary

The [glossary](spec/glossary.md) names each concept once, and each
frontend reifies it under its own language's names. The surface a
Service author touches keeps the guest-visible word (`block`), while the
reified machinery carries the concept's own name.

| Concept | Ruby frontend | Rust SDK |
|---|---|---|
| Receiver | any object, through methods its class defines under the reflection floor | the `Receiver` trait, one contract for Services and Handle objects |
| Service | any object bound via `bind`, duck-typed | a `Receiver` bound via `Sandbox::bind` |
| Bound constant | `bind(path, object)` on the `Sandbox` | `Sandbox::bind(path, object)` |
| Yielder | `Kobako::Transport::Yielder`, internal, in the `&block` slot | `kobako::Yielder`, public, the `block` parameter of `Receiver::call` |
| Block | never crosses; only the Call's `block_given` flag travels | same; the wire contract is shared |
| Execution | `Kobako::Execution`, from `#eval` / `#run` | `kobako::Execution`, from `eval` / `run` |

An SDK `Receiver` whose `respond_to_guest` denies every name is opaque.
The Ruby Yielder stays internal so a Service method takes an ordinary
block ([transport-yield](spec/behavior/transport-yield.md)); the SDK
yield site still reads `block.call(args)`.

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

## Per-Frontend Pins

A compared behavior with no guest-expressible differential scenario is
pinned by each frontend on its own, and the owning feature says why.

| Behavior | Owner |
|---|---|
| an engine trap no cap caused | [outcome](spec/behavior/outcome.md) |
| a Yielder held past its frame | [transport-yield](spec/behavior/transport-yield.md) |
| a stale reference | [transport-dispatch](spec/behavior/transport-dispatch.md) |
| a reflective object a host method returns | [transport-boundary](spec/behavior/transport-boundary.md) |

The engine trap and the stale reference keep a skipped placeholder in
`test/parity/`.

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
