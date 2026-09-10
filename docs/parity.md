# Ruby ↔ Rust Host Parity

The Ruby gem (`lib/`) and the Rust host SDK (`crates/kobako`) are two
frontends over the same wire, contract, and Guest Binary. Their API
shapes are deliberately idiomatic per language; what must never drift
is the **host-observable behavior** — which value comes back, which
error origin is attributed, what the captures and usage readers show.
The differential parity harness mechanizes that check so a behavior
change on one side surfaces as a failing comparison, not as a report
from an embedder.

## Mechanism

One declarative scenario — caps, Service stubs, invocations, all pure
data — is executed by both frontends against the same
`data/kobako.wasm`:

- the Ruby executor (`test/support/parity/ruby_executor.rb`) assembles
  a `Kobako::Sandbox`;
- the Rust runner (`crates/kobako-parity`) assembles a `kobako::Sandbox`
  and answers over the CargoOracle framed protocol.

Both emit raw observables per invocation — neutral status, tagged
value, capture bytes and truncation predicates, usage — and the test
asserts equality after normalization (`test/support/parity/case.rb`:
host-generated `message` wording and raw usage numbers are
diagnostic-only). Stub behaviors (`echo` / `echo_positional` / `value`
/ `raise` / `yield_each` / `opaque` / `read_label`), invocation verbs
(`eval` / `run` / `late_bind`), and preload kinds (`source` /
`bytecode`) are closed sets that grow append-only with the corpus;
`undefined` / `argument` faults must arise from the scenario's shape
on both sides, never from a stub declaration (`echo_positional`
declares a positional-only signature, so kwargs on the wire fail its
binding on both sides). A service's optional `exposed` list declares
the `respond_to_guest?` narrowing both stubs enforce; the scenarios
narrow bound Services only — both dispatchers run the same narrowing
check for Handle targets, but opaque stubs expose just `label`, so
that rejection path stays pinned by each frontend's dispatch unit
tests.

Capability Handles compare by **identity, not id**: an `opaque` stub
(or `run` argument — the `run` verb carries tagged `args` and
`kwargs`, exercising the auto-wrap in both positions) is a labeled
non-wire host object, and a crossed
object tags as `{"t": "opaque", "label": …}` on both sides — the Ruby
executor reads the label off the restored object, the Rust runner
resolves the result Handle against the Sandbox's table and recovers
the label by object identity. A raw Handle id never appears in an
observable.

The suite rides `rake test`; on a checkout without cargo the families
skip. A compared behavior with no guest-expressible differential
scenario is pinned per-frontend instead — see What the harness compares.

## Frontend vocabulary

SPEC's Internal Concepts glossary words each concept against the Ruby
frontend; the SDK reifies the same concepts under Rust names. One rule
keeps the two surfaces coherent: the surface a Service author touches
keeps the guest-visible word (`block`), while the reified machinery
carries the concept's own name.

| SPEC concept | Ruby frontend | Rust SDK |
|---|---|---|
| Receiver — the host object a dispatch resolves its target to | any Ruby object, reached by reflection under the reflection floor ([`T-117`](spec/behavior/transport-boundary.md)) | the `Receiver` trait — one dispatch contract covering bound Services and Handle-allocated objects; a Receiver whose `respond_to_guest` denies every name is opaque ([`T-124`](spec/behavior/transport-boundary.md)) |
| Service — the host object bound at a constant-path name | any Ruby object bound via `bind` (duck-typed) | a `Receiver` bound via `Sandbox::bind` |
| Bound constant — the leaf name of a constant path | `bind(path, object)` on the `Sandbox` | `Sandbox::bind(path, object)` |
| Yielder — the host-side stand-in for a guest Block | `Kobako::Transport::Yielder`, internal: it rides the `&block` slot, so the Service method sees an ordinary Proc | `kobako::Yielder`, public: it rides the `block` parameter of `Receiver::call`, so the yield site still reads `block.call(args)` |
| Block — the guest-side block body | never crosses the wire; only the Call's `block_given` flag travels | same — the wire contract is shared |
| Invocation result — the value a run produced with its captures and usage | `Kobako::Execution`, returned from `#eval` / `#run` | `kobako::Execution`, returned from `eval` / `run` |

The result surface carries the frontends' one lasting asymmetry, the
error model. The guest reports success or failure as a value (its
`Outcome`); the Ruby host raises a taxonomy error carrying the frozen
`Execution` on `#execution`, while the SDK keeps failure a value — a run
that reached the guest is `Ok(Execution)`, and its outcome (the value or a
failure `Error`) rides `Execution::value` as a `Result`, so a caller cannot
pass over a guest failure unnoticed. Ruby's
`Execution#failed?` is the mirror of the SDK's `Err` arm: both let a
caller tell a failed run from a success whose value was legitimately
`nil`, on either side of the raise-versus-return split. A run that never
started raises without an Execution on Ruby and is the outer `Err` on the
SDK. The harness compares the observables both carry —
value, captures, usage, failure attribution — after normalization, so
this raise-versus-return spelling never surfaces as drift.

## What the harness compares

The compared set is declared in the behavior specification: every
scenario whose operation reads *both frontends run it* — for example
[`S-016`](spec/behavior/sandbox.md), [`T-075`](spec/behavior/transport-dispatch.md),
[`EX-038`](spec/behavior/extension.md) — can be witnessed only by a
parity case, so `sumi verify` fails when one loses the case that
claims it. A behavior joins the set by declaring such a scenario and
claiming it from the case that runs it.

Where a compared behavior has no guest-expressible differential
scenario, the feature that owns it says why and each frontend pins it
on its own: an engine trap no cap caused ([`outcome.md`](spec/behavior/outcome.md)),
a Yielder held past its frame ([`transport-yield.md`](spec/behavior/transport-yield.md)),
a stale reference ([`transport-dispatch.md`](spec/behavior/transport-dispatch.md)),
and a reflective object returned from a host method ([`transport-boundary.md`](spec/behavior/transport-boundary.md)).

## Outside the compared set

- **Language surface** — setup-time validation, host pre-flight
  refusals, `Kobako::Pool`, option readers, construction failures, and
  the shape of the result object: each frontend spells these in its own
  idiom. The seal's *timing* is compared ([`S-099`](spec/behavior/sandbox.md))
  while its spelling is not; Extension composition and backend
  resolution are compared while the dependency assertion and the
  install-error shapes are not; a requested isolation posture is
  compared ([`RT-027`](spec/behavior/runtime.md)) while its
  floor-refusal spelling is not.
- **Guest-internal** — behavior the shared Guest Binary fixes regardless
  of frontend (Regexp, JSON, guest-side proxy construction and probing,
  capability callbacks, the guest-entry refusal of an unrepresentable
  integer): pinned by the guest E2E suites and the codec oracles.
- **Reachable only through a codec kobako does not ship** — a payload
  position the guest's own codec does not serve. Nothing about the
  refusal is frontend-specific, but every codec kobako ships serves
  every position, so its attribution is pinned a tier below both
  frontends, in `wasm/kobako-mruby`'s refusal table
  ([`CD-026`](spec/behavior/codec.md) onward). A refusing codec shipping
  here moves it into the compared set.
- **Hard-to-trigger wire corners** — malformed envelopes and outcome
  bytes with no deterministic trigger through the real guest: revisit
  if a legitimate trigger appears; parallel fixture guests stay off the
  table.
