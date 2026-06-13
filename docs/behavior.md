# Behavior

The behavior contract for `Kobako::Sandbox` and its execution, as per-anchor
specifications (Initial State → Operation → Result / Final State). The governing
summary — the four-outcome guarantee for every invocation and the two-step
attribution decision — lives in [`SPEC.md`](../SPEC.md) § Behavior; the files
below are the per-anchor reference. `B-xx` / `E-xx` anchors are global and
append-only across the corpus (N-8); `rake anchors` gates their uniqueness.

| Aspect | Anchors | Specification |
|--------|---------|---------------|
| Lifecycle — construction, `#eval`, output, usage | B-01..B-06, B-35 | [`behavior/lifecycle.md`](behavior/lifecycle.md) |
| Registration — Namespaces and Members | B-07..B-11 | [`behavior/registration.md`](behavior/registration.md) |
| Dispatch — Transport calls and Handle lifecycle | B-12..B-21, B-34, B-37 | [`behavior/dispatch.md`](behavior/dispatch.md) |
| Yield — block re-entry | B-23..B-30 | [`behavior/yield.md`](behavior/yield.md) |
| Invocation verbs — `#run` and `#preload` | B-31..B-33 | [`behavior/invocation.md`](behavior/invocation.md) |
| Security — capability confinement | B-36, B-38..B-39, B-41..B-45, B-50 | [`behavior/security.md`](behavior/security.md) |
| Runtime — isolation, ABI, Pool, boot state | B-22, B-40, B-46..B-49 | [`behavior/runtime.md`](behavior/runtime.md) |
| Error scenarios & attribution | E-01..E-48 | [`behavior/errors.md`](behavior/errors.md) |
