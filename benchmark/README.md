# Benchmarks

Kobako maintains a regression benchmark suite covering the six performance dimensions [SPEC.md](../SPEC.md) names as release quality gates (startup, Transport round-trip, codec, mruby VM, Catalog::Handles, yield round-trip) plus three characterization suites (multi-thread behaviour, per-Sandbox RSS, `#preload` + `#run` dispatch). Baselines for every release live under `benchmark/results/` so subsequent runs can diff against a known point; a +10% regression on any of the six gated benchmarks requires explicit review before release.

## Latest baseline

Captured on **2026-05-27** at commit `711665d` (the 0.5.0 release line) — macOS arm64, Ruby 3.4.7, 16 CPUs, YJIT off. Numbers below are typical; absolute values vary by hardware, but the relative shape (cold/warm ratio, Transport overhead, scaling curves) is consistent across machines.

How the numbers are obtained:

- **`ips` cases** — the runner calibrates an iteration count that lands ~1/5 of the time budget per cycle, runs a warmup pass with the same iteration shape, then records CPU time per cycle until the budget is exhausted. `ips` is the **median** of the per-cycle samples (not the mean — a single GC-inflated cycle skews a mean but not a median, which matters for large-payload cases that allocate fresh Ruby objects per iteration); `±ips_sd` is the sample standard deviation as a percentage. The arithmetic mean rides along as `ips_mean` (rendered `mean=…` in the tables) for the capacity / throughput reading, mirroring Google Benchmark and Criterion, which report both. CPU time excludes scheduler / background-load noise, so the same code on the same machine reproduces within the reported `±ips_sd`.
- **`one_shot` cases** — the block runs exactly once and the CPU seconds consumed are recorded. Used for cold-path costs (the very first `Sandbox.new` in a process, large-table allocations) where iterating would only ever observe the warm path.
- **Multi-thread cases** keep their own wall-clock helper and bypass the runner, because measuring scheduler overhead by CPU time would defeat the purpose.
- **Per-invocation `usage`** (sandbox-driven `ips` cases only) — for cases whose block drives a `Kobako::Sandbox`, the runner runs a dedicated post-measurement sampling loop (`UsageSampler`, CPU-budget-bounded) that reads `sandbox.usage` ([SPEC.md B-35](../SPEC.md)) after each invocation and folds the **median** `wall_time` (Float seconds the guest export call spent inside wasmtime) and `memory_peak` (Integer bytes of `memory.grow` delta past the per-invocation baseline) into the same JSON row. The median makes `wall_time` a distribution rather than the single last-iteration reading, so deriving the host-wrapper term (`1/ips − wall_time`) no longer subtracts a point sample from a loop aggregate. Its dispersion rides along as `wall_time_sd` (rendered `±…%` next to `wall=`) so the release gate can build a noise band on the gate-correct metric. Host throughput (`ips`) and per-invocation guest budget surface together, so the per-`#eval` overhead and the VM execution time are directly readable instead of derived by subtraction. The `memory.rb` (#8) suite samples the same two fields alongside the RSS deltas it already records.

### Reading the numbers

Every per-suite table in this file is derived from the raw JSON in `benchmark/results/<date>-<sha>.json` via one shared conversion script. Future baseline refreshes regenerate the tables by re-running it:

```
bundle exec ruby benchmark/support/format_baseline.rb [path/to/results.json]
```

The script defaults to the most recently modified result file when no path is given. The unit conversions it applies (single source of truth — change here, not by hand in the tables below):

| Raw field | Derived | Conversion |
|---|---|---|
| `ips` (≥ 1 000 000) | `ns` per op | `1e9 / ips` |
| `ips` (1 000 .. 999 999) | `µs` per op | `1e6 / ips` |
| `ips` (< 1 000) | `ms` per op | `1000 / ips` |
| `ips_sd` | `±sd` percentage | `(ips_sd / ips) * 100` |
| `seconds` (`one_shot`) | `ms` | `seconds * 1000` |
| `wall_time` (< 1e-6 s) | `ns` | `wall_time * 1e9` |
| `wall_time` (1e-6 .. 1e-3 s) | `µs` | `wall_time * 1e6` |
| `wall_time` (≥ 1e-3 s) | `ms` | `wall_time * 1e3` |
| `memory_peak` (< 1024 B) | `B` | direct |
| `memory_peak` (1024 .. 1 048 575 B) | `KiB` | `memory_peak / 1024` |
| `memory_peak` (≥ 1 048 576 B) | `MiB` | `memory_peak / 1 048 576` |
| `rss_kb` | `MB` | `rss_kb / 1024` |
| `ops_per_sec` (concurrent) | `ops/s` (pretty-printed) | direct, with `k` suffix above 10 000 |

For "N-ops-in-one-invocation" cases (e.g., `2d-1000-calls-in-one-eval`), the table cell shows both the per-invocation total (`N / ips`) and the per-op cost (the same value divided by N). The script emits the per-invocation total; the per-op interpretation is added in prose.

For "delta between waypoints" rows (e.g., 9a-1 → 9a-64 isolating per-snippet preload cost), subtract the lower waypoint from the higher and divide by the snippet-count delta. Worked examples are inline in the respective sections.

For sandbox-driven `ips` cases the JSON now also carries `wall_time` and `memory_peak` from `Kobako::Sandbox#usage`. `format_baseline.rb` renders them next to the ips meta as `wall=… mem=…` (e.g. `±1.4%, n=3 | wall=80.0 µs mem=0 B`). The two readers turn the per-`#eval` overhead into a directly recorded number: subtract `wall_time` from `1 / ips` to get the host wrapper cost. For "N-ops-in-one-invocation" cases divide `wall_time` by N to get the steady-state per-op guest cost without the host wrapper term — `2d-1000-calls-in-one-eval` is the canonical example. `memory_peak` is `0` for cases that don't grow guest linear memory (nil-returning evals, Transport round-trips); for cases that do (large String returns, deep Array construction), it reports the high-water `memory.grow` delta past the per-invocation baseline.

**Rounding convention.** The script emits one decimal place for ips-derived values (`275.6 µs`) so the conversion is reversible; the prose tables round to three significant figures (`276 µs`) for readability. A future-baseline diff that comes from rounding alone — e.g., `275.6 µs` versus `274.9 µs` both rendering as `275 µs` — is not a real change. When comparing two baselines treat any prose-table delta under 1 µs as noise and consult the JSON / script output for the precise value.

### Sandbox construction and first run ([`cold_start.rb`](cold_start.rb))

Two costs dominate the very first `Kobako::Sandbox` in a process: wasmtime Engine initialization and `data/kobako.wasm` Module JIT compile. Both are cached at process scope, so every subsequent `Sandbox.new` in the same process is orders of magnitude cheaper.

| Scenario | Latency |
|---|---|
| First `Sandbox.new` in a fresh process | **629 ms** |
| Second-through-tenth `Sandbox.new` (cache warm) | **~0.11 ms** each |
| Steady-state `Sandbox.new` only | **125 µs** |
| Steady-state `Sandbox.new` + first `#eval("nil")` | **272 µs** |

The first-Sandbox cost is dominated by wasmtime JIT compiling the Module on macOS arm64. The Module is sizeable today because the guest binary embeds the mruby interpreter, the `mruby-onig-regexp` Onigmo engine, and the precompiled `mrblib/io.rb` + `mrblib/kernel.rb` IO preamble; each of those is a feature commitment the cold-start cost pays for once per process.

Practical implication: pre-warm by constructing one Sandbox at boot. After that, every per-request Sandbox costs micro-, not seconds.

### Reusing a Sandbox vs constructing one per request

| Pattern | Cost per request | Source |
|---|---|---|
| Reuse the same Sandbox (`#eval("nil")` on a warm instance) | **136 µs** | `2a-empty-call` |
| Fresh Sandbox every request (`Kobako::Sandbox.new.eval("nil")`) | **272 µs** | `1b-sandbox-new+eval-nil` |
| Overhead of constructing a new Sandbox per request | **~136 µs** | difference |

Per-request construction does NOT pay the multi-second Engine/Module cold cost again — that is amortized to the first Sandbox in the process regardless of pattern. The per-request overhead is the `Sandbox.new` work itself (Wasm instance creation, capture-buffer allocation, dispatch Proc wiring).

B-35 `wall_time` makes the per-`#eval` split readable, but read it from the warm-reuse row, not the construct-per-request row. `2a-empty-call` (a warm-instance `#eval("nil")`) records a 136 µs total against a median guest `wall_time` of 127 µs, so the host wrapper term (msgpack envelope encode, outcome decode, capture readout) is small — single-digit µs — and the guest export dominates even an empty `#eval`. The `1b − 2a` difference (~136 µs) is therefore the `Sandbox.new` construction cost on the Ruby side, not the host wrapper. (`1b`'s own `wall_time` is a single post-loop `annotate_usage!` sample of a *fresh* Sandbox's first `#eval` — it carries no dispersion and can read well above the warm guest export, so it is not a reliable basis for the split; the median `2a` reading is.)

Practical implication: choose per-request construction when guest scripts are mutually untrusted; choose reuse when a single Sandbox serves repeated requests from the same trust scope. At ~140 µs of extra overhead per request, per-request isolation is affordable for most web/job workloads.

### Per-request Transport latency ([`transport_roundtrip.rb`](transport_roundtrip.rb))

Each row wraps the call inside one `#eval`, so the absolute number bundles `#eval` setup (~130 µs) with the Transport round-trip.

| Scenario | Latency | `wall_time` (guest) |
|---|---|---|
| One Service call returning `nil`, alone in `#eval` | **136 µs** | 127 µs |
| One Service call with one Integer arg | **138 µs** | 128 µs |
| One Service call with one Symbol-keyed keyword arg | 139 µs | 131 µs |
| 1 000 sequential Service calls inside one `#eval` | 6.74 ms total → 6.7 µs per call | 6.73 ms / 6.7 µs per call |
| Handle chain — one call returns object, second targets the Handle | 152 µs | 142 µs |

All five rows reproduce within ±2% across cycles. The per-call steady-state cost holds at ~6.7 µs once the per-`#eval` setup is amortized over many calls in one invocation; `wall_time` shows the same per-call cost holds inside the guest export bracket too, so the loop is dominated by guest-side dispatch and not Ruby-side scheduling. The Handle-chain row exercises [SPEC.md B-17](../SPEC.md): a Factory Service returns a host object → kobako allocates a Handle on the return path → the guest invokes a method against the Handle → kobako fetches the host object via the Handle. The cost above the empty-call baseline is the second call plus one `Catalog::Handles#alloc` and one `Catalog::Handles#fetch`.

### Wire codec — host side ([`codec.rb`](codec.rb))

Encoding and decoding through `Kobako::Codec` directly from Ruby. These numbers do not cross the wasm boundary; they characterize the host-side codec on its own.

| Payload | Encode | Decode |
|---|---|---|
| String, 64 B | 541 ns | 581 ns |
| String, 1 KiB | 644 ns | 668 ns |
| String, 64 KiB | 7.6 µs | 2.9 µs |
| String, 1 MiB | 59.3 µs | 36.0 µs |
| Array nested 1 deep (1 KiB leaf) | 676 ns | 800 ns |
| Array nested 64 deep (1 KiB leaf) | 1.1 µs | 8.8 µs |

Per-wire-type micro-bench at primitive sizes, one entry per SPEC.md Type Mapping row (12 entries):

| Wire type | Encode | Decode |
|---|---|---|
| `nil` / Boolean / Integer / Float | 524-529 ns | 530-556 ns |
| Short String / binary String | 537-541 ns | 563-590 ns |
| 3-element Array / 1-entry Hash | 554-558 ns | 815-917 ns |
| Symbol (ext 0x00) | 619 ns | 730 ns |
| Handle (ext 0x01) | 657 ns | 1.0 µs |
| Exception envelope (ext 0x02) | 1.3 µs | 2.9 µs |

All rows reproduce within ±3% across cycles. The 64KiB / 1MiB decode rows hold the load-bearing String codec numbers; large-payload allocation noise occasionally widens the encode side past ±3% but the decode side stays tight.

### Wire codec — guest side ([`codec.rb`](codec.rb))

The guest builds a value in mruby and returns it from `#eval`. The absolute numbers bundle guest encode + host decode + the constant `#eval` overhead; cross-row differences isolate the codec contribution.

| Guest script returns | Latency | `wall_time` (guest) |
|---|---|---|
| `"x" * 64` (64 B String) | 123.5 µs | 116 µs |
| `"x" * 1024` (1 KiB String) | 124.4 µs | 116 µs |
| `"x" * 65536` (64 KiB String) | 158.1 µs | 143 µs |
| `"x" * 524288` (512 KiB String) | 449.4 µs | 333 µs |
| Array nested 1 deep (1 KiB leaf) | 125.3 µs | 117 µs |
| Array nested 64 deep (1 KiB leaf) | 164.4 µs | 147 µs |

Note: guest mruby caps a single String at 1 MiB ([SPEC Invariant](../SPEC.md)); the largest guest sample here is 512 KiB. Composite values (Arrays, Hashes) can still approach the 16 MiB wire payload limit.

The `wall_time` column splits "guest export inside wasmtime" from the per-`#eval` host wrapper (msgpack envelope + outcome decode + capture readout). Cross-row: the host wrapper holds steady at ~12-15 µs regardless of guest payload size — most of the size-dependence lives inside `wall_time` (guest codec construct + return-value encode), which matches the codec-host-side scaling above.

### mruby VM, no Transport ([`mruby_eval.rb`](mruby_eval.rb))

Pure interpreter work — every script is a self-contained mruby computation whose only host cost is the constant `Sandbox#eval` overhead. Useful for spotting regressions in `build_config/wasi.rb` flag changes.

| Script | Latency | `wall_time` (guest) |
|---|---|---|
| 100 000-iteration integer XOR loop | **42.83 ms** | 42.71 ms |
| 1 000 single-character String appends | 585 µs | 575 µs |
| 100 cycles of `raise` / `rescue` | 301 µs → 3.0 µs per cycle | 303 µs |
| 1 000 Onigmo `Regexp =~` matches | 3.01 ms → 3.0 µs per match | 3.01 ms |
| 1 000 `puts` of 64 B (below 1 MiB stdout cap) | 4.26 ms → 4.3 µs per write | 4.17 ms |
| 2 048 `puts` of ~1 KiB against the 1 MiB stdout cap | 9.28 ms (first ~1 024 land, rest silently dropped) | 8.54 ms |

The `4d` / `4e` / `4f` rows cover features that landed since `0.1.2`: Onigmo `Regexp` via `mruby-onig-regexp`, the full B-04 IO surface (`puts` / `print` / `printf` / `p` / `$stdout` / `$stderr` wired through to a host-captured WASI pipe), and the per-channel `stdout_limit` cap on that capture buffer. The cap is honored: guest `puts` does not raise on rejection, the pipe returns short, the loop runs to completion, and `sandbox.stdout_truncated?` is `true` after the run.

The `wall_time` column rounds to ips-equivalent for everything except `4e` / `4f`, where the guest export bracket captures essentially the whole cost — the host wrapper is fixed per-`#eval` while the inside-VM stdout loop dominates. `4e` per-write holds at ~4.3 µs and `4f` at ~9.3 ms, flat versus the previous (`1be572c`) baseline. That level reflects the `dfd14db` move of the `IO#write` byte loop out of a hand-written C shim into safe Rust via bindgen (a one-time +24% step absorbed at the prior baseline, not a fresh regression); `memory_peak = 0` on both rows confirms the path is wasi-libc-bound, not guest-linear-memory-bound.

### Handle table scaling ([`catalog_handles.rb`](catalog_handles.rb))

`Catalog::Handles` is the host-side mapping from opaque integer IDs to Ruby objects, reset at the start of every invocation (`#eval` or `#run`). These numbers verify the underlying Hash stays O(1) as it grows.

| Scenario | Latency |
|---|---|
| Allocate one Handle in an empty table | 541 ns |
| Allocate 100 Handles from empty | 42.1 µs total |
| Allocate 10 000 Handles from empty | 4.08 ms total |
| Allocate 100 000 Handles from empty | 44.24 ms total |
| 1 000 allocs against a 1 K-entry table | 0.349 ms |
| 1 000 allocs against a 10 K-entry table | 0.340 ms |
| 1 000 allocs against a 100 K-entry table | 0.353 ms |
| 1 000 allocs against a 1 M-entry table | 0.487 ms |
| Warm `Sandbox#eval("nil")` round-trip under sustained heap pressure | 122 µs (`wall_time` = 115 µs) |

The 1 K to 1 M waypoint rows confirm the underlying dictionary still stays effectively flat as the table grows — per-alloc cost holds around 349-487 ns across four orders of magnitude, with the gentle climb attributable to allocator state rather than dictionary lookup curve. ([SPEC.md B-21](../SPEC.md) caps the counter at `0x7fff_ffff` and rejects allocation past the cap; the cap guard itself is constant-time and not iterated here.)

All `5a` / `5b` rows hold flat (a few percent better) versus the previous (`1be572c`) baseline. The `Catalog::Handles#alloc` cost of returning a `Kobako::Handle` object instead of a bare Integer id — a 3-4× step absorbed at an earlier baseline (`14b8754`) — is now the steady level, not a moving number.

The `5c-warm-eval-nil-under-gc-pressure` row deliberately measures a different dimension than `1b-sandbox-new+eval-nil` from cold_start (~272 µs). It runs **after** the 5b loop has grown a 1 M-entry handle table that stays alive in the same Ruby process for the rest of the run, so every measured `#eval` allocates capture-buffer Strings under sustained GC pressure. 1b is the clean per-invocation cost; 5c is the regression signal for changes that make per-invocation work more GC-sensitive when the process is already holding a large handle table — a condition 1b cannot detect.

### Yield round-trip latency ([`yield_roundtrip.rb`](yield_roundtrip.rb))

The host-initiated counterpart of #2. Where `transport_roundtrip` measures the guest→host Request/Response direction, this suite measures the reverse re-entry: a Service method yields into a guest-supplied block ([SPEC.md B-23..B-30](../SPEC.md)). The cost lives on a different path — the `YieldResponse` codec, the `__kobako_yield_to_block` export, and the guest-side `BLOCK_STACK` push/pop — so a regression here is invisible to #2. Every case wraps one `#eval`; regression detection is on the delta between cases, not the absolute ips.

| Case | What it isolates |
|---|---|
| `6a-single-yield` | One yield (tag 0x01 ok) above the no-block #2 baseline — the single-yield latency. |
| `6b-block-no-yield` | A call site that supplies a block the Service never invokes (B-30): the `block_given` flag travels and the host builds a Yielder, but there is zero re-entry. The block-flag + Yielder construction/invalidation floor. |
| `6c-1000-yields-in-one-call` | 1 000 yields in one dispatch (the J-06 iteration shape). Per-yield steady-state cost is `wall_time / 1000`, isolating the re-entry path from the per-dispatch setup the way `2d` does for guest→host calls. This is the load-bearing number for `each`-style Services. |
| `6d-yield-break` | A block that runs `break` on the first yield (tag 0x02), unwinding the Service via catch/throw (B-25). The delta over `6a` is the break classification + unwind cost. |

| Case | Latency | `wall_time` (guest) |
|---|---|---|
| `6a-single-yield` | 143 µs | 133 µs |
| `6b-block-no-yield` | 138 µs | 129 µs |
| `6c-1000-yields-in-one-call` | 3.86 ms → 3.9 µs per yield | 3.89 ms / 3.9 µs per yield |
| `6d-yield-break` | 270 µs | 258 µs |

`6c` is the gate-relevant row: it gates on `wall_time` (guest budget), so the 1 000-element return-array decode on the host side does not enter the gated metric. The steady-state per-yield re-entry cost is `wall_time / 1000` ≈ **3.9 µs** — comparable to the ~6.7 µs guest→host Service call (`2d`), with the lighter cost reflecting the shorter `YieldResponse` path. The single-yield floor sits ~5 µs above the block-given-but-never-yielded case (`6a` 143 µs vs `6b` 138 µs), so constructing a Yielder the Service never invokes (B-30) is nearly free; the cost is the re-entry itself. `6d` also builds the 1 000-element guest array that `6c` does, so the `6d − 6a` gap is the break classification + catch/throw unwind (B-25) on top of that allocation, not a pure delta.

### `#preload` + `#run` dispatch ([`preload_dispatch.rb`](preload_dispatch.rb)) — characterization only

Coverage of the two verbs added after the SPEC #1..#5 suite was written. `#preload` and `#run` are independent features — `#preload(code: ..., name: ...)` registers snippets that replay against the fresh `mrb_state` on every subsequent invocation (whether `#eval` or `#run`); `#run(:Target)` dispatches into a preloaded entrypoint constant via the Invocation envelope. The rows below isolate each verb's contribution rather than comparing them against `#eval`.

| Scenario | Latency | `wall_time` (guest) |
|---|---|---|
| `Sandbox.new` + 1 `#preload(code:)` | 126 µs | — |
| `Sandbox.new` + 8 `#preload(code:)` | 133 µs | — |
| `Sandbox.new` + 64 `#preload(code:)` | 259 µs | — |
| Warm `#run(:Noop)` (1 entrypoint preloaded) | 163 µs | 151 µs |
| Warm `#run(:Echo, 42)` (positional arg) | 163 µs | 151 µs |
| Warm `#run(:Greet, name: :alice)` (Symbol-keyed kwargs) | 166 µs | 153 µs |
| Warm `#run(:Wrap, StringIO)` (B-34 host→guest auto-wrap) | 157 µs | 141 µs |
| Warm `#run(:Noop)` with 0 helper snippets preloaded | 151 µs | 138 µs |
| Warm `#run(:Noop)` with 8 helper snippets preloaded | 208 µs | 193 µs |
| Warm `#run(:Noop)` with 64 helper snippets preloaded | 735 µs | 730 µs |

9a's 1→8→64 sweep is dominated by the `Sandbox.new` term (~125 µs from `1a-sandbox-new`) at low N; the meaningful signal is the 1→64 delta — 259 − 126 = 133 µs spread across 63 extra `#preload` calls, which puts the per-snippet preload cost at ~2.1 µs. The `#preload(code:)` path trial-compiles each source against a fresh `mrb_state` to catch E-32 early; that compile dominates per-snippet cost. 9a rows do not carry `wall_time` because the timer wraps `Sandbox.new + #preload` and neither call invokes the guest export — `sandbox.usage` is the `EMPTY` sentinel at that point.

9b / 9c / 9d show that positional args and Symbol kwargs add essentially nothing on top of the empty `#run` baseline (163-166 µs vs 163 µs). The Invocation envelope's args / kwargs encoding is cheap compared to the per-invocation setup. The ext 0x00 path here is the host→guest direction; the structurally distinct guest→host kwargs path is covered by `transport_roundtrip 2c` at ~139 µs.

9f covers the [B-34](../docs/behavior.md) host→guest auto-wrap path that 9c / 9d miss. The arg (a `StringIO`) is not wire-representable, so `Kobako::Codec::Utils.deep_wrap` routes it through `Catalog::Handles#alloc` and the guest sees a `Kobako::Handle` proxy in its place. The entrypoint discards the proxy without calling back, so the case isolates the host-side wrap cost — predicate + `alloc` + wire encode — without compounding with a guest→host Transport round-trip. At 157 µs the case lands ~6 µs *below* 9c's 163 µs positional `Integer` baseline, which means the wrap path itself is at worst comparable to a wire-fast Integer arg path under the current implementation. A regression that makes `deep_wrap` or `Catalog::Handles#alloc` super-linear in arg count would show as 9f rising above 9c here.

9e isolates per-invocation snippet replay cost: the 0→8 delta gives (208 − 151) / 8 ≈ 7.1 µs per snippet per invocation, and the 0→64 delta gives (735 − 151) / 64 ≈ 9.1 µs per snippet — linear in snippet count, which is what B-32's "replay every snippet against the fresh `mrb_state`" contract requires. `wall_time` on `9e-replay-64` (730 µs) confirms the replay cost is paid inside the guest export, not in host-side dispatch.

### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb)) — characterization only

`ext/` does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is GVL-serialized. Ruby-side `#eval` setup can still overlap, which is why throughput scales modestly rather than not at all. This suite uses wall-clock timing because that is what scheduler effects manifest in.

| Scenario | Result |
|---|---|
| 1 Thread, owning one Sandbox | 7.8k `#eval`/s |
| 2 Threads, each owning one Sandbox | 6.6k `#eval`/s |
| 4 Threads, each owning one Sandbox | 7.4k `#eval`/s |
| 8 Threads, each owning one Sandbox | 7.5k `#eval`/s |
| Per-Sandbox `Sandbox.new` cost, single-Threaded | 0.175 ms |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel | 0.163 ms each (1.306 ms total / 8) |
| `#eval("nil")` baseline | 0.134 ms |
| `#eval("nil")` while another Thread is in a long `#eval` | 0.199 ms (1.5× baseline) |

Practical implication for Sidekiq / Puma cluster shapes: a long-running script does NOT block other Threads' short `#eval` calls by hundreds of milliseconds. The contention overhead is bounded because any host-side synchronization (Queue push from a Service callback, mutex acquisition, IO) yields the GVL and lets the contending Thread interleave. The exact ratio varies run-to-run (1.5-3×) with scheduler quirks; the order of magnitude is the regression signal.

### Memory cost ([`memory.rb`](memory.rb)) — characterization only

External RSS sampling (`ps -o rss=`) — we only observe what the host process consumes, never look inside the Sandbox's mruby heap or Wasm linear memory. This is the right granularity for capacity planning (how many tenants fit in one process) without violating SPEC's Non-Goal of per-invocation instrumentation.

| Scenario | RSS | B-35 `memory_peak` |
|---|---|---|
| Process RSS at boot (no Sandbox) | 25.3 MB | — |
| RSS after the first `Sandbox.new` + `#eval("nil")` | 137.9 MB (**+113 MB** — Engine init + Module JIT + 1 instance, one-time) | — |
| RSS after 10 Sandboxes total | 143.0 MB (~570 KB per additional Sandbox) | — |
| RSS after 100 Sandboxes total | 194.8 MB (~570 KB per additional Sandbox) | — |
| RSS after 1 000 Sandboxes total | 705.3 MB (~**570 KB per additional Sandbox**) | — |
| RSS drift after 10 000 consecutive `#eval("nil")` on one Sandbox | +2.3 MB over the whole run (~0.23 KB / invocation; allocator page retention) | **0 B** per invocation across all 1 K sample points |
| Peak RSS while holding a 512 KiB return value | +3.6 MB above baseline | **2.5 MiB** (guest `memory.grow` delta for the 512 KiB String) |
| Retained RSS after GC of the same value | +3.6 MB (allocator does not eagerly return pages to the OS; the Ruby reference is dropped) | — |
| Peak RSS while holding a 1 MiB capped stdout buffer | +4.2 MB above baseline (allocator-state-dependent — see note) | **64 KiB** (stdout flows through the WASI pipe, not guest linear memory) |
| Retained RSS after GC of the same capture | +3.5 MB | — |

Practical implication for tenant isolation: budget ~140 MB up front per worker process (paid by the first Sandbox), plus ~570 KB per concurrent tenant. **1 000 tenants ≈ 705 MB** in one Ruby process — comfortably within a typical Sidekiq / Puma worker's RSS limit. The first-Sandbox figure swings run-to-run with host process load and allocator state (the ~30% caveat below); this baseline came in ~50 MB under the previous one without a code-attributable cause, so treat it as the low end of the observed range. The 570 KB number is dominated by each Sandbox's own Wasm Instance, its linear memory, and the per-channel WASI capture pipes (stdout/stderr); the Engine and the compiled Module are shared at process scope and not re-paid per Sandbox.

The B-35 `memory_peak` column makes the guest's contribution to each row directly attributable. `8b` `memory_peak = 0` per nil-returning eval confirms the per-invocation reset reaches the linear-memory layer (RSS drift here is purely allocator page retention, not guest-side leakage). `8c` `memory_peak = 2.5 MiB` against a +3.6 MB RSS jump means the guest's own `memory.grow` for the 512 KiB String accounts for most of it, with ~1 MB of host-side allocator slack on top. `8d` `memory_peak = 64 KiB` says the 2 MiB-attempted stdout write barely touched guest linear memory at all — the bytes flow through the WASI pipe; the +4.2 MB RSS is the host-side capture buffer plus allocator slack.

The `8d` peak / retained numbers fluctuate run-to-run depending on whether the allocator already holds pages large enough to fit the 1 MiB capture buffer. The cap itself is honored regardless: `stdout_truncated?` flips to `true` and the captured buffer ends at the 1 MiB boundary regardless of how much the guest tried to write. A persistent jump in this row across runs would indicate the capture buffer is growing without bound.

The `8b` per-invocation drift remains bounded — 2.3 MB over 10 000 invocations, in line with allocator page retention. B-15 / B-19 per-invocation reset is honored at both the Ruby level (Catalog::Handles counter, capture buffers) and the linear-memory level (`memory_peak = 0` per call).

## What changed vs previous baseline

This section is the diff against the *immediately previous* baseline — it is replaced (not appended) every time the Latest baseline above is refreshed. Pre-history lives in git (`benchmark/results/<date>-<sha>.json` files) and in release-tagged `benchmark/<semver>` annotated tags.

**Previous baseline:** `1be572c`, 2026-05-22. **This baseline:** `711665d`, 2026-05-27 (the 0.5.0 release line).

The 0.5.0 cycle was overwhelmingly internal refactoring — the `RPC`→`Transport` and `HandleTable`/`Binding`→`Catalog::{Handles,Namespaces}` renames, the three-class error taxonomy, the codec `Encode`/`Decode` trait lift, and envelope self-encoding. None of it touched the wire bytes or the hot paths, so every gated suite is flat within noise. The structural changes are in the suite roster and JSON keys, not the numbers.

**Roster / schema changes:**

- **New gated benchmark #6 — `yield_roundtrip`.** SPEC.md grew a sixth regression benchmark (host-initiated yield re-entry: `YieldResponse` codec + `__kobako_yield_to_block` + guest `BLOCK_STACK`) and this baseline records its numbers for the first time ([`feat(bench)`: add yield round-trip suite as gated benchmark #6](https://github.com/elct9620/kobako/commit/315f923), [`docs(spec)`: gate yield round-trip latency](https://github.com/elct9620/kobako/commit/c1d5559)). Steady-state per-yield re-entry is ~3.9 µs (`6c` `wall_time / 1000`).
- **Suite keys renamed; characterization renumbered.** The `RPC`/`HandleTable` rename family repointed the JSON suite keys: `rpc_roundtrip` → `transport_roundtrip`, `handle_table` → `catalog_handles`. With the yield suite taking #6, the three characterization suites shifted up one — concurrent #6 → **#7**, memory #7 → **#8**, `preload_dispatch` #8 → **#9** ([`refactor(bench)`: renumber characterization suites to 7/8/9](https://github.com/elct9620/kobako/commit/98798c0)). The `1be572c` JSON still carries the old keys/numbers; cross-baseline label diffs are this rename, not a moved case.
- **JSON grew `ips_mean` and `wall_time_sd`.** `ips_mean` (rendered `mean=…`) rides along for the capacity reading; `wall_time_sd` (rendered `±…%` next to `wall=`) gives the gate a dispersion band on the gate-correct metric ([`refactor(bench)`: report median ips + median usage](https://github.com/elct9620/kobako/commit/5034215), [`feat(bench)`: add noise-aware release gate, report mean alongside median](https://github.com/elct9620/kobako/commit/0cfaebc)).

**Performance — flat across the board:**

- **All five pre-existing gated suites hold within noise.** Codec host/guest, transport round-trip, mruby VM, and Catalog::Handles are unchanged vs `1be572c` (e.g. `2a-empty-call` 136 → 136 µs, `5a-alloc-100_000` 47.90 → 44.24 ms, `4a-arith-100k` 43.89 → 42.83 ms — all within a few percent and several slightly faster). The codec byte-for-byte invariant is the load-bearing observation: the `Encode`/`Decode` trait lift and envelope self-encode refactors changed call structure, not emitted bytes, and the codec numbers confirm it. **No release-gate +10% trip this cycle.**
- **The prior baseline's `4e` / `4f` / `5a` / `5b` shifts are now the steady level**, not moving numbers. The `dfd14db` IO-write-into-Rust step (+24% on `4e`) and the `14b8754` Handle-object alloc step (3-4× on `5a`/`5b`) were both absorbed at `1be572c`; this baseline reproduces them flat.

**Memory — first-Sandbox RSS came in ~50 MB lower, no code cause:**

- `8a-rss-after-1-sandbox` 191.3 → 137.9 MB (and boot RSS 27.7 → 25.3 MB). Nothing in the 0.5.0 refactor changes the compiled module or the wasmtime Engine/Module JIT that dominates this figure, so the drop is host process load / allocator state — a ~28% swing, inside the documented ~30% RSS variance caveat. Per-additional-Sandbox RSS holds at ~570 KB and the 1 000-tenant total tracks down proportionally (764 → 705 MB). Treat first-Sandbox RSS as a range, not a fixed number.

**Note on `1b` `wall_time`.** The single-sample `annotate_usage!` reading on `1b-sandbox-new+eval-nil` moved 135 → 257 µs. This is not a regression and not reliable: it samples one fresh-Sandbox first `#eval` with no dispersion, and 257 µs is internally impossible (it exceeds `1b`'s total minus the `Sandbox.new` term). The warm-reuse `2a` median (`wall_time` 127 µs) is the correct basis for the guest/host split; see [Reusing a Sandbox vs constructing one per request](#reusing-a-sandbox-vs-constructing-one-per-request).

For the previous (`19e51d9` → `1be572c`) diff — the B-35 schema addition, `5a`/`5b` Handle-object alloc, and `4e`/`4f` IO-write step — see git history.

## Running

```bash
bundle exec rake bench             # six gated benchmarks (CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full              # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent        # multi-Thread characterization (#7)
bundle exec rake bench:memory            # per-Sandbox RSS characterization (#8)
bundle exec rake bench:preload_dispatch  # #preload + #run characterization (#9)
```

Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; you can also invoke a single script directly for fast iteration:

```bash
bundle exec ruby benchmark/transport_roundtrip.rb
```

Total wall time for `bundle exec rake bench` is roughly 5-8 minutes on a current-gen laptop (codec dominates with 46 cases × 3 s warmup + 3 s measurement); each characterization task adds 30 s to 1 minute.

YJIT is not turned on by the suite. Invoke with `RUBY_YJIT_ENABLE=1 bundle exec rake bench` or `bundle exec ruby --yjit benchmark/<file>.rb` to capture a YJIT baseline; the resulting JSON records `yjit_enabled: true` so it is unambiguously distinct from a non-YJIT baseline.

## Result files

Every run writes (or merges into) `benchmark/results/<date>-<short-sha>.json`:

```json
{
  "env": {
    "ruby_version": "3.4.7",
    "ruby_platform": "arm64-darwin24",
    "processors": 16,
    "yjit_enabled": false,
    "git_sha": "711665d",
    "captured_at": "2026-05-27T14:20:41Z"
  },
  "suites": {
    "cold_start":          [ { "label": "1a-sandbox-new", "ips": 8013.0, "ips_mean": 7956.4, "ips_sd": 112, "iterations": 18432, "cycles": 3 } ],
    "transport_roundtrip": [ { "label": "2a-empty-call",  "ips": 7365.3, "ips_mean": 7357.6, "ips_sd": 15,  "iterations": 24576, "cycles": 3,
                               "wall_time": 0.0001273, "wall_time_sd": 0.0000191, "memory_peak": 0 } ],
    ...
  }
}
```

- **`ips`** — iterations per CPU second (the **median** of the per-cycle samples); higher is better.
- **`ips_mean`** — arithmetic mean of the per-cycle `ips` samples; rides along for the capacity / throughput reading next to the median.
- **`ips_sd`** — standard deviation of the per-cycle `ips` samples; report as a percentage of `ips`.
- **`iterations`** / **`cycles`** — total iterations measured and number of samples; small `cycles` means few samples were collected within the time budget (high per-iter cost), and the corresponding `ips_sd` should be read accordingly.
- **`seconds`** — appears on one-shot entries (cold construction, large-table allocs, concurrent measurements) where iterating would mask the cold-path cost. CPU seconds for the `case`/`one_shot` runners; wall-clock seconds for the multi-thread suite.
- **`wall_time`** / **`wall_time_sd`** / **`memory_peak`** — present on sandbox-driven `ips` rows ([SPEC.md B-35](../SPEC.md)). `wall_time` is the Float seconds the guest export call spent inside wasmtime; on `case_with_usage` rows it is the **median** of a dedicated post-measurement sampling loop, and `wall_time_sd` is its standard deviation (the gate's noise band). `memory_peak` is the Integer high-water `memory.grow` delta past the per-invocation baseline. `wall_time` / `memory_peak` reflect *one* invocation, not the loop total — combine with `ips` for steady-state interpretation. (Rows annotated via the one-shot `annotate_usage!` path — `1b` — carry a single `wall_time` sample with no `wall_time_sd`; read those cautiously.)

Release baselines are additionally marked with annotated git tags following `benchmark/<semver>` (per SPEC.md).

## Release gate

`rake bench:gate[current,baseline]` compares a fresh run against the previous release baseline and flags the gated cases that regressed; it exits non-zero so CI fails the release on a real regression. With no arguments it diffs the two newest files under `benchmark/results/`. The comparison logic lives in `tasks/support/kobako_bench_gate.rb`.

A case is flagged only when its regression clears **both** a relative floor (**+10 %**) **and** a noise band of `2 × √(cv_current² + cv_baseline²)` (the combined coefficient of variation across the two runs). The floor is the conservative backstop; the noise band can only *widen* the bar on high-variance rows, never narrow it below the floor — so the gate never flags more than a bare +10 % rule would. It only *suppresses* flags on demonstrably noisy rows, which is exactly what the false 512 KiB "codec drop" needed.

**The gate reads the metric each row is actually about.** Rows carrying `wall_time` (sandbox-driven) are judged on `wall_time` — the machine-load-insensitive guest budget, where a slowdown is a larger value. Pure host rows (`3a-host-decode-*` / `3a-host-encode-*`) are judged on the median `ips`. The guest-return rows' host wrapper (`1/ips − wall_time`) is GC/allocator-bound on the largest payloads — a fresh 512 KiB Ruby String per iteration makes it swing with host GC timing while the guest compute is flat — so it is **characterization, not a gate signal**. One-shot / cold-path rows carry no dispersion and are skipped.

The three characterization suites (`#7` multi-Thread, `#8` memory, `#9` `#preload` + `#run` dispatch) are informational and not part of the gate, but baselines are recorded so before/after comparison is possible when changes touch the GVL boundary (e.g. introducing `rb_thread_call_without_gvl` in `ext/`), the per-Sandbox memory model, or the snippet preload / dispatch path.

## Known caveats when reading results

- **Guest String size cap at 1 MiB.** `MRB_STR_LENGTH_MAX` is 1 MiB by mruby default; the guest-side codec cases stop at 512 KiB. The wire payload limit (16 MiB) is reachable only through composite values.
- **Aggregate throughput is GVL-bounded.** Multi-Thread scaling stays close to flat because `ext/` does not release the GVL during wasmtime execution. Genuine wasm parallelism would require introducing `rb_thread_call_without_gvl` on the host side.
- **One-shot timings are sensitive to filesystem cache.** The first `Sandbox.new` reads `data/kobako.wasm` from disk and JIT-compiles the Module. Numbers can vary 5-10 % between a cold OS page cache and a hot one — record both states when investigating a regression in the first-construction number.
- **Per-suite ordering matters.** Several rows (`5c`, `8d`) are explicitly sensitive to GC / allocator state built up by earlier cases in the same process. Re-running a single case in isolation will produce different numbers than running it as part of `rake bench`. The published baseline reflects the in-suite numbers.
- **`ips` is steady-state.** Cold-path costs that only occur once per process (Engine init, Module compile) are captured via `one_shot` entries (`1c-sandbox-new-1`), not the `ips` cases. Watch the right metric for the question you are asking.
