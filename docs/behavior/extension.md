# Extension — installing a guest idiom with an optional host backend

Installing an Extension: a guest idiom (mruby `source`) paired with an
optional host `backend`, composed onto a Sandbox through the existing
`#preload` and `#bind` verbs. The governing summary lives in
[`SPEC.md`](../../SPEC.md) § Behavior; this file is the per-anchor
reference. `B-xx` anchors are global and append-only across the corpus
(N-8).

The mechanism is entirely host-side setup — it adds no wire, codec, or
Guest Binary surface. The guest sees only the preloaded snippet and the
bound Service the composition produces.

---

## B-55 — Install an Extension

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance whose first invocation (`#eval` or `#run`) has not yet sealed registration (B-33). |
| **Operation** | `sandbox.install(*extensions)` — each argument is an Extension, an object exposing `name` (a Symbol matching `/\A[A-Z]\w*\z/`), `source` (a String of mruby source), `backend` (a value exposing a constant `path` and a `provider`, or `nil`), and `depends_on` (an Array of Symbol). `Kobako::Extension` and `Kobako::Extension::Backend` are the bundled value types; `install` duck-types on these readers, so any conforming objects are accepted. |
| **Result / Final State** | For each Extension, its `source` is registered as a preloaded snippet under `name` exactly as `#preload(code: source, name: name)` (B-32), and — when `backend` is present — `backend.path` is bound as a Service exactly as `#bind(backend.path, object)` (B-08), the `object` resolved from `backend.provider` (B-56). `name` is the snippet's canonical name and the `depends_on` match key; it is independent of `backend.path`, so the guest constant the idiom routes to need not equal the Extension's name. `source` is mandatory — an Extension always carries a guest idiom; a host object with no idiom is bound with `#bind` directly (E-53). An Extension carries at most one `backend`; a capability spanning several host-backed constants composes as several Extensions linked by `depends_on`. The method returns the Sandbox (`self`) for chaining. Installation is sealed by the first invocation alongside preload and bind (B-33): after the seal `install` raises `ArgumentError` (E-51). |

---

## B-56 — Resolve an Extension backend's bound object per invocation

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox with one or more installed Extensions carrying a `backend`, whose `provider` is either a fixed object or a callable (a per-invocation source). |
| **Operation** | An invocation (`#eval` or `#run`) begins. |
| **Result / Final State** | Before the guest runs, each installed backend's bound object is resolved from its `provider`: a provider that is not itself callable is the bound object, resolved once and identical across every invocation; a callable provider is invoked once at the start of each invocation to yield that invocation's object, so a fresh object backs the path on every invocation and no writable per-invocation state leaks across invocations (B-03). Callability is the sole discriminator — a fixed backend that is itself callable is not directly expressible and must be supplied through a non-callable wrapper. Provider identity is resource identity — distinct provider values resolve to distinct objects, and one provider value shared by several Extensions resolves once per invocation to a single object shared by their paths. The bound path set is fixed at the seal and its Frame 1 preamble is unchanged across invocations (B-33); only the object behind each path is refreshed. A callable provider that raises propagates its exception unchanged to the `#eval` / `#run` caller — never a wrapped `Kobako` error — and the guest does not run; resolution being per-invocation, a later invocation whose provider succeeds runs normally. |

---

## B-57 — Assert an Extension's declared dependencies are present

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox with one or more installed Extensions, whose first invocation has not yet sealed registration. |
| **Operation** | The first invocation (`#eval` or `#run`) seals the registries. |
| **Result / Final State** | Every installed Extension's `depends_on` names must all be present among the installed Extension `name`s. An unmet dependency raises `ArgumentError` (E-52) at the seal, before the guest runs, naming the missing dependency. The check is presence-only: it neither orders installation nor orders snippet replay, since cross-Extension references resolve at guest call time after every snippet has replayed — so dependency cycles among installed Extensions are permitted. |
