# Registration — Namespaces and Members

Declaring Namespaces and binding Members as the guest-reachable Service surface. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

## B-07 — Declare a Namespace on a Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance on which no invocation (`#eval` or `#run`) has yet been called. No Namespace named `Name` exists on this Sandbox. |
| **Operation** | `sandbox.define(:Name)` where `:Name` is a Symbol matching `/\A[A-Z]\w*\z/` (Ruby constant-name form). |
| **Result / Final State** | A `Kobako::Namespace` instance is created and associated with this Sandbox under the name `Name`. The namespace has no members yet. The method returns the new `Kobako::Namespace` instance. The Sandbox's `Catalog::Namespaces` now tracks one additional namespace entry. Declaration is a design-time operation sealed by the first invocation (B-33): a non-conforming name raises `ArgumentError` (E-16), and `define` after the seal raises `ArgumentError` (E-18) while the Sandbox stays usable with the registrations that existed at sealing. A namespace may have zero members at declaration; members are added via B-08. |

---

## B-08 — Bind a Member to a declared Namespace

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Namespace` instance (returned by `sandbox.define`) with no member bound under the name `MemberName`. The owning Sandbox has not yet run its first invocation (B-33). |
| **Operation** | `namespace.bind(:MemberName, object)` where `:MemberName` matches `/\A[A-Z]\w*\z/` and `object` is any Ruby object (class, instance, or module) that responds to the methods guest code will invoke. |
| **Result / Final State** | `object` is registered as the Member named `MemberName` within the namespace. Guest code can now reach this object via the two-level path `<Namespace>::<Member>`. The method returns the `Kobako::Namespace` instance (`self`) to allow chaining. The bound object must remain valid for the Sandbox's lifetime; the Host App manages its lifecycle. A non-conforming `MemberName` raises `ArgumentError` (E-17). Binding is sealed by the first invocation alongside declaration and preload (B-33): after the seal `bind` raises `ArgumentError` (E-45), and every subsequent invocation carries exactly the bindings that existed at sealing. |

---

## B-09 — Declare multiple Namespaces on the same Sandbox

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with one or more Namespaces already declared. |
| **Operation** | `sandbox.define(:OtherName)` with a name distinct from all already-declared namespaces on this Sandbox. |
| **Result / Final State** | A new, independent `Kobako::Namespace` is created alongside the existing namespaces. Each namespace's members are accessible to guest code only via that namespace's own path (e.g., `NamespaceA::Member` and `NamespaceB::Member` are distinct paths with no cross-namespace visibility). Namespaces on different Sandbox instances are fully isolated from each other. There is no declared upper limit on the number of namespaces per Sandbox; each namespace name within a Sandbox must be unique (idempotent re-declaration is specified in B-10). |

---

## B-10 — Re-declare a Namespace that already exists (idempotent define)

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox instance with a Namespace already declared under the name `Name`. |
| **Operation** | `sandbox.define(:Name)` — same name as an existing namespace. |
| **Result / Final State** | No new namespace is created. `sandbox.define(:Name)` returns the identical `Kobako::Namespace` object previously created — the same object identity (Ruby `equal?`), not a new instance wrapping the same `Catalog::Namespaces` entry. All previously bound members remain in place. The Sandbox's `Catalog::Namespaces` is not modified. |

---

## B-11 — Attempt to bind a Member name that is already bound in the same Namespace

| Field | Value |
|-------|-------|
| **Initial State** | A `Kobako::Namespace` instance with a member already bound under the name `MemberName`. |
| **Operation** | `namespace.bind(:MemberName, new_object)` — same member name as an already-bound member. |
| **Result / Final State** | `ArgumentError` is raised. The existing binding is not overwritten. The namespace's member registry is unchanged. |
