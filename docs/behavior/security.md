# Security — capability confinement

What the guest can and cannot reach: proxy probing and construction, the regexp compute capability, reflection / eval rejection, ambient-nondeterminism denial, and guest-surface narrowing. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

## B-36 — Guest probes a Member or Handle with `respond_to?`

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The guest holds a Member constant `<Namespace>::<Member>` (B-08) or a `Kobako::Handle` instance obtained from a prior dispatch (B-14) or `#run` auto-wrap (B-34). |
| **Operation** | Guest code calls `<Namespace>::<Member>.respond_to?(:any_name)` on the Member constant, or `handle.respond_to?(:any_name)` on the Handle instance, for a name the proxy does not define locally. |
| **Result / Final State** | `respond_to?` returns `true` for every such probe, on both the Member constant and the Handle instance. The probe is answered entirely inside the guest — no Transport Request is sent. A following method call dispatches normally (B-12 for a Member, B-17 for a Handle). |
| **Notes** | Every method call on a Member or Handle is forwarded to the host, so `respond_to?` answers `true` to stay consistent; the answer is optimistic, not authoritative — it does not consult the host and does not confirm the bound object implements the method. An unimplemented method surfaces at dispatch as `type="runtime"` (E-11), distinct from the unresolvable-target `type="undefined"` (E-12 / E-13). Names the proxy defines locally resolve through their own methods and never reach this path. |

---

## B-38 — Guest attempts to construct a Member proxy

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The guest holds a Member constant `<Namespace>::<Member>` (B-08) whose Host App binding is an object the guest reaches by calling methods on the constant (B-12). |
| **Operation** | Guest code calls `<Namespace>::<Member>.new(...)` or `<Namespace>::<Member>.allocate` — any attempt to instantiate the Member constant. |
| **Result / Final State** | The call raises `NoMethodError` inside the guest. No instance is produced and no Transport Request is sent — the construction attempt never reaches the host. When the guest does not rescue it, the exception reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to any other uncaught guest exception. |
| **Notes** | A Member is a dispatch target, not a constructible type; blocking `new` and `allocate` — the two construction entries mruby exposes — keeps the proxy surface to dispatch only and fails fast. The proxy defines both locally as raising methods, so B-36's optimistic `respond_to?` answer does not apply to them. B-39 blocks the `Kobako::Handle` proxy through the same entries; the two proxies differ only in that a Handle is still constructed internally by the wire decoder (B-14 / B-34) through a path that bypasses the blocked Ruby entries. |

---

## B-39 — Guest attempts to construct a Handle proxy

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The `Kobako::Handle` proxy class is reachable as a named constant, but the guest has not received a Handle from a Service return (B-14) or a `#run` argument auto-wrap (B-34). |
| **Operation** | Guest code calls `Kobako::Handle.new(...)` or `Kobako::Handle.allocate` — any attempt to fabricate a Handle proxy, with or without an integer ID argument. |
| **Result / Final State** | The call raises `NoMethodError` inside the guest. No proxy is produced and no Handle ID is bound. When the guest does not rescue it, the exception reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to any other uncaught guest exception. |
| **Notes** | This is the guest-side enforcement of B-20; the Host App side is enforced separately at host pre-flight (E-29). Both entries raise regardless of passed arguments rather than tripping an arity check first. The wire decoder's restoration path (B-14 / B-34) constructs a Handle through an internal instance-allocation path that does not dispatch the blocked Ruby entries. Parallels B-38. |

---

## B-41 — Guest regexp matching as a compute capability

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The Guest Binary provides `Regexp` and `MatchData` as guest-visible Ruby classes — a regexp literal (`/.../`), `Regexp.new` / `Regexp.compile`, and the `String` integration methods — that compile and run patterns against `String` values. Neither class is among the 12 wire types (→ [`docs/wire-codec.md`](../wire-codec.md) § Type Mapping). |
| **Operation** | Guest code compiles a pattern and matches it against a `String`, then uses the result inside the invocation — a `MatchData`, an Integer match index, `nil` for no match, captured substrings (positional or by name), or the refreshed match backref globals. |
| **Result / Final State** | Matching runs entirely inside the Guest Binary; `Regexp` and `MatchData` are guest-internal and never cross the wire. A value the guest hands back to host code reduces to a wire type first — a captured substring (`str`), a match index (`int`), a capture list (`array`), a `named_captures` map (`map`), absent-match `nil`, or a `Symbol`; a bare `Regexp` or `MatchData` in a returned position is a non-wire value governed by the ordinary return-value semantics (B-06). A pattern that fails to compile raises `RegexpError` inside the guest; uncaught, it is attributed as `Kobako::SandboxError` per E-04. |
| **Notes** | Regexp is a Rust capability gem composed into the Guest Binary shell, the pure-compute peer of the IO / Kernel surface (B-04); like every guest stdlib capability it carries no per-feature wire contract — the closed 12-entry wire type set already excludes `Regexp` / `MatchData`, so projecting a result to wire types is structural, not a new envelope. Coverage is a curated subset of the CRuby `Regexp` / `MatchData` API, byte-based throughout, following MRI within that subset except where a per-behavior contract states otherwise. The full surface and the per-behavior contracts (anchored `RX-xx`) live in [`docs/regexp.md`](../regexp.md). |

---

## B-42 — Host rejects ambient reflection / eval methods at guest→host dispatch

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code dispatches to a target resolved through `Catalog::Namespaces` or `Catalog::Handles` (B-12, B-16, B-17). The target is a bound Service object — a plain object, a `Proc` / lambda bound as a Member, or a host object received earlier as a Capability Handle. |
| **Operation** | Guest code invokes a method whose name resolves, on the target, to Ruby's ambient reflection / metaprogramming surface — the `send` family, `eval` / `instance_eval` / `instance_exec` / `class_eval` / `module_eval`, `binding`, `method` / `public_method`, `define_method`, `const_get` / `const_set`, `instance_variable_get` / `instance_variable_set`, or any other method whose resolved owner is a core meta module (`BasicObject`, `Kernel`, `Object`, `Module`, `Class`) or a callable gadget type (`Proc`, `Method`, `UnboundMethod`, `Binding`). |
| **Result / Final State** | The host rejects the call before it reaches the target: the dispatch returns error `type="undefined"` and no method runs on the host. Only methods the bound object itself exposes as Service behaviour are reachable. For a `Proc` or `Method` target the callable allowlist — `call`, `[]`, `yield`, `arity`, `lambda?` — is the sole exception: these invoke or describe the callable and stay reachable, while every other `Proc` / `Method` / `UnboundMethod` / `Binding` method (notably `Proc#binding`, `Method#receiver`, `Method#unbind`, `Binding#eval`) is rejected. Unrescued in the guest, the rejection reaches the Host App as `Kobako::ServiceError` per E-43. |
| **Notes** | This is the host-authoritative enforcement point: the decision rests on the resolved method's owner, so it holds regardless of what the guest sends — a guest that forges a dispatch Request directly is bound identically to one going through the guest proxy (B-44 is the non-authoritative guest-side mirror). The contract is least-privilege: a bound object's reachable surface is its own Service methods plus, for callables, the invocation allowlist. A method name with no concrete public method on the target is allowed only when the target opts in via `respond_to?` (dynamic `method_missing` Services), since the ambient methods above are all concretely defined and never reach that branch. Reflective objects never become reachable Handles in the first place — B-43 keeps `Binding` / `Method` / `UnboundMethod` off the wire entirely. |

---

## B-43 — Reflective gadget objects are not wire-representable

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Service dispatch (B-12, B-16, B-17) returns a `Binding`, `Method`, or `UnboundMethod` instance. |
| **Operation** | The host wire layer evaluates the return value for wire representation and Capability Handle wrapping (B-13, B-14). |
| **Result / Final State** | The host refuses to mint a Capability Handle for a `Binding`, `Method`, or `UnboundMethod`: these types are neither wire-representable nor Handle-wrappable. The dispatch reports error `type="runtime"` (E-44) instead of returning a Handle, so the guest never receives a callable proxy onto a host reflection object. A gadget nested inside an otherwise non-representable Array / Hash rides back inside that container's Handle and is refused the same way when the guest extracts it (the extraction is itself a B-12 dispatch return). A `Proc` is excluded — it stays Handle-wrappable, and any reflective method on the resulting Handle (e.g. `#binding`) is rejected by B-42. |
| **Notes** | Defense in depth behind B-42, closing the second hop of a `Proc#binding` → `Binding#eval` escalation: even were a reflective method reachable, its gadget result cannot cross back as a Handle. The three blocked types are the ones whose reachable surface is wholly reflection (`Binding#eval`, `Method#receiver` / `#unbind`, `UnboundMethod#bind`); a `Proc`'s legitimate use is invocation (B-42 callable allowlist), so it stays wrappable. The refusal lives at the single Handle mint point (`Catalog::Handles`), so it holds on both wrap paths: the Service-return path reports `type="runtime"` (E-44), and the `#run` host→guest auto-wrap (B-34) refuses a gadget argument as a host-side `Kobako::SandboxError` before the invocation runs, the same surface as the B-21 cap failure during auto-wrap (E-07). |

---

## B-44 — Guest proxy mirrors the reflection rejection

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code holds a Member constant (B-08) or a Capability Handle (B-14); the guest proxy forwards method calls to the host via the Transport (B-12). |
| **Operation** | Guest code invokes a reflection / eval method by name on a Member or Handle proxy — a name in the recognized denylist: the `send` family (`send`, `__send__`, `public_send`), `eval` / `instance_eval` / `instance_exec` / `class_eval` / `module_eval`, `binding`, `method` / `public_method` / `instance_method`, `define_method` / `define_singleton_method`, `const_get` / `const_set`, `instance_variable_get` / `instance_variable_set`, `singleton_class`, and the non-allowlisted callable methods `curry` / `to_proc` / `receiver` / `unbind`. |
| **Result / Final State** | The guest proxy rejects the call before emitting a Transport Request: it raises `NoMethodError` inside the guest and sends no wire Request. Unrescued, the exception reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to any other uncaught guest exception. The callable allowlist (B-42) is preserved — `call`, `[]`, `yield`, `arity`, `lambda?` forward normally. |
| **Notes** | This is the guest-side mirror of B-42, parallel to how B-39 mirrors the host's Handle-forgery enforcement (E-29): opacity and fail-fast UX, not the security boundary. B-42 is authoritative and complete — it decides on the resolved method's owner, so a name the guest denylist misses, a guest binary that bypasses this proxy, or a forged Request is still rejected host-side. The guest enforces on method name because it cannot resolve the bound object's method owner; its denylist is therefore a best-effort recognized set, not the exhaustive contract. |

---

## B-45 — Guest ambient time and randomness are denied (deterministic guest execution)

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The Guest Binary links no mrbgem exposing time, sleep, or randomness (the strict allowlist, → [`SPEC.md`](../../SPEC.md) Goals), and reaches the host only through injected Services (B-08) and the stdout / stderr write surface (B-04). |
| **Operation** | Guest code — or a Rust capability gem linked into the Guest Binary — reaches for ambient wall-clock time or entropy through the WASI layer (`wasi:clocks/wall-clock`, `wasi:clocks/monotonic-clock`, or `wasi:random`), whether via libc (`time`, `gettimeofday`, `getrandom`) or a Rust `SystemTime` / `Instant` / RNG. |
| **Result / Final State** | The host denies every ambient source: `wasi:clocks` reads the Unix epoch and never advances, and `wasi:random` yields a constant byte stream. Guest code observes no real wall-clock time and no host entropy through any ambient path; the only time or randomness available to it is a value a Service injects (B-12) or a snippet embeds (B-32). Given identical source, snippets, and Service responses, guest execution is reproducible. |
| **Notes** | The denial is a property of the host's WASI context, layered behind the mrbgem allowlist: a future Guest Binary gem that reaches libc time or randomness obtains the frozen, deterministic values rather than ambient ones, so the no-ambient-nondeterminism guarantee does not rest on the allowlist alone. The per-invocation wall-clock `timeout` (B-01) is unaffected — it is measured on the host clock and enforced by the engine, never the guest's frozen `wasi:clocks`. A Host App that needs real time or randomness inside the guest injects it explicitly as a Service value, the same mediation every host capability takes. |

---

## B-50 — Host object narrows its guest-reachable method surface

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code dispatches to a target resolved through `Catalog::Namespaces` or `Catalog::Handles` (B-12, B-16, B-17): a bound Service Member, or a host object received earlier as a Capability Handle. The target's class either defines the private predicate `respond_to_guest?(name)` — answering, for a method name delivered as a Symbol, whether the guest may call it — or does not. |
| **Operation** | Guest code invokes a method by name on the target. |
| **Result / Final State** | A target that defines `respond_to_guest?` has the predicate consulted before the call reaches the method: a falsy answer for the invoked name rejects the dispatch with error `type="undefined"` and no method runs on the host; a truthy answer leaves the call subject to the B-42 floor alone, as does a target that does not define the predicate. The predicate composes with the floor by conjunction and only narrows — a name the floor rejects (the reflection / eval surface) stays rejected whatever the predicate answers. A target whose predicate is falsy for every name is opaque: the guest holds it, passes it as a dispatch argument (B-16), and returns it across the boundary (B-37), yet can call nothing on it; a predicate truthy for a chosen subset is an allow-list. Unrescued in the guest, the rejection reaches the Host App as `Kobako::ServiceError` per E-48. |
| **Notes** | The predicate is opt-in least-privilege: the default reachable surface is unchanged (the B-42 floor alone), and a bound object restricts its own guest-facing surface without the Host App hand-building a wrapper that exposes nothing. `respond_to_guest?` answers permission, not existence: a truthy answer permits a name, it does not conjure a method. A permitted name that resolves to a running method which then fails — a dynamic `method_missing` Service that cannot satisfy it — surfaces as `type="runtime"` (E-11); a permitted name with no method to run at all is still rejected `type="undefined"` by the B-42 floor before the predicate is reached. The `type="undefined"` rejection matches the unresolved-target and reflection-rejection surface (E-12 / E-13 / E-43), so an opaque target discloses nothing about which methods it defines. A predicate defined private is honored, yet stays unreachable to the guest: guest dispatch is routed through `public_send`, which never reaches a private `respond_to_guest?` itself. The decision is host-authoritative and rests on the resolved target, binding Member and Handle targets identically; it is the host-side complement of B-36, where the guest's `respond_to?` answers optimistically inside the guest without consulting the host. |
