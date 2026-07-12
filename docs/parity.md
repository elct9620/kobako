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
skip. A CORE anchor with no guest-expressible differential scenario is
listed under Pending anchors instead and pinned per-frontend — see
that section.

## Frontend vocabulary

SPEC's Internal Concepts glossary words each concept against the Ruby
frontend; the SDK reifies the same concepts under Rust names. One rule
keeps the two surfaces coherent: the surface a Service author touches
keeps the guest-visible word (`block`), while the reified machinery
carries the concept's own name.

| SPEC concept | Ruby frontend | Rust SDK |
|---|---|---|
| Receiver — the host object a dispatch resolves its target to | any Ruby object, reached by reflection under the B-42 floor | the `Receiver` trait — one dispatch contract covering bound Services and Handle-allocated objects; a Receiver whose `respond_to_guest` denies every name is opaque (B-50) |
| Service — the host object bound at a constant-path name | any Ruby object bound via `bind` (duck-typed) | a `Receiver` bound via `Sandbox::bind` |
| Member — the leaf name of a constant path | `bind(path, object)` on the `Sandbox` | `Sandbox::bind(path, object)` |
| Yielder — the host-side stand-in for a guest Block | `Kobako::Transport::Yielder`, internal: it rides the `&block` slot, so the Service method sees an ordinary Proc | `kobako::Yielder`, public: it rides the `block` parameter of `Receiver::call`, so the yield site still reads `block.call(args)` |
| Block — the guest-side block body | never crosses the wire; only the Request's `block_given` flag travels | same — the wire contract is shared |

## Coverage gate

`rake parity:coverage` requires every CORE anchor below to be either
**asserted** — named in a scenario's `anchors:` list, so a scenario
actually runs it through both frontends — or **pending** — listed in
the Pending anchors block, where no guest-expressible differential
scenario exists and the behavior is pinned per-frontend instead. An
anchor mentioned only in a comment or a `skip` message counts as
neither, so a scenario that silently degrades to comment-only fails
the gate. (`rake anchors` separately guarantees every ID resolves to a
real definition.)

## CORE anchor manifest

Host-observable behaviors both frontends must exhibit identically —
the parity harness's target surface:

```
B-01 B-02 B-03 B-04 B-06 B-12 B-13 B-14 B-16 B-17 B-18 B-20
B-23 B-24 B-25 B-26 B-27 B-28 B-29 B-30 B-31 B-32 B-33 B-34 B-35 B-37
B-42 B-43 B-45 B-49 B-50 B-55 B-56
E-01 E-04 E-05 E-06 E-11 E-12 E-13 E-15 E-19 E-20 E-21 E-22 E-23
E-27 E-28 E-32 E-36 E-37 E-38 E-43 E-44 E-48
```

## Pending anchors

CORE anchors with no guest-expressible differential scenario. The
coverage gate accepts these in place of an asserted scenario; each is
pinned per-frontend where its behavior is actually verified:

```
E-01 E-23 B-18 E-13 B-43 E-44
```

- **E-01** — a raw engine trap has no deterministic pure-guest trigger:
  the guest turns deep recursion into its own `SystemStackError`, and
  the live host-callback path is frontend-specific. Ruby-side behavior
  is pinned by `test/e2e/test_capability_exception_safety.rb`, trap-kind
  routing by the driver's `classify_trap` unit tests.
- **E-23** — the SDK's `Yielder` borrows its dispatch frame, so an
  escaped-Yielder misuse is a compile error with no runnable scenario;
  the Ruby frontend's runtime refusal is pinned by
  `test/e2e/test_yield_unwind.rb`.
- **B-18 / E-13** — one fresh guest instance per invocation means no
  scenario can present a stale Handle; staleness is unit-pinned by
  `test/transport/test_dispatcher_invalidity.rb` and the SDK's
  handles/dispatch unit tests.
- **B-43 / E-44** — reflective gadgets are Ruby surface with no Rust
  counterpart, so no stub can express a gadget return; the refusal is
  pinned by `test/transport/test_dispatcher_gadget_return.rb` and
  `test/catalog/test_handles.rb`.

## Out of the manifest

- **Language surface** — setup-time validation (`ArgumentError` /
  `TypeError` shapes), host pre-flight refusals, `Kobako::Pool`, option
  readers, construction failures (E-16..E-18, E-24, E-25, E-29, E-30,
  E-33..E-35, E-39..E-42, E-45..E-47, E-49, E-51..E-53, B-05,
  B-07..B-11, B-19, B-22, B-33's exception class, B-40, B-46..B-48,
  B-54, B-57): each frontend spells these in its own idiom; the seal's
  *timing* (B-33) stays in the manifest, its spelling does not. The
  Extension install composition and its backend provider resolution
  (B-55 / B-56) are in the manifest — a differential install scenario
  runs them through both frontends — while the dependency assertion
  (B-57) and the install-error shapes (E-51..E-53) stay per-frontend. The hermetic family does
  exercise the successful profile *switch* (B-54) — a requested posture
  resolves identically on both frontends — leaving only its
  floor-refusal spelling per-frontend.
- **Guest-internal** — behavior the shared Guest Binary fixes
  regardless of frontend (B-15, B-36, B-38, B-39, B-41, B-44, B-51,
  B-52, B-53): pinned by the guest E2E suites and the codec oracles.
- **Hard-to-trigger wire corners** — comparable in principle but with
  no deterministic trigger through the real guest (B-21, E-02, E-03,
  E-07..E-10, E-26, E-31, E-50): revisit if a legitimate trigger
  appears; parallel fixture guests stay off the table.
- **Retired** — E-14 (N-8: reserved and never reassigned) names no
  behavior, so it neither appears in a scenario nor needs one.
