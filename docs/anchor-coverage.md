# Anchor Coverage

Every behavior anchor is a contract, and the tests that cite it are the
contract's witnesses — the test that verifies an anchor names it. This
instrument reads that convention back as data: the per-anchor citation
profile shows which contracts are thickly pinned, which are thin, and
which have no witness at all. The profile is a signal for choosing
where to spend maintenance effort; only the rules under Coverage gate
block.

## Mechanism

`rake anchors:coverage` cross-references two in-repo sources:

- **Definitions** — the same corpus `rake anchors` audits: `B-xx` /
  `E-xx` from `docs/behavior/*.md`, `RX-xx` from
  [`docs/regexp.md`](regexp.md), `JS-xx` from [`docs/json.md`](json.md).
  Retired anchors (tombstones, e.g. `E-14`) are outside the profile —
  a permanently reserved number has no behavior left to witness.
- **Citations** — textual anchor references in `test/**/*.rb`, with the
  tooling suites (`test/tasks/`, `test/bench/`) excluded: their
  anchor-shaped tokens are hand-built fixtures, not witnesses. A
  citation is the naming convention itself (a comment block, an
  assertion message, a parity `anchors:` list), so any textual
  reference counts, and the counting unit is the **citing file** —
  mention counts inflate with witness-table size and are not
  comparable across anchors.

The task prints the actionable ends of the profile: anchors with zero
or one citing file (thin coverage — candidates for a new witness or a
Pending entry), and the most-cited anchors (candidates for duplicate
coverage review). Rust-side unit tests are not scanned; a behavior
pinned only there is recorded under Pending anchors with its pinning
location named.

## Coverage gate

The task fails when either rule breaks; everything else in the report
is signal, not gate:

- a defined anchor has **zero citing files** under `test/` and is not
  listed under Pending anchors;
- a Pending anchor **is cited** by a test — the entry is stale, drop it;
- an anchor listed under **E2E-witnessed anchors** has no citing file
  under `test/e2e/` — its invocation-boundary contract needs a witness
  that drives the real guest, not a unit citation alone.

## E2E-witnessed anchors

An anchor whose contract is observable only across the invocation
boundary — a guest-visible effect, or a raise at the first invocation —
can be cited by a unit test that drives the collaborator in isolation
while no test walks the real `install` / `#eval` path. The citation gate
alone would pass over that unwalked seam, so these anchors require at
least one citing file under `test/e2e/`, the suite that drives
`data/kobako.wasm`:

```
B-55 B-56 E-51 E-52
```

The rule forces an end-to-end witness to exist; whether that witness
walks the whole seam stays a review concern.

## Pending anchors

Anchors whose behavior cannot be reached through the public Ruby
surface, accepted by the gate in place of a citing test; each is pinned
where the behavior is actually verified:

```
E-10 E-26
```

- **E-10** — the official guest never presents an invalid wire payload
  in a dispatch position (`kobako-mruby` only re-emits Handles it
  received), so no `test/` scenario reaches the rejection; the
  guest-side refusal is pinned by the `kobako-codec` Request decode
  unit tests (a Request `target` must be a path or a Handle).
- **E-26** — the official host cannot write a malformed invocation
  envelope through the public API; guest-entry shape validation is
  pinned by the `parse_invocation` unit tests in
  `wasm/kobako-mruby/src/flows/run.rs` (`rake wasm:test`).

## Frontend witness asymmetries

The profile scans `test/**/*.rb` only; the Rust SDK's witnesses under
`crates/**/tests/` are not cross-checked here. Two install-error anchors
are deliberately not held to a matching Rust witness: **E-53** (malformed
Extension shape) is unrepresentable through the Rust SDK's typed
`Extension` / `Backend` API, so it has no Rust witness by construction;
**E-51** (install after the seal) holds on both frontends, but the Rust
side is pinned by a registry-level unit test rather than a
through-the-SDK one.
