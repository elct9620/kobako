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
binding on both sides).

Capability Handles compare by **identity, not id**: an `opaque` stub
(or `run` argument) is a labeled non-wire host object, and a crossed
object tags as `{"t": "opaque", "label": …}` on both sides — the Ruby
executor reads the label off the restored object, the Rust runner
resolves the result Handle against the Sandbox's table and recovers
the label by object identity. A raw Handle id never appears in an
observable.

The suite rides `rake test`; on a checkout without cargo the families
skip. A family whose SDK seam has not landed yet carries `skip`
entries citing its anchors, so coverage stays visible while the seam
is pending. Three permanent entries share that shape, each pinned
per-frontend instead: E-23 (the SDK's `Block` borrows its dispatch
frame, so the escaped-Yielder misuse is a compile error — Ruby's
runtime refusal lives in `test/e2e/test_yield_unwind.rb`); B-18 / E-13
(one fresh guest instance per invocation means no scenario can present
a stale Handle — staleness is unit-pinned by
`test/transport/test_dispatcher_invalidity.rb` and the SDK's
handles/dispatch unit tests); B-43 / E-44 (reflective gadgets are Ruby
surface with no Rust counterpart — the refusal is pinned by
`test/transport/test_dispatcher_gadget_return.rb`).

## Coverage gate

`rake parity:coverage` cross-checks the manifest below against the
anchors actually cited under `test/parity/` and fails on any CORE
anchor with no scenario or pending entry — the guard that keeps a new
host-observable anchor from landing on one frontend only. (`rake
anchors` separately guarantees every ID below resolves to a real
definition.)

## CORE anchor manifest

Host-observable behaviors both frontends must exhibit identically —
the parity harness's target surface:

```
B-01 B-02 B-03 B-04 B-06 B-12 B-13 B-14 B-16 B-17 B-18 B-20
B-23 B-24 B-25 B-26 B-27 B-28 B-29 B-30 B-31 B-32 B-33 B-34 B-35 B-37
B-42 B-43 B-45 B-49 B-50
E-01 E-04 E-05 E-06 E-11 E-12 E-13 E-15 E-19 E-20 E-21 E-22 E-23
E-27 E-28 E-32 E-36 E-37 E-38 E-43 E-44 E-48
```

## Out of the manifest

- **Language surface** — setup-time validation (`ArgumentError` /
  `TypeError` shapes), `Kobako::Pool`, option readers, construction
  failures (E-39..E-42, E-49, B-05, B-07..B-11, B-19, B-22, B-33's
  exception class, B-40, B-46..B-48, B-54): each frontend spells these
  in its own idiom; the seal's *timing* (B-33) stays in the manifest,
  its spelling does not.
- **Guest-internal** — behavior the shared Guest Binary fixes
  regardless of frontend (B-15, B-36, B-38, B-39, B-41, B-44, B-51,
  B-52, B-53): pinned by the guest E2E suites and the codec oracles.
- **Hard-to-trigger wire corners** — comparable in principle but with
  no deterministic trigger through the real guest (B-21, E-02, E-03,
  E-07..E-10, E-26, E-31): revisit if a legitimate trigger appears;
  parallel fixture guests stay off the table.
