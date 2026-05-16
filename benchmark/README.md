# Benchmarks

Kobako maintains a regression benchmark suite covering the five performance dimensions [SPEC.md](../SPEC.md) names as release quality gates (startup, RPC round-trip, codec, mruby VM, HandleTable) plus two characterization suites (multi-thread behaviour, per-Sandbox RSS). Baselines for every release live under `benchmark/results/` so subsequent runs can diff against a known point; a +10% regression on any of the five gated benchmarks requires explicit review before release.

## Latest baseline

Captured on **2026-05-16** at commit `c605109` — macOS arm64, Ruby 3.4.7, 16 CPUs, YJIT off. Numbers below are typical; absolute values vary by hardware, but the relative shape (cold/warm ratio, RPC overhead, scaling curves) is consistent across machines.

How the numbers are obtained:

- **`ips` cases** — the runner calibrates an iteration count that lands ~1/5 of the time budget per cycle, runs a warmup pass with the same iteration shape, then records CPU time per cycle until the budget is exhausted. `ips` is the mean of per-cycle samples; `±ips_sd` is the sample standard deviation as a percentage. CPU time excludes scheduler / background-load noise, so the same code on the same machine reproduces within the reported `±ips_sd`.
- **`one_shot` cases** — the block runs exactly once and the CPU seconds consumed are recorded. Used for cold-path costs (the very first `Sandbox.new` in a process, large-table allocations) where iterating would only ever observe the warm path.
- **Multi-thread cases** keep their own wall-clock helper and bypass the runner, because measuring scheduler overhead by CPU time would defeat the purpose.

### Sandbox construction and first run ([`cold_start.rb`](cold_start.rb))

Two costs dominate the very first `Kobako::Sandbox` in a process: wasmtime Engine initialization and `data/kobako.wasm` Module JIT compile. Both are cached at process scope, so every subsequent `Sandbox.new` in the same process is orders of magnitude cheaper.

| Scenario | Latency |
|---|---|
| First `Sandbox.new` in a fresh process | **1.98 s** |
| Second-through-tenth `Sandbox.new` (cache warm) | **~0.11 ms** each |
| Steady-state `Sandbox.new` only | **128 µs** |
| Steady-state `Sandbox.new` + first `#run("nil")` | **275 µs** |

The first-Sandbox cost is dominated by wasmtime JIT compiling the Module on macOS arm64. The Module is sizeable today because the guest binary embeds the mruby interpreter, the `mruby-onig-regexp` Onigmo engine, and the precompiled `mrblib/io.rb` + `mrblib/kernel.rb` IO preamble; each of those is a feature commitment the cold-start cost pays for once per process.

Practical implication: pre-warm by constructing one Sandbox at boot. After that, every per-request Sandbox costs micro-, not seconds.

### Reusing a Sandbox vs constructing one per request

| Pattern | Cost per request | Source |
|---|---|---|
| Reuse the same Sandbox (`#run("nil")` on a warm instance) | **134 µs** | `2a-empty-rpc` |
| Fresh Sandbox every request (`Kobako::Sandbox.new.run("nil")`) | **275 µs** | `1b-sandbox-new+run-nil` |
| Overhead of constructing a new Sandbox per request | **~141 µs** | difference |

Per-request construction does NOT pay the multi-second Engine/Module cold cost again — that is amortized to the first Sandbox in the process regardless of pattern. The per-request overhead is the `Sandbox.new` work itself (Wasm instance creation, capture-buffer allocation, RPC Server init).

Practical implication: choose per-request construction when guest scripts are mutually untrusted; choose reuse when a single Sandbox serves repeated requests from the same trust scope. At ~140 µs of extra overhead per request, per-request isolation is affordable for most web/job workloads.

### Per-request RPC latency ([`rpc_roundtrip.rb`](rpc_roundtrip.rb))

Each row wraps the call inside one `#run`, so the absolute number bundles `#run` setup (~130 µs) with the RPC.

| Scenario | Latency |
|---|---|
| One Service call returning `nil`, alone in `#run` | **134 µs** |
| One Service call with one Integer arg | **134 µs** |
| One Service call with one Symbol-keyed keyword arg | 620 µs (n=1, see note) |
| 1 000 sequential Service calls inside one `#run` | 35 ms total → 35 µs per RPC |
| Handle chain — one RPC returns object, second targets the Handle | 734 µs |

`2c-kwargs`, `2d-1000-rpcs-in-one-run`, and `2e-handle-chain` came in noticeably slower than the empty-RPC baseline this capture. The empty-RPC numbers (`2a`, `2b`) reproduce cleanly and are the load-bearing per-RPC cost figure (~134 µs / call); the higher rows reflect a combination of per-call codec work (kwargs goes through Symbol ext 0x00), GC pressure accumulated by earlier cases in the same process, and small sample counts. Track as a runner / suite-isolation follow-up.

The Handle-chain row exercises [SPEC.md B-17](../SPEC.md): a Factory Service returns a host object → kobako allocates a Handle on the return path → the guest invokes a method against the Handle → kobako fetches the host object via the Handle. The cost above the empty-RPC baseline is the second RPC plus one `HandleTable#alloc` and one `HandleTable#fetch`.

### Wire codec — host side ([`codec.rb`](codec.rb))

Encoding and decoding through `Kobako::Codec` directly from Ruby. These numbers do not cross the wasm boundary; they characterize the host-side codec on its own.

| Payload | Encode | Decode |
|---|---|---|
| String, 64 B | 2.4 µs | 2.8 µs |
| String, 1 KiB | 1.2 µs (see note) | 2.9 µs (n=1) |
| String, 64 KiB | 41 µs | 9.2 µs |
| String, 1 MiB | 257 µs | 98 µs |
| Array nested 1 deep (1 KiB leaf) | 2.9 µs | 3.4 µs |
| Array nested 64 deep (1 KiB leaf) | 4.2 µs | 34 µs |

Per-wire-type micro-bench at primitive sizes, one entry per SPEC.md Type Mapping row (12 entries):

| Wire type | Encode | Decode |
|---|---|---|
| `nil` / Boolean / Integer / Float | 440-470 k ips (~2.2 µs) | 410-470 k ips (~2.3 µs) |
| Short String / binary String | 1.85 M ips (~540 ns) | 1.7-1.8 M ips (~580 ns) |
| 3-element Array / 1-entry Hash | 1.8 M ips (~550 ns) | 1.1-1.2 M ips (~870 ns) |
| Symbol (ext 0x00) | 1.61 M ips (623 ns) | 1.35 M ips (743 ns) |
| Handle (ext 0x01) | 1.52 M ips (658 ns) | 941 k ips (1.1 µs) |
| Exception envelope (ext 0x02) | 760 k ips (1.3 µs) | 338 k ips (3.0 µs) |

`3a-host-encode-1KiB` had a single noisy outlier (`±57.5%`) this capture and `3a-host-decode-1KiB` only fit one measurement cycle (n=1); both are sensitive to allocator state at that point in the suite. The neighbouring rows (`3a-host-{encode,decode}-{64B,64KiB,1MiB}`) reproduce stably and are the load-bearing String codec numbers.

### Wire codec — guest side ([`codec.rb`](codec.rb))

The guest builds a value in mruby and returns it from `#run`. The absolute numbers bundle guest encode + host decode + the constant `#run` overhead; cross-row differences isolate the codec contribution.

| Guest script returns | Latency |
|---|---|
| `"x" * 64` (64 B String) | 121 µs |
| `"x" * 1024` (1 KiB String) | 121 µs |
| `"x" * 65536` (64 KiB String) | 154 µs |
| `"x" * 524288` (512 KiB String) | 397 µs |
| Array nested 1 deep (1 KiB leaf) | 123 µs |
| Array nested 64 deep (1 KiB leaf) | 137 µs |

Note: guest mruby caps a single String at 1 MiB ([SPEC Invariant](../SPEC.md)); the largest guest sample here is 512 KiB. Composite values (Arrays, Hashes) can still approach the 16 MiB wire payload limit.

### mruby VM, no RPC ([`mruby_eval.rb`](mruby_eval.rb))

Pure interpreter work — every script is a self-contained mruby computation whose only host cost is the constant `Sandbox#run` overhead. Useful for spotting regressions in `build_config/wasi.rb` flag changes.

| Script | Latency |
|---|---|
| 100 000-iteration integer XOR loop | **203 ms** |
| 1 000 single-character String appends | 2.8 ms |
| 100 cycles of `raise` / `rescue` | 1.4 ms → 14 µs per cycle |
| 1 000 Onigmo `Regexp =~` matches | 14 ms → 14 µs per match |
| 1 000 `puts` of 64 B (below 1 MiB stdout cap) | 19 ms → 19 µs per write |
| 2 048 `puts` of ~1 KiB against the 1 MiB stdout cap | 42 ms (first ~1 024 land, rest silently dropped) |

The `4d` / `4e` / `4f` rows cover features that landed since `0.1.2`: Onigmo `Regexp` via `mruby-onig-regexp`, the full B-04 IO surface (`puts` / `print` / `printf` / `p` / `$stdout` / `$stderr` wired through to a host-captured WASI pipe), and the per-channel `stdout_limit` cap on that capture buffer. The cap is honored: guest `puts` does not raise on rejection, the pipe returns short, the loop runs to completion, and `sandbox.stdout_truncated?` is `true` after the run.

### Handle table scaling ([`handle_table.rb`](handle_table.rb))

`HandleTable` is the host-side mapping from opaque integer IDs to Ruby objects, reset at the start of every `#run`. These numbers verify the underlying Hash stays O(1) as it grows.

| Scenario | Latency |
|---|---|
| Allocate one Handle in an empty table | 1.1 µs |
| Allocate 100 Handles from empty | 59 µs total |
| Allocate 10 000 Handles from empty | 5.9 ms total |
| Allocate 100 000 Handles from empty | 66 ms total |
| 1 000 allocs against a 1 K-entry table | 0.35 ms |
| 1 000 allocs against a 10 K-entry table | 0.41 ms |
| 1 000 allocs against a 100 K-entry table | 0.61 ms |
| 1 000 allocs against a 1 M-entry table | 0.57 ms |
| Warm `Sandbox#run("nil")` round-trip (includes per-run reset) | 515 µs |

The 1 K to 1 M waypoint rows confirm the dictionary stays effectively flat as the table grows — per-alloc cost holds around 350-610 ns across four orders of magnitude. ([SPEC.md B-21](../SPEC.md) caps the counter at `0x7fff_ffff` and rejects allocation past the cap; the cap guard itself is constant-time and not iterated here.)

The `5c-warm-run-nil-roundtrip` row is the slowest case in this suite by an order of magnitude. That number is GC-amplified — it runs after `5b` has grown a 1 M-entry HandleTable that stays alive in the same Ruby process, so each measured `#run` allocates capture-buffer Strings under heavy heap pressure. The cleaner per-`#run` cost is the `1b-sandbox-new+run-nil` 275 µs from cold_start; `5c` is preserved here as the regression guard against changes that make `#run` more GC-sensitive than today.

### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb)) — characterization only

`ext/` does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is GVL-serialized. Ruby-side `#run` setup can still overlap, which is why throughput scales modestly rather than not at all. This suite uses wall-clock timing because that is what scheduler effects manifest in.

| Scenario | Result |
|---|---|
| 1 Thread, owning one Sandbox | ~14k `#run`/s |
| 2 Threads, each owning one Sandbox | ~14k `#run`/s (essentially flat) |
| 4 Threads, each owning one Sandbox | ~14k `#run`/s |
| 8 Threads, each owning one Sandbox | ~14k `#run`/s |
| Per-Sandbox `Sandbox.new` cost, single-Threaded | 0.146 ms |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel | 0.113 ms (no mutex contention on the cache) |
| `#run("nil")` baseline | 0.067 ms |
| `#run("nil")` while another Thread is in a long `#run` | 0.190 ms (1.5-3× baseline depending on the OS scheduler) |

Practical implication for Sidekiq / Puma cluster shapes: a long-running script does NOT block other Threads' short `#run` calls by hundreds of milliseconds. The contention overhead is bounded because any host-side synchronization (Queue push from a Service callback, mutex acquisition, IO) yields the GVL and lets the contending Thread interleave. The exact ratio varies run-to-run (1.5-3×) with scheduler quirks; the order of magnitude is the regression signal.

### Memory cost ([`memory.rb`](memory.rb)) — characterization only

External RSS sampling (`ps -o rss=`) — we only observe what the host process consumes, never look inside the Sandbox's mruby heap or Wasm linear memory. This is the right granularity for capacity planning (how many tenants fit in one process) without violating SPEC's Non-Goal of per-`#run` instrumentation.

| Scenario | Result |
|---|---|
| Process RSS at boot (no Sandbox) | 25 MB |
| RSS after the first `Sandbox.new` + `#run("nil")` | 152 MB (**+127 MB** — Engine init + Module JIT + 1 instance, one-time) |
| RSS after 10 Sandboxes total | 158 MB (~560 KB per additional Sandbox) |
| RSS after 100 Sandboxes total | 208 MB (~560 KB per additional Sandbox) |
| RSS after 1 000 Sandboxes total | 726 MB (~**575 KB per additional Sandbox**) |
| RSS drift after 10 000 consecutive `#run("nil")` on one Sandbox | +208 KB over the whole run (~0.02 KB / run; consistent with allocator page retention, not a B-15 / B-19 violation) |
| Peak RSS while holding a 512 KiB return value | +2.6 MB above baseline |
| Retained RSS after GC of the same value | +2.6 MB (allocator does not eagerly return pages to the OS; the Ruby reference is dropped) |
| Peak RSS while holding a 1 MiB capped stdout buffer | +64 KB above baseline (allocator-state-dependent — see note) |
| Retained RSS after GC of the same capture | -688 KB (allocator freed pages from earlier captures) |

Practical implication for tenant isolation: budget ~130 MB up front per worker process (paid by the first Sandbox), plus ~575 KB per concurrent tenant. **1 000 tenants ≈ 730 MB** in one Ruby process — comfortably within a typical Sidekiq / Puma worker's RSS limit. The 575 KB number is dominated by each Sandbox's own Wasm Instance, its linear memory, and the per-channel WASI capture pipes (stdout/stderr); the Engine and the compiled Module are shared at process scope and not re-paid per Sandbox.

The `7d` peak / retained numbers fluctuate run-to-run depending on whether the allocator already holds pages large enough to fit the 1 MiB capture buffer. The cap itself is honored regardless: `stdout_truncated?` flips to `true` and the captured buffer ends at the 1 MiB boundary regardless of how much the guest tried to write. A persistent jump in this row across runs would indicate the capture buffer is growing without bound.

The `7b` per-`#run` drift remains bounded — 208 KB over 10 000 runs, in line with allocator page retention. B-15 / B-19 per-`#run` reset is still honored at the Ruby level.

## What changed vs previous baseline

This section is the diff against the *immediately previous* baseline — it is replaced (not appended) every time the Latest baseline above is refreshed. Pre-history lives in git (`benchmark/results/<date>-<sha>.json` files) and in release-tagged `benchmark/<semver>` annotated tags.

**Previous baseline:** `deb7c9d`, 2026-05-16. **This baseline:** `c605109`, 2026-05-16. Both use the same kobako code; the runner is the source of every difference below, so nothing here attributes to a kobako design change.

The previous baseline was captured under a measurement methodology that occasionally inflated `ips` on short-lived cases by 4-5× when Ruby's process state was favourable. The new methodology eliminates that variance: the gated cases that were stable across runs (`1a`, `1b`, `2a`, `2b`, the codec primitive types, the HandleTable allocation curve, the per-Sandbox RSS cost) all land within ±5% of the previous numbers, and the cases that previously fluctuated wildly are now stable:

| Case | previous ±SD | this baseline ±SD |
|---|---|---|
| `4f-stdout-cap-saturation` | ±68 % | ±4 % |
| `5a-alloc-100-from-empty` | ±57 % | ±2 % |
| `5a-alloc-10_000-from-empty` | ±16 % | ±2 % |
| `5c-warm-run-nil-roundtrip` | ±10 % | ±4 % |

A few cases ran with low sample counts and high variance this capture (`2c-kwargs`, `3a-host-encode-1KiB`, `3a-host-decode-1KiB`) — flagged inline in their respective sections. The empty-RPC and codec-primitive rows reproduce cleanly and remain the load-bearing reference points.

For the previous (`deb7c9d` vs `f4da86e`) diff — covering the post-0.1.2 IO / caps / Regexp feature lines that shifted absolute numbers — see git history.

## Running

```bash
bundle exec rake bench             # five gated benchmarks (CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full        # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent  # multi-Thread characterization
bundle exec rake bench:memory      # per-Sandbox RSS characterization
```

Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; you can also invoke a single script directly for fast iteration:

```bash
bundle exec ruby benchmark/rpc_roundtrip.rb
```

Total wall time for `bundle exec rake bench` is roughly 5-8 minutes on a current-gen laptop (codec dominates with 46 cases × 3 s warmup + 3 s measurement); `rake bench:concurrent` adds ~30 s.

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
    "git_sha": "c605109",
    "captured_at": "2026-05-16T13:48:01Z"
  },
  "suites": {
    "cold_start":   [ { "label": "1a-sandbox-new", "ips": 7790.5, "ips_sd": 304, "iterations": 5760, "cycles": 3 } ],
    "rpc_roundtrip": [ ... ],
    ...
  }
}
```

- **`ips`** — iterations per CPU second; higher is better.
- **`ips_sd`** — standard deviation of the per-cycle `ips` samples; report as a percentage of `ips`.
- **`iterations`** / **`cycles`** — total iterations measured and number of samples; small `cycles` means few samples were collected within the time budget (high per-iter cost), and the corresponding `ips_sd` should be read accordingly.
- **`seconds`** — appears on one-shot entries (cold construction, large-table allocs, concurrent measurements) where iterating would mask the cold-path cost. CPU seconds for the `case`/`one_shot` runners; wall-clock seconds for the multi-thread suite.

Release baselines are additionally marked with annotated git tags following `benchmark/<semver>` (per SPEC.md).

## Release gate

A regression greater than **+10 %** on any of the five gated benchmarks (startup, RPC, codec, mruby VM, HandleTable) versus the previous release baseline requires explicit review and approval before release proceeds.

The multi-Thread benchmark is informational — its results depend on the OS scheduler and are not part of the gate, but baselines are recorded so before/after comparison is possible when changes touch the GVL boundary (e.g. introducing `rb_thread_call_without_gvl` in `ext/`).

## Known caveats when reading results

- **Guest String size cap at 1 MiB.** `MRB_STR_LENGTH_MAX` is 1 MiB by mruby default; the guest-side codec cases stop at 512 KiB. The wire payload limit (16 MiB) is reachable only through composite values.
- **Aggregate throughput is GVL-bounded.** Multi-Thread scaling stays close to flat because `ext/` does not release the GVL during wasmtime execution. Genuine wasm parallelism would require introducing `rb_thread_call_without_gvl` on the host side.
- **One-shot timings are sensitive to filesystem cache.** The first `Sandbox.new` reads `data/kobako.wasm` from disk and JIT-compiles the Module. Numbers can vary 5-10 % between a cold OS page cache and a hot one — record both states when investigating a regression in the first-construction number.
- **Per-suite ordering matters.** Several rows (`5c`, `7d`) are explicitly sensitive to GC / allocator state built up by earlier cases in the same process. Re-running a single case in isolation will produce different numbers than running it as part of `rake bench`. The published baseline reflects the in-suite numbers.
- **`ips` is steady-state.** Cold-path costs that only occur once per process (Engine init, Module compile) are captured via `one_shot` entries (`1c-sandbox-new-1`), not the `ips` cases. Watch the right metric for the question you are asking.
