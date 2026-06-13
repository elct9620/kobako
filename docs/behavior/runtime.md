# Runtime — isolation, ABI, Pool, boot state

Per-Thread isolation, the Guest Binary ABI gate, the warm `Kobako::Pool`, and the canonical boot state every invocation begins from. The governing summary lives in [`SPEC.md`](../../SPEC.md)
§ Behavior; this file is the per-anchor reference. `B-xx` anchors are global
and append-only across the corpus (N-8).

## B-22 — Distinct Sandboxes on distinct Threads execute independently

| Field | Value |
|-------|-------|
| **Initial State** | Two or more Ruby Threads exist within the same process. The Host App has constructed one `Kobako::Sandbox` per Thread (honoring the input assumption in Scope → Interaction). |
| **Operation** | Each Thread invokes `#eval` or `#run` only on its own owning Sandbox; no Sandbox is shared across Threads. |
| **Result / Final State** | Each invocation executes independently — capability state, Handle IDs, and capture buffers are scoped per Sandbox and never observed by another Thread's invocation. The wasmtime Engine and the compiled Module for `data/kobako.wasm` are shared at process scope: the first Sandbox in the process pays the Engine init and Module compile cost; subsequent Sandboxes in any Thread amortize against that shared state. |
| **Notes** | Aggregate throughput across Threads is bounded by Ruby's GVL — Kobako's native extension does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is serialized. Ruby-side setup (preamble pack, buffer init) can overlap across Threads, giving modest but non-linear scaling under contention. The Host App is responsible for the one-Thread-per-Sandbox invariant; Kobako provides no locking and concurrent invocations on the same Sandbox are unsupported (Scope → Interaction). |

---

## B-40 — Host validates the Guest Binary ABI version at construction

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Sandbox` instance exists. A Guest Binary artifact is present at the resolved `wasm_path` and exports `__kobako_abi_version` returning the ABI version the Host Gem implements (→ [`docs/wire-codec.md`](../wire-codec.md) § ABI Version). |
| **Operation** | `Kobako::Sandbox.new` — with the default bundled Guest Binary or a custom `wasm_path:`. |
| **Result / Final State** | Construction probes `__kobako_abi_version` after the wasm runtime setup of B-01 and compares the reported value against the Host Gem's implemented version by equality. On equality, construction completes per B-01. No invocation entry point runs. |
| **Notes** | The probe is the only guest function construction calls — `__kobako_abi_version` is a pure constant function (→ [`docs/wire-codec.md`](../wire-codec.md) § ABI Version), so no invocation state exists before or after it. The check exists for Guest Binaries that ship independently of the Host Gem: the bundled `data/kobako.wasm` matches by construction, while a custom guest built against a different ABI version fails loudly at `Sandbox.new` (E-42) instead of misbehaving mid-invocation. An absent export is the same failure — a guest predating the version export is by definition built against a different ABI. |

---

## B-46 — Construct a Kobako::Pool

| Field | Value |
|-------|-------|
| **Initial State** | No `Kobako::Pool` instance exists. The Guest Binary is resolvable per B-01. |
| **Operation** | `Kobako::Pool.new(slots: n) { \|sandbox\| ... }` — `slots:` (positive Integer) is the number of pooled Sandboxes; `checkout_timeout:` (Numeric seconds, default `5.0`, `nil` to wait indefinitely) bounds the B-47 checkout wait; every other keyword argument is forwarded verbatim to `Kobako::Sandbox.new`; the optional block is the per-Sandbox setup hook. |
| **Result / Final State** | A Pool managing up to `slots` Sandboxes is returned. No Sandbox is constructed yet: a checkout (B-47) receives an idle constructed Sandbox when one exists, and constructs a new one — with the forwarded keyword arguments — only when no idle Sandbox exists and fewer than `slots` have been constructed. The setup block runs exactly once per pooled Sandbox immediately after its construction, before that Sandbox is first handed to any checkout caller. No invocation entry point runs. |
| **Notes** | The setup block is the pooled Sandbox's setup window (B-07 / B-08 / B-32); its registrations seal at that Sandbox's first invocation (B-33) exactly as on a directly constructed Sandbox. Sandbox construction and setup-block errors surface unchanged — original class and message — at the `#with` call whose checkout triggered the creation (E-39..E-42, or the block's own exception). Invalid `slots:` / `checkout_timeout:` raise `ArgumentError` at `Pool.new` (E-47). Pool construction signals the runtime to provision instance resources for slot reuse; provisioning is not observable — a pooled Sandbox satisfies every behavior anchor identically (B-47). |

---

## B-47 — Check out a Sandbox via Pool#with

| Field | Value |
|-------|-------|
| **Initial State** | A Pool (B-46). Any number of threads call `#with` concurrently. |
| **Operation** | `pool.with { \|sandbox\| ... }` |
| **Result / Final State** | The calling thread holds exclusive use of one pooled Sandbox for the duration of the block; `#with` returns the block's return value. When all `slots` Sandboxes are held, the call blocks until one is returned, or raises `Kobako::PoolTimeoutError` once the wait exceeds `checkout_timeout` (E-46). At block exit — normal return or raised exception — the Sandbox returns to the pool. At checkout, `#stdout` / `#stderr` read as empty and both truncation predicates are false; Service bindings and snippets registered by the setup block remain active. Every B-xx / E-xx behavior holds for a pooled Sandbox exactly as for a directly constructed one — in particular per-invocation isolation (B-03) guarantees that no guest-observable state crosses from one checkout holder to the next. |
| **Notes** | Checkouts are independent: a nested `#with` on the same thread checks out a second Sandbox and counts against `slots` like any other holder. Per-checkout exclusivity is what extends B-22's one-thread-at-a-time contract to pooled Sandboxes. A checkout whose block raises `Kobako::TrapError` has its Sandbox discarded at checkin — the pool applies the discard-and-recreate recovery contract itself, refilling the slot by a fresh construction + setup-block run on next demand. |

---

## B-48 — Pool teardown follows Pool reachability

| Field | Value |
|-------|-------|
| **Initial State** | A Pool holding constructed Sandboxes; zero or more are checked out. |
| **Operation** | The Host App drops its last reference to the Pool. |
| **Result / Final State** | The Pool and the pooled Sandboxes it holds become unreachable and are reclaimed by Ruby garbage collection like ordinary objects, releasing every runtime resource they held. A Sandbox held by an in-flight `#with` block remains valid until that block exits. |
| **Notes** | The Pool has no explicit teardown verb; reachability is the lifecycle. This mirrors B-19 — discarding the owning object is the resource-release path. |

---

## B-49 — Every invocation begins from the canonical boot state

| Field | Value |
|-------|-------|
| **Initial State** | A Sandbox with any invocation history — none, successful, or failed. |
| **Operation** | Any invocation entry — `sandbox.eval(code)` or `sandbox.run(target, ...)`. |
| **Result / Final State** | The mruby interpreter the invocation observes starts in the canonical boot state: the deterministic post-boot interpreter state of the Guest Binary, identical for every invocation of the same artifact (B-45) and carrying no artifact of any prior invocation (B-03). On top of that state, in order: the Frame 1 preamble installs the Sandbox's registrations, preloaded snippets replay (B-32), and then per-invocation source load (`#eval`) or entrypoint resolution (`#run`) proceeds. |
| **Notes** | The canonical boot state may be computed at build time and embedded in the Guest Binary as a pre-initialized image; embedding is unobservable, and re-baking the same inputs yields a byte-identical artifact — the reproducible-build pipeline (F-10) gates this. The engine may likewise provision per-invocation instance resources for reuse (B-46 Notes); provisioning is equally unobservable. |
