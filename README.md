# Kobako

Kobako is a Ruby gem that embeds a Wasm-isolated mruby interpreter inside your application, so you can execute untrusted Ruby scripts (LLM-generated code, user formulas, student submissions, third-party plugins) in-process without giving them access to host memory, files, network, or credentials.

The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing mruby and an RPC client. The only way a guest script can reach the outside world is through Host App-declared **Services** — named Ruby objects you explicitly inject into the sandbox.

## Features

- **In-process Wasm sandbox** — no subprocess, no container. Each `Sandbox#run` is a synchronous Ruby call.
- **Capability injection via Services** — guest scripts can only call Ruby objects you explicitly `bind` under a two-level `Group::Member` namespace.
- **Structured outcome** — `#run` returns the deserialized last expression of the guest script as a normal Ruby value.
- **Three-class error taxonomy** — every failure is exactly one of `TrapError` (Wasm engine), `SandboxError` (script / wire fault), or `ServiceError` (Service capability fault), so you can route errors without inspecting messages.
- **Per-run state reset** — Handles issued during one `#run` are invalidated before the next; Service bindings remain.
- **Separated stdout / stderr capture** — guest `puts`/`warn` output is buffered (1 MiB default cap, configurable, with a `[truncated]` marker on overflow) and is independent of the RPC channel.
- **Capability Handles** — Services may return stateful host objects; the guest receives an opaque token it can use as the target of follow-up RPC calls, with no way to dereference it.

## Requirements

- **Ruby ≥ 3.3.0**
- **Rust / Cargo** at install time — the native extension compiles from source via `rb_sys`
- **Linux** or **macOS** — Windows is not supported

The precompiled `kobako.wasm` Guest Binary ships inside the gem, so end users do **not** need a WASI toolchain. (The toolchain is only required if you build the gem from a source checkout — see [Development](#development).)

## Installation

Add Kobako to your Gemfile:

```bash
bundle add kobako
```

Or install it directly:

```bash
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

Guest scripts reach host resources only through Services. Declare a **Group**, then `bind` named **Members** on it — each member can be any Ruby object that responds to the methods the guest will call.

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

Names must match the Ruby constant pattern `/\A[A-Z]\w*\z/`. Services declared before the first `#run` remain active across subsequent runs.

### Keyword arguments

Keyword keys travel as Symbols and reach the host method as keyword arguments:

```ruby
sandbox.define(:Geo).bind(:Lookup, ->(name:, region:) { "#{region}/#{name}" })

sandbox.run('Geo::Lookup.call(name: "alice", region: "us")')
# => "us/alice"
```

## Capturing stdout and stderr

Guest output is captured into per-run buffers and exposed independently from the return value:

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

Each `#run` clears the buffers at start. Output past the per-channel cap is truncated; the buffer ends with `[truncated]` and `#run` still returns normally.

```ruby
Kobako::Sandbox.new(stdout_limit: 64 * 1024, stderr_limit: 64 * 1024)
```

## Error handling

Every `#run` either returns a value or raises exactly one of three classes:

```ruby
begin
  sandbox.run(script)
rescue Kobako::TrapError => e
  # Wasm engine crashed: OOM, stack overflow, corrupted guest runtime.
  # The Sandbox is unrecoverable — discard and recreate it.
rescue Kobako::ServiceError => e
  # A Service call failed and the script did not rescue it.
  # Treat like any other downstream-service failure in your app.
rescue Kobako::SandboxError => e
  # The script itself raised, failed to compile, or produced an
  # unrepresentable value. A script-level fault, not infrastructure.
end
```

`SandboxError` and `ServiceError` carry structured fields (`origin`, `klass`, `backtrace_lines`, `details`) when the guest produced a panic envelope.

`Kobako::ServiceError::Disconnected` is a named subclass raised when an RPC target Handle has been invalidated. `Kobako::HandleTableExhausted` is a named `SandboxError` subclass raised when the per-run Handle counter reaches its cap (2³¹ − 1).

## Capability Handles

When a Service returns a stateful host object (anything beyond `nil` / Boolean / Integer / Float / String / Symbol / Array / Hash), the wire layer transparently allocates an opaque Handle. The guest receives a `Kobako::Handle` proxy it can use as the target of further RPC calls — but cannot dereference, forge from an integer, or smuggle across runs.

```ruby
class Greeter
  def initialize(name) = @name = name
  def greet            = "hi, #{@name}"
end

sandbox.define(:Factory).bind(:Make, ->(name) { Greeter.new(name) })

sandbox.run(<<~RUBY)
  g = Factory::Make.call("Bob")  # g is a Kobako::Handle proxy
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

## Development

After checking out the repo:

```bash
bin/setup                  # install dependencies
bundle exec rake           # default: compile + test + rubocop
```

Building from source requires a WASI-capable Rust toolchain in addition to the standard host toolchain. The first compile walks the full vendor / mruby / wasm chain:

```bash
bundle exec rake compile   # build the native extension
bundle exec rake wasm:build # rebuild data/kobako.wasm (requires vendor:setup + mruby:build)
bundle exec rake test      # run the Ruby test suite
```

`bin/console` opens an IRB session with the gem preloaded for experimentation.

To install the local checkout as a gem:

```bash
bundle exec rake install
```

## Performance

Headline numbers from the current baseline (macOS arm64, Ruby 3.4.7 — full results in [`benchmark/results/`](benchmark/results)):

| What | Cost |
|---|---|
| First `Sandbox.new` in a fresh process (Engine init + Module compile) | ~410 ms one-time |
| Subsequent `Sandbox.new` (cache warm) | ~90 µs |
| Reusing a Sandbox for one `#run("nil")` | ~67 µs |
| Fresh Sandbox per request — the tenant-isolation pattern | ~175 µs (+110 µs versus reuse) |
| Per-RPC cost amortized across many calls in one `#run` | ~5.4 µs |
| 100 000-iteration integer XOR loop in mruby | ~44 ms |
| One-time process memory for wasmtime Engine + Module | ~110 MB |
| Memory per additional Sandbox after the first | ~200 KB |
| 1 000 isolated tenants in one process (1 Sandbox each) | ~340 MB total |
| Aggregate throughput across N Threads | GVL-bound — wasm execution is serialized, modest scaling from Ruby-side overlap |

Practical implications:

- **Pre-warm at boot.** The 410 ms first-Sandbox cost is paid once per process; every subsequent Sandbox amortizes to micro-, not milliseconds. Construct one Sandbox at boot before serving requests.
- **Tenant isolation is affordable.** Per-request Sandbox construction adds ~110 µs of overhead; per-tenant RSS budget is ~200 KB plus one-time ~110 MB for the engine. 1 000 isolated tenants in a single Sidekiq / Puma worker is well within typical RSS limits.
- **Batch RPCs inside one `#run`.** A single Service call costs ~76 µs because each `#run` carries ~67 µs of setup; 1 000 calls inside one `#run` reduce the per-call cost to ~5.4 µs.

A +10% regression on any of the five SPEC-mandated benchmarks blocks release. See [`benchmark/README.md`](benchmark/README.md) for the full per-suite breakdown, rake task reference, and known measurement caveats (guest String size cap, GVL bounds, allocator retention).

```bash
bundle exec rake bench   # five gated regression benchmarks (≤ 1 MiB payloads, ~5-7 min)
```

## Contributing

Bug reports and pull requests are welcome at <https://github.com/elct9620/kobako>. Please open an issue before starting on non-trivial changes so we can align on scope.

## License

Kobako is released under the [Apache License 2.0](https://opensource.org/licenses/Apache-2.0).
