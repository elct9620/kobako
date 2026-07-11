# Registration — binding Services at constant-path names

Binding Host objects at constant-path names as the guest-reachable Service
surface. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

## B-07 (retired)

B-07 is a retired anchor — permanently reserved and never reassigned (N-8).

---

## B-08 — Bind a Service at a constant-path name

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance whose first invocation (`#eval` or `#run`) has not yet sealed Service registration (B-33). No Service is bound at `path`. |
| **Operation** | `sandbox.bind(path, object)` where `path` is a Symbol or String of one or more `::`-separated segments, each matching `/\A[A-Z]\w*\z/` (Ruby constant form) — e.g. `"MyService::KV"` or a top-level `"File"` — and `object` is any Ruby object (class, instance, or module) that responds to the methods guest code will invoke. |
| **Result / Final State** | `object` is registered as the Service reachable at `path`. Guest code reaches it through the constant path the segments spell: a multi-segment path nests the leaf constant under a module named by its prefix (`MyService::KV`), a single-segment path binds it at top level (`File`). The bound object handles class, instance, and module receivers identically — dispatch forwards the guest's method call to it without distinguishing the three. The method returns the Sandbox (`self`) to allow chaining. The bound object must remain valid for the Sandbox's lifetime; the Host App manages its lifecycle. A segment that does not match the constant pattern raises `ArgumentError` (E-16). Binding is sealed by the first invocation alongside preload (B-33): after the seal `bind` raises `ArgumentError` (E-45), and every subsequent invocation carries exactly the bindings that existed at sealing. |

---

## B-09 — Multiple Services coexist independently on one Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with one or more Services already bound. |
| **Operation** | `sandbox.bind(other_path, object)` with a `path` distinct from every bound path and colliding with none of them (B-11). |
| **Result / Final State** | The new Service is registered alongside the existing ones. Each Service is reachable only at its own path; paths that share a prefix segment (`MyService::KV`, `MyService::Log`) present that prefix as one shared guest module, while unrelated paths stay independent with no cross-visibility. Services on different Sandbox instances are fully isolated. There is no declared upper limit on the number of Services per Sandbox. |

---

## B-10 (retired)

B-10 is a retired anchor — permanently reserved and never reassigned (N-8).

---

## B-11 — Bind a path that duplicates or collides with an existing binding

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with a Service already bound at `path`. |
| **Operation** | `sandbox.bind(conflicting, object)` where `conflicting` either equals `path` or, on the `::` segment boundary, is an ancestor or descendant of it (e.g. `path` is `"MyService::KV"` and `conflicting` is `"MyService"` or `"MyService::KV::Sub"`). |
| **Result / Final State** | `ArgumentError` is raised. The existing binding is not overwritten and the registry is unchanged. A name is either a bound Service or a prefix that groups other bindings, never both — so a bind is refused when its path equals an existing path, is a prefix of one (`MyService` while `MyService::KV` is bound), or extends one (`MyService::KV::Sub` while `MyService::KV` is bound). Sibling paths under a shared prefix (`MyService::KV` and `MyService::Log`) do not collide. |
