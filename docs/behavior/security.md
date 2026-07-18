# Security — capability confinement

What the guest can and cannot reach: proxy probing and construction, the regexp and JSON compute capabilities, reflection / eval rejection, ambient-nondeterminism denial, guest-surface narrowing, and the runtime isolation-profile floor. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

## B-36 — Guest probes a bound constant or Handle with `respond_to?`

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The guest holds a bound-Service constant `MyService::KV` (B-08) or a `Kobako::Handle` instance obtained from a prior dispatch (B-14) or `#run` auto-wrap (B-34). |
| **Operation** | Guest code calls `MyService::KV.respond_to?(:any_name)` on the bound constant, or `handle.respond_to?(:any_name)` on the Handle instance, for a name the proxy does not define locally. |
| **Result / Final State** | `respond_to?` returns `true` for every such probe, on both the bound constant and the Handle instance. The probe is answered entirely inside the guest — no Transport Request is sent. A following method call dispatches normally (B-12 for a bound constant, B-17 for a Handle). The answer is optimistic, not authoritative — `respond_to?` returns `true` because every call on a bound-constant or Handle proxy is forwarded to the host, but it does not consult the host and does not confirm the bound object implements the method. A method the bound object does not expose is rejected host-side as `type="undefined"` — the same opaque rejection as an unresolvable target (E-12 / E-13), so the target discloses nothing about which methods it defines (B-42); a method it exposes that then raises surfaces as `type="runtime"` (E-11). Names the proxy defines locally resolve through their own methods and never reach this path. |

---

## B-38 — Guest constructs a bound-Service proxy

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The guest holds a bound-Service constant `MyService::KV` (B-08) whose Host App binding is an object the guest reaches by calling methods on the constant (B-12). |
| **Operation** | Guest code calls `MyService::KV.new(...)` or `MyService::KV.allocate` on the bound constant. |
| **Result / Final State** | The call resolves through mruby's ordinary `Class#new` / `Class#allocate` and yields a plain instance of the bound constant. That instance carries no dispatch: the constant `extend`s `Kobako::Proxy`, so the forwarding seam lives on the constant at class level and never on an instance — a method call on the produced object finds no seam and raises an ordinary `NoMethodError`. The construction sends no Transport Request. Construction is neither blocked nor a capability gate: the object it yields grants nothing, and what any real dispatch reaches is confined by the host's path resolution (E-12), not by whether construction was possible. B-59 governs the target a dispatch derives from its receiver; B-39 governs the same construction question for the `Kobako::Handle` proxy. |

---

## B-39 — Guest constructs a Handle proxy

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The `Kobako::Handle` proxy class is reachable as a named constant. |
| **Operation** | Guest code calls `Kobako::Handle.new(...)` or `Kobako::Handle.allocate`, with or without an integer ID argument. |
| **Result / Final State** | Both entries raise `NoMethodError` in the guest, so an exact `Kobako::Handle` instance arises only from the wire decoder's internal allocation on the return path (B-14 / B-34), which bypasses these Ruby entries. This keeps the receiver-identity check (B-59) sound: a `Kobako::Handle` proxy is always host-issued, never guest-fabricated. Blocking construction is opacity, not the capability gate — the host's per-invocation `Catalog::Handles` membership is the authoritative boundary, and however the guest presents an id (a received Handle, or one re-pointed through reflective mutation of a held Handle), an id the invocation was not handed is rejected `type="undefined"` (E-13); ids are a sequential per-invocation counter the design never treats as secret, and a presented id grants no reference the invocation was not already handed (B-18). The Host App side is enforced separately at host pre-flight (E-29). |

---

## B-59 — Guest→host dispatch derives its target from the receiver's identity

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. A dispatch reaches the `Kobako::Proxy` `method_missing` seam — from a bound-Service constant that `extend`s the module, from a `Kobako::Handle` instance that `include`s it, or from a guest's own class or module that has mixed the module in. |
| **Operation** | Guest code invokes a name the receiver does not define locally, reaching the shared forwarding seam. |
| **Result / Final State** | The seam derives the Transport Request's target from the receiver's exact identity: an exact `Kobako::Handle` instance yields a Handle target from its stored id; a class receiver yields a constant-path target from its name. Any other receiver — a subclass of `Kobako::Handle`, or a guest object or module that mixed in the seam without being a Handle — has no target and is refused inside the guest with `NoMethodError`, sending no Transport Request. Because guest construction of `Kobako::Handle` is blocked (B-39), an exact `Kobako::Handle` is always host-issued; a guest thus cannot drive a Handle-targeted dispatch off arbitrary instance state by fabricating or subclassing a proxy holder. Deriving the target from exact identity is guest-side opacity; every target that does reach the wire remains subject to the host's authoritative resolution — path resolution (E-12) for a constant target, `Catalog::Handles` membership (E-13) for a Handle target. |

---

## B-60 — A guest-held Handle is immutable

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code that holds a `Kobako::Handle` from a Service return (B-14) or a `#run` auto-wrap (B-34). |
| **Operation** | Guest code attempts to re-point the Handle's id to a value the invocation was not handed — `handle.instance_variable_set(:@__kobako_id__, n)`, an `instance_eval` assignment to the ivar, or a mutation of a `dup` / `clone` copy. |
| **Result / Final State** | The decoder freezes every Handle it mints, so the id ivar cannot be reassigned: `instance_variable_set` and an `instance_eval` assignment raise `FrozenError`, and `dup` / `clone` yield frozen copies — each copy keeps the original's id and stays a valid Handle to the same object. With blocked construction (B-39) and the exact-identity target check (B-59), the guest can present only ids of Handles it actually holds; it cannot construct, forge, guess, or probe an id, so it cannot emit a dispatch carrying an id it was not handed. This is defense-in-depth that makes id unforgeability in-guest-enforced rather than only host-gated; the host's per-invocation `Catalog::Handles` membership remains the authoritative gate (E-13). Freezing does not weaken dispatch — the seam only reads the id — so a frozen Handle forwards its calls normally. |

---

## B-41 — Guest regexp matching as a compute capability

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code. The Guest Binary provides `Regexp` and `MatchData` as guest-visible Ruby classes — a regexp literal (`/.../`), `Regexp.new` / `Regexp.compile`, and the `String` integration methods — that compile and run patterns against `String` values. Neither class is among the 12 wire types (→ [`docs/wire-codec.md`](../wire-codec.md) § Type Mapping). |
| **Operation** | Guest code compiles a pattern and matches it against a `String`, then uses the result inside the invocation — a `MatchData`, an Integer match index, `nil` for no match, captured substrings (positional or by name), or the refreshed match backref globals. |
| **Result / Final State** | Matching runs entirely inside the Guest Binary; `Regexp` and `MatchData` are guest-internal and never cross the wire. A value the guest hands back to host code reduces to a wire type first — a captured substring (`str`), a match index (`int`), a capture list (`array`), a `named_captures` map (`map`), absent-match `nil`, or a `Symbol`; a bare `Regexp` or `MatchData` in a returned position is a non-wire value governed by the ordinary return-value semantics (B-06). A pattern that fails to compile raises `RegexpError` inside the guest; uncaught, it is attributed as `Kobako::SandboxError` per E-04. Coverage is a curated subset of the CRuby `Regexp` / `MatchData` API, byte-based throughout, following MRI within that subset except where a per-behavior contract states otherwise; the full surface and the per-behavior contracts (anchored `RX-xx`) live in [`docs/regexp.md`](../regexp.md). |

---

## B-42 — Host rejects ambient reflection / eval methods at guest→host dispatch

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code dispatches to a target resolved through `Catalog::Services` or `Catalog::Handles` (B-12, B-16, B-17). The target is a bound Service object — a plain object, a `Proc` / lambda bound as a Service, or a host object received earlier as a Capability Handle. |
| **Operation** | Guest code invokes a method whose name resolves, on the target, to Ruby's ambient reflection / metaprogramming surface — the `send` family, `eval` / `instance_eval` / `instance_exec` / `class_eval` / `module_eval`, `binding`, `method` / `public_method`, `define_method`, `const_get` / `const_set`, `instance_variable_get` / `instance_variable_set`, or any other method whose resolved owner is a core meta module (`BasicObject`, `Kernel`, `Object`, `Module`, `Class`) or a callable gadget type (`Proc`, `Method`, `UnboundMethod`, `Binding`). |
| **Result / Final State** | The host rejects the call before it reaches the target: the dispatch returns error `type="undefined"` and no method runs on the host. Only methods the bound object itself exposes as Service behaviour are reachable. For a `Proc` or `Method` target the callable allowlist — `call`, `[]`, `yield`, `arity`, `lambda?` — is the sole exception: these invoke or describe the callable and stay reachable, while every other `Proc` / `Method` / `UnboundMethod` / `Binding` method (notably `Proc#binding`, `Method#receiver`, `Method#unbind`, `Binding#eval`) is rejected. Unrescued in the guest, the rejection reaches the Host App as `Kobako::ServiceError` per E-43. This is the host-authoritative enforcement point: the decision rests on the resolved method's owner, so it holds regardless of what the guest sends — a guest that forges a dispatch Request directly is bound identically to one going through the guest proxy (B-44 is the non-authoritative guest-side mirror). The contract is least-privilege: a bound object's reachable surface is its own Service methods plus, for callables, the invocation allowlist. A method name with no concrete public method on the target is allowed only when the target opts in via `respond_to?` (dynamic `method_missing` Services), since the ambient methods above are all concretely defined and never reach that branch. Reflective objects never become reachable Handles in the first place — B-43 keeps `Binding` / `Method` / `UnboundMethod` off the wire entirely. |

---

## B-43 — Reflective gadget objects are not wire-representable

| Field | Value |
|-------|-------|
| **Initial State** | A guest-initiated Service dispatch (B-12, B-16, B-17) returns a `Binding`, `Method`, or `UnboundMethod` instance. |
| **Operation** | The host wire layer evaluates the return value for wire representation and Capability Handle wrapping (B-13, B-14). |
| **Result / Final State** | The host refuses to mint a Capability Handle for a `Binding`, `Method`, or `UnboundMethod`: these types are neither wire-representable nor Handle-wrappable. The dispatch reports error `type="runtime"` (E-44) instead of returning a Handle, so the guest never receives a callable proxy onto a host reflection object. A gadget nested inside an otherwise non-representable Array / Hash rides back inside that container's Handle and is refused the same way when the guest extracts it (the extraction is itself a B-12 dispatch return). A `Proc` is excluded — it stays Handle-wrappable, and any reflective method on the resulting Handle (e.g. `#binding`) is rejected by B-42. This is defense in depth behind B-42, closing the second hop of a `Proc#binding` → `Binding#eval` escalation: the three blocked types are the ones whose reachable surface is wholly reflection, while a `Proc`'s legitimate use is invocation (B-42 callable allowlist), so it stays wrappable. The refusal lives at the single Handle mint point (`Catalog::Handles`), so it holds on both wrap paths: the Service-return path reports `type="runtime"` (E-44), and the `#run` host→guest auto-wrap (B-34) refuses a gadget argument as a host-side `Kobako::SandboxError` before the invocation runs, the same surface as the B-21 cap failure during auto-wrap (E-07). |

---

## B-44 — Guest proxy mirrors the reflection rejection

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code holds a bound-Service constant (B-08) or a Capability Handle (B-14); the guest proxy forwards method calls to the host via the Transport (B-12). |
| **Operation** | Guest code invokes a reflection / eval method by name on a bound-constant or Handle proxy — a name in the recognized denylist: the `send` family (`send`, `__send__`, `public_send`), `eval` / `instance_eval` / `instance_exec` / `class_eval` / `module_eval`, `binding`, `method` / `public_method` / `instance_method`, `define_method` / `define_singleton_method`, `const_get` / `const_set`, `instance_variable_get` / `instance_variable_set`, `singleton_class`, and the non-allowlisted callable methods `curry` / `to_proc` / `receiver` / `unbind`. |
| **Result / Final State** | The guest proxy rejects the call before emitting a Transport Request: it raises `NoMethodError` inside the guest and sends no wire Request. Unrescued, the exception reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to any other uncaught guest exception. The callable allowlist (B-42) is preserved — `call`, `[]`, `yield`, `arity`, `lambda?` forward normally. This is the guest-side mirror of B-42 — opacity and fail-fast UX, not the security boundary, parallel to how B-39 mirrors the host's Handle-forgery enforcement (E-29). B-42 is authoritative and complete: it decides on the resolved method's owner, so a name the guest denylist misses, a guest binary that bypasses this proxy, or a forged Request is still rejected host-side. The guest enforces on method name because it cannot resolve the bound object's method owner; its denylist is a best-effort recognized set, not the exhaustive contract. |

---

## B-45 — Guest ambient time and randomness are denied under the hermetic profile (deterministic guest execution)

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code under the `hermetic` isolation profile — the default rung (B-54). The Guest Binary links no mrbgem exposing time, sleep, or randomness (the strict allowlist, → [`SPEC.md`](../../SPEC.md) Goals), and reaches the host only through injected Services (B-08) and the stdout / stderr write surface (B-04). |
| **Operation** | Guest code — or a Rust capability gem linked into the Guest Binary — reaches for ambient wall-clock time or entropy through the WASI layer (`wasi:clocks/wall-clock`, `wasi:clocks/monotonic-clock`, or `wasi:random`), whether via libc (`time`, `gettimeofday`, `getrandom`) or a Rust `SystemTime` / `Instant` / RNG. |
| **Result / Final State** | The host denies every ambient source: `wasi:clocks` reads the Unix epoch and never advances, and `wasi:random` yields a constant byte stream. Guest code observes no real wall-clock time and no host entropy through any ambient path; the only time or randomness available to it is a value a Service injects (B-12) or a snippet embeds (B-32). Given identical source, snippets, and Service responses, guest execution is reproducible. The denial is a property of the host's WASI context, layered behind the mrbgem allowlist: a future Guest Binary gem that reaches libc time or randomness obtains the frozen, deterministic values rather than ambient ones, so the no-ambient-nondeterminism guarantee does not rest on the allowlist alone. The per-invocation wall-clock `timeout` (B-01) is unaffected — it is measured on the host clock and enforced by the engine, never the guest's frozen `wasi:clocks`. This denial is the grant that separates the profile rungs: under `permissive` (B-54) the same WASI sources read the host's live clocks and entropy, and the reproducibility guarantee does not hold for that Sandbox — every other clause here, the allowlist layering and the host-clock `timeout` included, holds on both rungs. A Host App that needs real time or randomness inside a hermetic guest injects it explicitly as a Service value, the same mediation every host capability takes; one that accepts ambient nondeterminism requests `permissive` instead. |

---

## B-50 — Host object narrows its guest-reachable method surface

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code dispatches to a target resolved through `Catalog::Services` or `Catalog::Handles` (B-12, B-16, B-17): a bound Service, or a host object received earlier as a Capability Handle. The target's class either defines the private predicate `respond_to_guest?(name)` — answering, for a method name delivered as a Symbol, whether the guest may call it — or does not. |
| **Operation** | Guest code invokes a method by name on the target. |
| **Result / Final State** | A target that defines `respond_to_guest?` has the predicate consulted before the call reaches the method: a falsy answer for the invoked name rejects the dispatch with error `type="undefined"` and no method runs on the host; a truthy answer leaves the call subject to the B-42 floor alone, as does a target that does not define the predicate. The predicate composes with the floor by conjunction and only narrows — a name the floor rejects (the reflection / eval surface) stays rejected whatever the predicate answers. A target whose predicate is falsy for every name is opaque: the guest holds it, passes it as a dispatch argument (B-16), and returns it across the boundary (B-37), yet can call nothing on it; a predicate truthy for a chosen subset is an allow-list. Unrescued in the guest, the rejection reaches the Host App as `Kobako::ServiceError` per E-48. The predicate is opt-in least-privilege: the default reachable surface is unchanged (the B-42 floor alone), and a bound object restricts its own guest-facing surface without the Host App hand-building a wrapper. `respond_to_guest?` answers permission, not existence — a truthy answer permits a name, it does not conjure a method: a permitted name that resolves to a running method which then fails surfaces as `type="runtime"` (E-11), while a permitted name with no method to run at all is still rejected `type="undefined"` by the B-42 floor before the predicate is reached. That `type="undefined"` rejection matches the unresolved-target and reflection surfaces (E-12 / E-13 / E-43), so an opaque target discloses nothing about which methods it defines. A predicate defined private is honored yet stays unreachable to the guest, since guest dispatch routes through `public_send`. The decision is host-authoritative and rests on the resolved target, binding bound-constant and Handle targets identically — the host-side complement of B-36's optimistic guest-side `respond_to?`. |

---

## B-51 — Capability gem propagates a guest callback's raise as a guest exception

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code that exercises a capability gem — the IO / Kernel write surface (B-04: `$stdout.puts` / `p` / `print` / `IO#write`) or the regexp compute capability (B-41: `String#gsub` / `sub` and the other integrations). To service the operation, the gem invokes a guest-supplied method on a guest value: an argument's `to_s` / `inspect` for output coercion, a replacement block or hash's `[]` for substitution, or any guest method the gem dispatches. |
| **Operation** | The guest-supplied method the gem invokes raises an exception — a `RuntimeError` from a user-defined `to_s`, a `NoMethodError`, or any other guest-level raise — instead of returning a value. |
| **Result / Final State** | The raise propagates as an ordinary guest exception: it surfaces in the enclosing guest frame, where the guest may `rescue` it like any other exception. Unrescued, it reaches the invocation top level and is attributed as `Kobako::SandboxError` per E-04, identical to a raise originating directly in guest script — never a Wasm trap (E-01). A guest method invoked across a capability gem's host-language boundary cannot, by raising, corrupt the guest execution environment or retire the Sandbox. This holds for every capability gem and every guest method it dispatches — `to_s` / `inspect` coercion on the IO write surface, the replacement block or hash `[]` of a regexp substitution, a JSON `as_json` hook (B-53), and any future capability that calls back into guest code — so untrusted guest code cannot escalate a raising conversion method into a sandbox-level fault. Block results that cross the host yield boundary stay governed by B-24 (E-21..E-23); B-51 governs the guest-internal callbacks a gem makes while servicing one guest operation. |

---

## B-52 — Guest JSON parse and generate as a compute capability

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing mruby guest code in a JSON-capable variant (→ [`docs/variants.md`](../variants.md)) whose Guest Binary links the JSON capability gem. The Guest Binary provides a `JSON` module with `parse`, `generate`, and `pretty_generate`, plus the guest-visible error classes `JSON::JSONError` and its subclasses `JSON::ParserError` / `JSON::GeneratorError`. None of these are among the 12 wire types (→ [`docs/wire-codec.md`](../wire-codec.md) § Type Mapping). |
| **Operation** | Guest code parses an untrusted JSON string into mruby values, or generates a JSON string from mruby values, then uses the result inside the invocation. |
| **Result / Final State** | Parsing and generation run entirely inside the Guest Binary; the `JSON` module is guest-internal and never crosses the wire. `parse` yields only JSON-native mruby values — `nil`, the booleans, `Integer` or `Float` (per the integer-range policy), `String`, `Array`, and `Hash` keyed by `String` (or `Symbol` under `symbolize_names:`) — preserving object member order; nesting beyond the depth bound, and an integer too large to represent without precision loss, raise `JSON::ParserError`. `generate` emits well-formed JSON for JSON-native values and refuses any other object (B-53). Malformed input raises `JSON::ParserError` inside the guest; uncaught, it is attributed as `Kobako::SandboxError` per E-04. A value the guest hands back to host code is an ordinary mruby value governed by the return-value semantics (B-06); JSON adds no wire type. Coverage is a curated subset of the CRuby `JSON` API following MRI within that subset except where a per-behavior contract states otherwise; the full surface and the per-behavior contracts (anchored `JS-xx`) live in [`docs/json.md`](../json.md). |

---

## B-53 — JSON capability-reference boundary

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox executing guest code in a JSON-capable variant. The guest may hold a `Kobako::Handle` (B-14 / B-34) or a bound-Service constant (B-08) — proxies whose method calls forward to the host (B-12). |
| **Operation** | The guest passes arbitrary input to `JSON.parse`, or passes to `JSON.generate` / `pretty_generate` a value that may be a Handle, a bound constant, or any non-JSON-native object. |
| **Result / Final State** | **Inbound:** no JSON syntax decodes to a capability — `parse` produces only JSON-native values, so guest code cannot fabricate a `Kobako::Handle` or any host capability through parsing. This non-forgeability is contractual, not incidental. **Outbound:** the generator encodes JSON-native values — and a `Symbol`, by its name — directly and reaches every other object only through `as_json`, a method defined on `Object` whose default raises `JSON::GeneratorError`. A `Handle`, a bound constant, or any object that has not opted in therefore fails loud rather than serializing — and because the default is a *defined* method on the root class, the call resolves to it and never reaches `method_missing`, so it never forwards to the host. The generator classifies JSON-native types by exact identity and never probes a value with `respond_to?` or a conversion protocol (`to_ary` / `to_hash` / a bare `to_json`): a proxy answers `respond_to?` optimistically (B-36) and an undefined conversion method forwards to the host (B-12), so probing would re-open the dispatch path this boundary closes. An object that opts in by defining `as_json` serializes as the value that method returns, re-encoded under the same depth bound and the same refusal — a returned value that itself carries a Handle still raises. Uncaught, the `GeneratorError` is attributed as `Kobako::SandboxError` per E-04. |

---

## B-54 — Sandbox requests an isolation profile; the runtime builds it, declares the built posture, and construction enforces the request as a floor

| Field | Value |
|-------|-------|
| **Initial State** | A Host App constructs a `Kobako::Sandbox` over a runtime implementation. Isolation postures form the ordered ladder `permissive < hermetic`. `hermetic` is the full ambient-denial posture: the WASI-boundary denial of ambient time and entropy (B-45) with no filesystem, environment, or network reachability through the WASI surface, and no host import beyond the wire ABI's closed set (whose single host import is `__kobako_dispatch`) — so the guest's only paths outward are injected Services (B-08) and the captured stdout / stderr write surface (B-04). `permissive` differs from `hermetic` in exactly one grant: the guest's `wasi:clocks` and `wasi:random` read the host's live time and entropy (B-45); filesystem, environment, and network stay unreachable and the host-import set is unchanged. |
| **Operation** | The Host App passes `profile:` to `Kobako::Sandbox.new` — `:permissive` or `:hermetic`, defaulting to `:hermetic` — naming the posture it requests, which is also the weakest posture it accepts. |
| **Result / Final State** | The runtime builds the requested posture and declares the posture it actually built; the bundled wasmtime runtime builds either rung exactly as requested. Construction compares the declaration against the request: a declaration at or above the request constructs normally — a runtime that can only build a stronger posture satisfies a weaker request by building what it has — and a declaration below the request fails with `Kobako::SetupError` before any invocation entry point runs (E-49), mirroring the B-01 / B-40 setup-time treatment, so guest code never runs on a posture weaker than the Host App accepted. A declaration the Host Gem cannot place on the ladder is fail-closed: it ranks below every request and fails construction identically. The default keeps the strongest rung: requesting `:permissive` is the Host App's explicit acceptance of ambient nondeterminism for that Sandbox. The configured request is readable via `Kobako::Sandbox#profile` alongside the cap readers; a `profile:` value outside the ladder — `nil` included, since the weakest posture is requested explicitly as `:permissive` — is rejected at host pre-flight per E-39. |
