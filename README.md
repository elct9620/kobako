# Kobako

Kobako is a Ruby gem that embeds a Wasm-isolated mruby interpreter inside your application, so you can execute untrusted Ruby scripts (LLM-generated code, user formulas, student submissions, third-party plugins) in-process without giving them access to host memory, files, network, or credentials.

The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing mruby and an RPC client. The only way a guest script can reach the outside world is through Host App-declared **Services** — named Ruby objects you explicitly inject into the sandbox.

```
        Host process                       Wasm guest
   ┌──────────────────────┐         ┌──────────────────────┐
   │  Kobako::Sandbox     │ ──run─▶ │  mruby interpreter   │
   │                      │         │                      │
   │  Services            │ ◀──RPC─ │  KV::Lookup.call(k)  │
   │   KV::Lookup         │ ─resp─▶ │                      │
   │                      │         │                      │
   │  stdout / stderr buf │ ◀─pipe─ │  puts / warn         │
   │                      │         │                      │
   │  return value        │ ◀─last─ │  last expression     │
   └──────────────────────┘         └──────────────────────┘
            trusted                       untrusted
```

## Features

- **In-process Wasm sandbox** — no subprocess, no container. Each `Sandbox#run` is a synchronous Ruby call.
- **Per-run caps** — every `#run` enforces a wall-clock `timeout` (default 60 s) and a guest `memory_limit` (default 5 MiB). Exhaustion raises `Kobako::TimeoutError` / `Kobako::MemoryLimitError`.
- **Capability injection via Services** — guest scripts can only call Ruby objects you explicitly `bind` under a two-level `Namespace::Member` path.
- **Three-class error taxonomy** — every failure is exactly one of `TrapError` (Wasm engine / per-run cap), `SandboxError` (script / wire fault), or `ServiceError` (Service capability fault), so you can route errors without inspecting messages.
- **Per-run state reset** — Handles issued during one `#run` are invalidated before the next; Service bindings remain.
- **Separated stdout / stderr capture** — guest `puts` / `warn` / `print` / `printf` / `p` and writes to `$stdout` / `$stderr` are buffered per-channel (1 MiB default cap, configurable). Output past the cap is clipped; `#stdout_truncated?` / `#stderr_truncated?` report overflow.
- **Capability Handles** — Services may return stateful host objects; the guest receives an opaque `Kobako::RPC::Handle` proxy it can use as the target of follow-up RPC calls, with no way to dereference it.
- **Curated mruby stdlib** — core extensions plus `mruby-onig-regexp` for full Onigmo `Regexp` support. No mrbgem with I/O, network, or syscall access is bundled.

## Requirements

- **Ruby ≥ 3.3.0**
- **Rust / Cargo** at install time — the native extension compiles from source via `rb_sys`
- **Linux** or **macOS** — Windows is not supported

The precompiled `kobako.wasm` Guest Binary ships inside the gem, so end users do **not** need a WASI toolchain. (The toolchain is only required if you build the gem from a source checkout — see [Development](#development).)

## Installation

```bash
bundle add kobako
# or
gem install kobako
```

## Quick Start

```ruby
require "kobako"

sandbox = Kobako::Sandbox.new

result = sandbox.run(<<~RUBY)
  1 + 2
RUBY

result        # => 3
sandbox.stdout # => ""
```

The script executes inside the Wasm guest. It cannot read your filesystem, open sockets, or touch your `ENV`.

## Injecting Services

Guest scripts reach host resources only through Services. Declare a **Namespace**, then `bind` named **Members** on it — each member can be any Ruby object that responds to the methods the guest will call.

```ruby
sandbox = Kobako::Sandbox.new

sandbox.define(:KV).bind(:Lookup, ->(key) { redis.get(key) })
sandbox.define(:Log).bind(:Sink,   ->(msg) { logger.info(msg) })

sandbox.run(<<~RUBY)
  Log::Sink.call("starting")
  KV::Lookup.call("user_42")
RUBY
# => "..." (the redis value)
```

Names must match the Ruby constant pattern `/\A[A-Z]\w*\z/`. Services declared before the first `#run` remain active across subsequent runs; `define` after the first `#run` raises `ArgumentError`.

### Keyword arguments

Keyword keys travel as Symbols and reach the host method as keyword arguments:

```ruby
sandbox.define(:Geo).bind(:Lookup, ->(name:, region:) { "#{region}/#{name}" })

sandbox.run('Geo::Lookup.call(name: "alice", region: "us")')
# => "us/alice"
```

## Per-run caps

Each Sandbox enforces a wall-clock timeout and a guest linear-memory cap on every `#run`. Both default to safe values and may be overridden at construction; pass `nil` to disable.

```ruby
sandbox = Kobako::Sandbox.new(
  timeout:      5.0,           # seconds, default 60.0
  memory_limit: 10 * 1024 * 1024, # bytes, default 5 MiB
  stdout_limit: 64 * 1024,     # bytes, default 1 MiB
  stderr_limit: 64 * 1024
)
```

| Cap            | Raises (subclass of `TrapError`)   | Default  |
|----------------|------------------------------------|----------|
| `timeout`      | `Kobako::TimeoutError`             | 60.0 s   |
| `memory_limit` | `Kobako::MemoryLimitError`         | 5 MiB    |
| `stdout_limit` | output silently clipped at cap     | 1 MiB    |
| `stderr_limit` | output silently clipped at cap     | 1 MiB    |

The timeout deadline is absolute wall-clock from `#run` entry and is checked at guest Wasm safepoints. Long-running host Service callbacks still consume wall-clock time but do not themselves trap — the next guest safepoint will trap immediately on return if the deadline has passed.

## Capturing stdout and stderr

Guest output is captured into per-run buffers and exposed independently from the return value. The buffers cover the full Ruby IO surface — `puts`, `print`, `printf`, `p`, `<<`, and writes through `$stdout` / `$stderr` — all routed through the host-captured WASI pipe.

```ruby
sandbox = Kobako::Sandbox.new

result = sandbox.run(<<~RUBY)
  puts "hello"
  warn "be careful"
  42
RUBY

result          # => 42
sandbox.stdout  # => "hello\n"
sandbox.stderr  # => "be careful\n"
```

Each `#run` clears the buffers at start. Output past the per-channel cap is clipped at the cap boundary — `#run` still returns normally, the bytes carry no truncation sentinel, and `#stdout_truncated?` / `#stderr_truncated?` flip to `true`.

```ruby
sandbox = Kobako::Sandbox.new(stdout_limit: 64 * 1024)
sandbox.run('puts "a" * 100_000')
sandbox.stdout.bytesize     # => 65_536
sandbox.stdout_truncated?   # => true
```

## Error handling

Every `#run` either returns a value or raises exactly one of three classes:

```ruby
begin
  sandbox.run(script)
rescue Kobako::TrapError => e
  # Wasm engine fault OR per-run cap exhaustion:
  #   - Kobako::TimeoutError       (wall-clock timeout)
  #   - Kobako::MemoryLimitError   (memory_limit exceeded)
  #   - Kobako::TrapError          (engine crash / wire-violation fallback)
  # The Sandbox is unrecoverable — discard and recreate it.
rescue Kobako::ServiceError => e
  # A Service call failed and the script did not rescue it.
  # Treat like any other downstream-service failure in your app.
rescue Kobako::SandboxError => e
  # The script itself raised, failed to compile, or produced an
  # unrepresentable value. A script-level fault, not infrastructure.
end
```

`SandboxError` and `ServiceError` carry structured fields (`origin`, `klass`, `backtrace_lines`, `details`) when the guest produced a panic envelope. Named subclasses:

- `Kobako::TimeoutError` / `Kobako::MemoryLimitError` — per-run cap exhaustion (subclasses of `TrapError`).
- `Kobako::ServiceError::Disconnected` — RPC target Handle has been invalidated.
- `Kobako::HandleTableExhausted` — per-run Handle counter reached its cap (2³¹ − 1); subclass of `SandboxError`.

## Capability Handles

When a Service returns a stateful host object (anything beyond `nil` / Boolean / Integer / Float / String / Symbol / Array / Hash), the wire layer transparently allocates an opaque Handle. The guest receives a `Kobako::RPC::Handle` proxy it can use as the target of further RPC calls — but cannot dereference, forge from an integer, or smuggle across runs.

```ruby
class Greeter
  def initialize(name) = @name = name
  def greet            = "hi, #{@name}"
end

sandbox.define(:Factory).bind(:Make, ->(name) { Greeter.new(name) })

sandbox.run(<<~RUBY)
  g = Factory::Make.call("Bob")  # g is a Kobako::RPC::Handle proxy
  g.greet                         # second RPC, routed to the Greeter
RUBY
# => "hi, Bob"
```

Handles are scoped to a single `#run` — a Handle obtained in run N is invalid in run N+1, even on the same Sandbox.

## Setup-once, run-many

A single Sandbox can serve many script executions. Service bindings persist; capability state (Handles, stdout, stderr) resets between runs.

```ruby
sandbox = Kobako::Sandbox.new
sandbox.define(:Data).bind(:Fetch, ->(id) { records[id] })

sandbox.run('Data::Fetch.call("a")')  # => "..."
sandbox.run('Data::Fetch.call("b")')  # => "..." (same bindings, fresh state)
```

For workloads that must be isolated from each other (e.g., one Sandbox per tenant, per student submission), construct a fresh `Kobako::Sandbox` per scope. wasmtime's Engine and the compiled Module are cached at process scope, so additional Sandboxes amortize cold-start cost automatically.

## Performance

Headline numbers from the current baseline (macOS arm64, Ruby 3.4.7, YJIT off — full results in [`benchmark/results/`](benchmark/results) and [`benchmark/README.md`](benchmark/README.md)).

| What | Cost |
|---|---|
| First `Sandbox.new` in a fresh process (Engine init + Module JIT) | ~2.0 s one-time |
| Subsequent `Sandbox.new` (cache warm) | ~130 µs |
| Reusing a Sandbox for one `#run("nil")` | ~135 µs |
| Fresh Sandbox per request — the tenant-isolation pattern | ~275 µs (+140 µs vs reuse) |
| Per-RPC cost amortized across 1 000 calls in one `#run` | ~35 µs |
| 100 000-iteration integer XOR loop in mruby | ~200 ms |
| 1 000 Onigmo `Regexp =~` matches | ~14 µs per match |
| Process RSS after the first `Sandbox.new` | ~150 MB (one-time) |
| Memory per additional Sandbox | ~575 KB |
| 1 000 isolated tenants in one process | ~730 MB total |
| Aggregate throughput across N Threads | GVL-bound — wasm execution serialized, modest scaling from Ruby-side overlap |

Practical implications:

- **Pre-warm at boot.** The ~2 s first-Sandbox cost is paid once per process; every subsequent Sandbox amortizes to micro-, not seconds. Construct one Sandbox at boot before serving requests.
- **Tenant isolation is affordable.** Per-request Sandbox construction adds ~140 µs of overhead; per-tenant RSS budget is ~575 KB plus one-time ~130 MB for the engine. 1 000 isolated tenants in a single Sidekiq / Puma worker is well within typical RSS limits.
- **Batch RPCs inside one `#run`.** A single Service call costs ~135 µs because each `#run` carries ~130 µs of setup; 1 000 calls inside one `#run` reduce the per-call cost to ~35 µs.

A +10% regression on any of the five SPEC-mandated benchmarks blocks release. See [`benchmark/README.md`](benchmark/README.md) for the full per-suite breakdown and known measurement caveats.

```bash
bundle exec rake bench   # five gated regression benchmarks (≤ 1 MiB payloads, ~5-8 min)
```

## Development

After checking out the repo:

```bash
bin/setup                  # install dependencies
bundle exec rake           # default: compile + test + rubocop + steep
```

Building from source requires a WASI-capable Rust toolchain in addition to the standard host toolchain. The first compile walks the full vendor / mruby / wasm chain:

```bash
bundle exec rake compile    # build the native extension
bundle exec rake wasm:build # rebuild data/kobako.wasm
bundle exec rake test       # run the Ruby test suite
```

`bin/console` opens an IRB session with the gem preloaded for experimentation. To install the local checkout as a gem, run `bundle exec rake install`.

## Contributing

Bug reports and pull requests are welcome at <https://github.com/elct9620/kobako>. Please open an issue before starting on non-trivial changes so we can align on scope.

## License

Kobako is released under the [Apache License 2.0](https://opensource.org/licenses/Apache-2.0).
