# Benchmarks

Kobako maintains a regression benchmark suite covering the five performance dimensions [SPEC.md](../SPEC.md) names as release quality gates (startup, RPC round-trip, codec, mruby VM, HandleTable) plus one extra concurrency characterization. Baselines for every release live under `benchmark/results/` so subsequent runs can diff against a known point; a +10% regression on any of the five gated benchmarks requires explicit review before release.

## Latest baseline

Captured on **2026-05-13** at commit `55ee78b` — macOS arm64, Ruby 3.4.7, 16 CPUs. Numbers below are typical; absolute values vary by hardware, but the relative shape (cold/warm ratio, RPC overhead, scaling curves) is consistent across machines.

### Sandbox construction and first run ([`cold_start.rb`](cold_start.rb))

Two costs dominate the very first `Kobako::Sandbox` in a process: wasmtime Engine initialization and `data/kobako.wasm` Module JIT compile. Both are cached at process scope, so every subsequent `Sandbox.new` in the same process is orders of magnitude cheaper.

| Scenario | Latency |
|---|---|
| First `Sandbox.new` in a fresh process | **408 ms** |
| Second-through-tenth `Sandbox.new` (cache warm) | **~0.07 ms** each (~5 800× faster) |
| Steady-state `Sandbox.new` only | **88 µs** |
| Steady-state `Sandbox.new` + first `#run("nil")` | **174 µs** |

Practical implication: pre-warm by constructing one Sandbox at boot. After that, every per-request Sandbox costs micro-, not milliseconds.

### Reusing a Sandbox vs constructing one per request

Two execution shapes dominate real use. The first is the "setup-once, run-many" pattern from [README.md](../README.md): one Sandbox per scope, many `#run` calls on it. The second is the per-request construction pattern needed for hard tenant isolation (one Sandbox per request, per submission, or per untrusted script — the shape of journeys J-03 and J-04 in SPEC.md), where every request pays the construction cost.

| Pattern | Cost per request | Source |
|---|---|---|
| Reuse the same Sandbox (`#run("nil")` on a warm instance) | **66 µs** | `5c-warm-run-nil-roundtrip` |
| Fresh Sandbox every request (`Kobako::Sandbox.new.run("nil")`) | **174 µs** | `1b-sandbox-new+run-nil` |
| Overhead of constructing a new Sandbox per request | **~108 µs / req (~2.6×)** | difference |

The overhead breaks down as ~88 µs `Sandbox.new` (Wasm instance creation, buffer allocation, Registry init — Engine and Module are cached at process scope so this is the warm path) plus the per-`#run` setup that both patterns pay. Per-request construction does NOT pay the 408 ms cold Engine/Module cost again — that cost is amortized to the first Sandbox in the process regardless of pattern.

Practical implication: choose per-request construction when guest scripts are mutually untrusted (so capability state and Handle leaks between requests are unacceptable); choose reuse when a single Sandbox serves repeated requests from the same trust scope. At ~108 µs of extra overhead per request, per-request isolation is affordable for most web/job workloads.

### Per-request RPC latency ([`rpc_roundtrip.rb`](rpc_roundtrip.rb))

Each row wraps the call inside one `#run`, so the absolute number bundles `#run` setup (~75 µs) with the RPC. The last row amortizes that overhead by making 1000 calls inside a single `#run`, which is the right number to compare against.

| Scenario | Latency |
|---|---|
| One Service call returning `nil`, alone in `#run` | **76 µs** |
| One Service call with one Integer arg | **78 µs** |
| One Service call with one Symbol-keyed keyword arg | **80 µs** |
| 1 000 sequential Service calls inside one `#run` | 5.31 ms total → **5.3 µs per RPC** |

### Wire codec — host side ([`codec.rb`](codec.rb))

Encoding and decoding through `Kobako::Wire::Codec` directly from Ruby. These numbers do not cross the wasm boundary; they characterize the host-side codec on its own.

| Payload | Encode | Decode |
|---|---|---|
| String, 64 B | 384 ns | 427 ns |
| String, 1 KiB | 498 ns | 475 ns |
| String, 64 KiB | 7.1 µs | 2.7 µs |
| String, 1 MiB | 60 µs | 36 µs |
| Array nested 1 deep (1 KiB leaf) | 506 ns | 578 ns |
| Array nested 64 deep (1 KiB leaf) | 990 ns | 8.4 µs |

Per-wire-type micro-bench at primitive sizes (mostly to detect a regression in a single type's codec path; absolute numbers cluster around 380 ns):

| Wire type | Encode | Decode |
|---|---|---|
| `nil` / Boolean / Integer / Float | ~380 ns | ~370 ns |
| Short String / binary String | ~383 ns | ~400 ns |
| 3-element Array / 1-entry Hash | ~400 ns | ~625 ns |
| Symbol (ext 0x00) | 453 ns | 530 ns |
| Handle (ext 0x01) | 504 ns | 874 ns |
| Exception envelope (ext 0x02) | 962 ns | 2.2 µs |

### Wire codec — guest side ([`codec.rb`](codec.rb))

The guest builds a value in mruby and returns it from `#run`. The absolute numbers bundle guest encode + host decode + the constant `#run` overhead; cross-row differences isolate the codec contribution.

| Guest script returns | Latency |
|---|---|
| `"x" * 64` (64 B String) | 68 µs |
| `"x" * 1024` (1 KiB String) | 69 µs |
| `"x" * 65536` (64 KiB String) | 99 µs |
| `"x" * 524288` (512 KiB String) | 311 µs |
| Array nested 1 deep (1 KiB leaf) | 82 µs |
| Array nested 64 deep (1 KiB leaf) | 104 µs |

Note: guest mruby caps a single String at 1 MiB ([SPEC Invariant](../SPEC.md)); the largest guest sample here is 512 KiB. Composite values (Arrays, Hashes) can still approach the 16 MiB wire payload limit.

### mruby VM, no RPC ([`mruby_eval.rb`](mruby_eval.rb))

Pure interpreter work. Useful for spotting performance regressions in `build_config/wasi.rb` flag changes.

| Script | Latency |
|---|---|
| 100 000-iteration integer XOR loop | **44 ms** |
| 1 000 single-character String appends | 533 µs |
| 100 cycles of `raise` / `rescue` | 238 µs → **2.4 µs per cycle** |

### Handle table scaling ([`handle_table.rb`](handle_table.rb))

`HandleTable` is the host-side mapping from opaque integer IDs to Ruby objects, reset at the start of every `#run`. These numbers verify the underlying Hash stays O(1) as it grows.

| Scenario | Latency |
|---|---|
| Allocate one Handle in an empty table | 261 ns |
| Allocate 100 Handles from empty | 15 µs total |
| Allocate 10 000 Handles from empty | 1.4 ms total |
| Allocate 100 000 Handles from empty | 14.7 ms total |
| 1 000 allocs against a 1 K-entry table | 0.098 ms |
| 1 000 allocs against a 10 K-entry table | 0.092 ms |
| 1 000 allocs against a 100 K-entry table | 0.101 ms |
| 1 000 allocs against a 1 M-entry table | 0.141 ms |
| Warm `Sandbox#run("nil")` round-trip (includes per-run reset) | 66 µs |

The last four rows confirm the Hash stays effectively flat from 1 K to 1 M entries; the 40 % bump at 1 M is normal HashMap resize overhead.

### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb)) — characterization only

`ext/` does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is GVL-serialized. Ruby-side `#run` setup can still overlap, which is why throughput scales modestly rather than not at all.

| Scenario | Result |
|---|---|
| 1 Thread, owning one Sandbox | 10.2k `#run`/s |
| 2 Threads, each owning one Sandbox | 14.2k `#run`/s (1.4× single-Thread) |
| 4 Threads, each owning one Sandbox | 14.8k `#run`/s |
| 8 Threads, each owning one Sandbox | 14.2k `#run`/s |
| Per-Sandbox `Sandbox.new` cost, single-Threaded | 0.124 ms |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel | 0.112 ms (no mutex contention on the cache) |
| `#run("nil")` baseline | 0.068 ms |
| `#run("nil")` while another Thread is in a long `#run` | 0.103 ms (**1.5× baseline**) |

Practical implication for Sidekiq / Puma cluster shapes: a long-running script does NOT block other Threads' short `#run` calls by hundreds of milliseconds. The contention overhead is modest because any host-side synchronization (Queue push from a Service callback, mutex acquisition, IO) yields the GVL and lets the contending Thread interleave.

### Memory cost ([`memory.rb`](memory.rb)) — characterization only

External RSS sampling (`ps -o rss=`) — we only observe what the host process consumes, never look inside the Sandbox's mruby heap or Wasm linear memory. This is the right granularity for capacity planning (how many tenants fit in one process) without violating SPEC's Non-Goal of per-`#run` instrumentation.

| Scenario | Result |
|---|---|
| Process RSS at boot (no Sandbox) | 26 MB |
| RSS after the first `Sandbox.new` + `#run("nil")` | 154 MB (**+128 MB** — Engine init + Module JIT + 1 instance, one-time) |
| RSS after 10 Sandboxes total | 156 MB (~203 KB per additional Sandbox) |
| RSS after 100 Sandboxes total | 173 MB (~194 KB per additional Sandbox) |
| RSS after 1 000 Sandboxes total | 372 MB (~**218 KB per additional Sandbox**) |
| RSS drift after 10 000 consecutive `#run("nil")` on one Sandbox | +2.2 MB over the whole run (~0.2 KB / run; consistent with allocator page retention, not a B-15 / B-19 violation) |
| Peak RSS while holding a 512 KiB return value | +2.5 MB above baseline |
| Retained RSS after GC of the same value | +2.5 MB (allocator does not eagerly return pages to the OS; the Ruby reference is dropped) |

Practical implication for tenant isolation: budget ~128 MB up front per worker process (paid by the first Sandbox), plus ~200 KB per concurrent tenant. **1 000 tenants ≈ 330 MB** in one Ruby process — well within a typical Sidekiq / Puma worker's RSS limit. The 200 KB number is dominated by each Sandbox's own Wasm Instance and its linear memory; the Engine and the compiled Module are shared at process scope and not re-paid per Sandbox.

The 7b drift is allocator behaviour, not a real leak — the per-`#run` HandleTable reset is honored at the Ruby level; the residual RSS is malloc pages held for reuse. If a host operator needs to bound a long-lived process tightly, monitor RSS over wall-clock hours rather than per-run growth.

## Running

```bash
bundle exec rake bench             # five gated benchmarks (= bench:smoke; CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full        # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent  # multi-Thread characterization
bundle exec rake bench:memory      # per-Sandbox RSS characterization
```

Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; you can also invoke a single script directly for fast iteration:

```bash
bundle exec ruby benchmark/rpc_roundtrip.rb
```

Total wall time for `bundle exec rake bench` is roughly 5-7 minutes on a current-gen laptop (codec dominates with 46 cases × 3 s warm + 3 s measurement); `rake bench:concurrent` adds ~30 s.

## Result files

Every run writes (or merges into) `benchmark/results/<date>-<short-sha>.json`:

```json
{
  "env": {
    "ruby_version": "3.4.7",
    "ruby_platform": "arm64-darwin24",
    "processors": 16,
    "git_sha": "55ee78b",
    "captured_at": "2026-05-13T13:19:17Z"
  },
  "suites": {
    "cold_start":   [ { "label": "1a-sandbox-new", "ips": 11391.4, "ips_sd": 854, ... } ],
    "rpc_roundtrip": [ ... ],
    ...
  }
}
```

- **`ips`** comes from `benchmark-ips` — iterations per second; higher is better.
- **`ips_sd`** is the standard deviation across measurement cycles.
- **`seconds`** appears on one-shot entries (cold construction, large-table allocs, concurrent measurements) where iterating would mask the cold-path cost.

Release baselines are additionally marked with annotated git tags following `benchmark/<semver>` (per SPEC.md).

## Release gate

A regression greater than **+10%** on any of the five gated benchmarks (startup, RPC, codec, mruby VM, HandleTable) versus the previous release baseline requires explicit review and approval before release proceeds.

The multi-Thread benchmark is informational — its results depend on the OS scheduler and are not part of the gate, but baselines are recorded so before/after comparison is possible when changes touch the GVL boundary (e.g. introducing `rb_thread_call_without_gvl` in `ext/`).

## Known caveats when reading results

- **Guest String size cap at 1 MiB.** `MRB_STR_LENGTH_MAX` is 1 MiB by mruby default; the guest-side codec cases stop at 512 KiB. The wire payload limit (16 MiB) is reachable only through composite values.
- **Aggregate throughput is GVL-bounded.** Multi-Thread scaling caps around 1.4× from extra Ruby-side overlap. Genuine wasm parallelism would require `ext/` to release the GVL during wasmtime execution, which is currently not done.
- **One-shot timings are sensitive to filesystem cache.** The first `Sandbox.new` reads `data/kobako.wasm` from disk and JIT-compiles the Module. Numbers can vary 5-10% between a cold OS page cache and a hot one — record both states when investigating a regression in the first-construction number.
- **`benchmark-ips` measures steady-state.** Cold-path costs that only occur once per process (Engine init, Module compile) are captured via one-shot measurements, not the `ips` cases. Watch the right metric for the question you are asking.
