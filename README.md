# Kobako

Kobako is a Ruby gem that embeds a Wasm-isolated mruby interpreter inside your application, so you can execute untrusted Ruby scripts (LLM-generated code, user formulas, student submissions, third-party plugins) in-process without giving them access to host memory, files, network, or credentials.

The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing mruby and an RPC client. The only way a guest script can reach the outside world is through Host App-declared **Services** — named Ruby objects you explicitly inject into the sandbox.

```
        Host process                       Wasm guest
   ┌──────────────────────┐         ┌──────────────────────┐
   │  Kobako::Sandbox     │ ─eval─▶ │  mruby interpreter   │
   │                      │ ─run──▶ │                      │
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

| Feature | Description |
|---|---|
| In-process Wasm sandbox | No subprocess, no container. Both invocation verbs (`Sandbox#eval` for ad-hoc source, `Sandbox#run` for entrypoint dispatch) are synchronous Ruby calls. |
| Per-invocation caps | Every invocation enforces a wall-clock `timeout` (default 60 s) and a per-invocation linear-memory `memory_limit` (default 1 MiB); exhaustion raises `Kobako::TimeoutError` / `Kobako::MemoryLimitError`. |
| Capability injection via Services | Guest scripts can only call Ruby objects you explicitly `bind` under a two-level `Namespace::Member` path. |
| Preloaded snippets | `Sandbox#preload` registers source or RITE bytecode for setup-once dispatch via `Sandbox#run(:Entrypoint, *args, **kwargs)`. |
| Capability Handles | Services may return stateful host objects; the guest receives an opaque `Kobako::RPC::Handle` proxy it can use as the target of follow-up RPC calls, with no way to dereference it. |
| Three-class error taxonomy | Every failure is exactly one of `TrapError`, `SandboxError`, or `ServiceError`, so you can route errors without inspecting messages. |
| Per-invocation state reset | Handles issued during one invocation are invalidated before the next; Service bindings and preloaded snippets remain. |
| Separated stdout / stderr capture | Guest writes to `$stdout` / `$stderr` are buffered per-channel (1 MiB default cap, configurable); overflow is clipped and reported by `#stdout_truncated?` / `#stderr_truncated?`. |
| Curated mruby stdlib | Core extensions plus `mruby-onig-regexp` for full Onigmo `Regexp` support; no mrbgem with I/O, network, or syscall access is bundled. |

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

result = sandbox.eval(<<~RUBY)
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

sandbox.eval(<<~RUBY)
  Log::Sink.call("starting")
  KV::Lookup.call("user_42")
RUBY
# => "..." (the redis value)
```

Names must match the Ruby constant pattern `/\A[A-Z]\w*\z/`. Services declared before the first invocation remain active across subsequent invocations; `define` after the first invocation (`#eval` or `#run`) raises `ArgumentError`.

### Keyword arguments

Keyword keys travel as Symbols and reach the host method as keyword arguments:

```ruby
sandbox.define(:Geo).bind(:Lookup, ->(name:, region:) { "#{region}/#{name}" })

sandbox.eval('Geo::Lookup.call(name: "alice", region: "us")')
# => "us/alice"
```

## Per-invocation caps

Each Sandbox enforces a wall-clock timeout and a guest linear-memory cap on every invocation (`#eval` or `#run`). Both default to safe values; pass `nil` to `timeout` or `memory_limit` to disable that cap. The output caps (`stdout_limit` / `stderr_limit`) cannot be disabled — pass a large Integer instead.

```ruby
sandbox = Kobako::Sandbox.new(
  timeout:      5.0,           # seconds, default 60.0
  memory_limit: 10 * 1024 * 1024, # bytes, default 1 MiB
  stdout_limit: 64 * 1024,     # bytes, default 1 MiB
  stderr_limit: 64 * 1024
)
```

| Cap            | Raises (subclass of `TrapError`)   | Default  |
|----------------|------------------------------------|----------|
| `timeout`      | `Kobako::TimeoutError`             | 60.0 s   |
| `memory_limit` | `Kobako::MemoryLimitError`         | 1 MiB    |
| `stdout_limit` | output silently clipped at cap     | 1 MiB    |
| `stderr_limit` | output silently clipped at cap     | 1 MiB    |

The timeout deadline is absolute wall-clock from invocation entry and is checked at guest Wasm safepoints. Long-running host Service callbacks still consume wall-clock time but do not themselves trap — the next guest safepoint will trap immediately on return if the deadline has passed.

`memory_limit` is scoped to the **per-invocation linear-memory delta** — the budget covers how much the current `#eval` / `#run` may grow `memory.grow` past the size observed at invocation entry. The mruby image's initial allocation and prior invocations' high-water mark are folded into that entry baseline, so a Sandbox reused across many invocations does not silently accumulate against a global budget.

The 1 MiB default targets lightweight dynamic RPC workloads — short scripts that orchestrate Service calls, return small structured values, or replace a tool-calling layer in an AI Agent's Code Mode dispatch. Bump `memory_limit` when scripts compose multi-hundred-KiB strings, hold large composite return values, or run computations that allocate substantial intermediate state. Because the cap resets every invocation, multi-call patterns on one Sandbox do not need a budget that covers their cumulative footprint — only the largest single invocation's working set.

## Capturing stdout and stderr

Guest output is captured into per-invocation buffers and exposed independently from the return value. The buffers cover the full Ruby IO surface — `puts`, `print`, `printf`, `p`, `<<`, and writes through `$stdout` / `$stderr` — all routed through the host-captured WASI pipe.

```ruby
sandbox = Kobako::Sandbox.new

result = sandbox.eval(<<~RUBY)
  puts "hello"
  warn "be careful"
  42
RUBY

result          # => 42
sandbox.stdout  # => "hello\n"
sandbox.stderr  # => "be careful\n"
```

Each invocation clears the buffers at start. Output past the per-channel cap is clipped at the cap boundary — the invocation still returns normally, the bytes carry no truncation sentinel, and `#stdout_truncated?` / `#stderr_truncated?` flip to `true`.

```ruby
sandbox = Kobako::Sandbox.new(stdout_limit: 64 * 1024)
sandbox.eval('puts "a" * 100_000')
sandbox.stdout.bytesize     # => 65_536
sandbox.stdout_truncated?   # => true
```

## Error handling

Every invocation (`#eval` or `#run`) either returns a value or raises exactly one of three classes:

```ruby
begin
  sandbox.eval(script)
rescue Kobako::TrapError => e
  # Wasm engine fault OR per-invocation cap exhaustion:
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

| Class                                  | Parent             | Trigger                                                                                  |
|----------------------------------------|--------------------|------------------------------------------------------------------------------------------|
| `Kobako::TimeoutError`                 | `TrapError`        | Per-invocation `timeout` exhausted                                                       |
| `Kobako::MemoryLimitError`             | `TrapError`        | Per-invocation `memory_limit` exhausted                                                  |
| `Kobako::ServiceError::Disconnected`   | `ServiceError`     | RPC target Handle has been invalidated                                                   |
| `Kobako::HandleTableExhausted`         | `SandboxError`     | Per-invocation Handle counter reached its 2³¹ − 1 cap                                    |
| `Kobako::BytecodeError`                | `SandboxError`     | `#preload(binary:)` payload failed RITE structural validation at first invocation replay |

## Capability Handles

When a Service returns a stateful host object (anything beyond `nil` / Boolean / Integer / Float / String / Symbol / Array / Hash), the wire layer transparently allocates an opaque Handle. The guest receives a `Kobako::RPC::Handle` proxy it can use as the target of further RPC calls — but cannot dereference, forge from an integer, or smuggle across runs.

```ruby
class Greeter
  def initialize(name) = @name = name
  def greet            = "hi, #{@name}"
end

sandbox.define(:Factory).bind(:Make, ->(name) { Greeter.new(name) })

sandbox.eval(<<~RUBY)
  g = Factory::Make.call("Bob")  # g is a Kobako::RPC::Handle proxy
  g.greet                         # second RPC, routed to the Greeter
RUBY
# => "hi, Bob"
```

Handles are scoped to a single invocation — a Handle obtained in invocation N is invalid in invocation N+1, even on the same Sandbox.

## Setup-once, run-many

A single Sandbox can serve many invocations. Service bindings and preloaded snippets persist; capability state (Handles, stdout, stderr) resets between invocations.

```ruby
sandbox = Kobako::Sandbox.new
sandbox.define(:Data).bind(:Fetch, ->(id) { records[id] })

sandbox.eval('Data::Fetch.call("a")')  # => "..."
sandbox.eval('Data::Fetch.call("b")')  # => "..." (same bindings, fresh state)
```

For workloads that must be isolated from each other (e.g., one Sandbox per tenant, per student submission), construct a fresh `Kobako::Sandbox` per scope. wasmtime's Engine and the compiled Module are cached at process scope, so additional Sandboxes amortize cold-start cost automatically.

## Preloaded snippets and entrypoint dispatch

`Sandbox#preload` registers named mruby snippets that replay against the fresh `mrb_state` before every invocation; `Sandbox#run(:Target, *args, **kwargs)` dispatches into a top-level `Object` constant defined by those snippets and returns the value of `Target.call(*args, **kwargs)`. Together they cover setup-once / dispatch-many workloads where the same logic is exercised across many requests.

```ruby
sandbox = Kobako::Sandbox.new
sandbox.preload(code: "Adder = ->(a, b) { a + b }", name: :Adder)
sandbox.preload(code: 'Greeter = ->(name:) { "hello, #{name}" }', name: :Greeter)

sandbox.run(:Adder, 2, 3)              # => 5
sandbox.run(:Greeter, name: "world")   # => "hello, world"
```

`#preload` accepts two payload forms:

| Form     | Signature                              | Snippet name source                 | Validation timing                                                                       |
|----------|----------------------------------------|-------------------------------------|------------------------------------------------------------------------------------------|
| Source   | `preload(code: "...", name: :Const)`   | The `name:` keyword                 | Trial-compiled at preload time; compile errors raise immediately                         |
| Bytecode | `preload(binary: bytes)`               | Read from the bytecode's `debug_info` | Structural validation runs at first invocation; failure raises `Kobako::BytecodeError`  |

The source form trial-compiles each snippet against a fresh `mrb_state` at preload time, so compile errors surface immediately at the `#preload` call. The bytecode form treats `binary:` as opaque bytes and defers RITE version / body validation to the first invocation's replay, because that is when the payload loads into a fresh `mrb_state`. Bytecode compiled without `debug_info` (`mrbc` without `-g`) is still accepted — only its backtrace frames are omitted, while exception class, message, and `origin` attribution are preserved.

Snippets replay in insertion order, so later snippets can reference constants defined by earlier ones. The snippet table is sealed by the first invocation alongside Service registration; additional `#preload` calls after the first `#eval` or `#run` raise `ArgumentError`.

`#run` resolves `target` (Symbol or String, normalized to Symbol) only as a top-level `Object` constant — `::`-segmented names and lowercase forms fail at host pre-flight with `ArgumentError`. A `Kobako::SandboxError` surfaces when the constant is missing or does not respond to `#call`.

### Choosing between source and bytecode

Use the **source form** when snippets are authored in your repo or generated at boot — compile errors land at the `#preload` call so a misbehaving snippet fails fast at setup time, and no separate `mrbc` toolchain is needed. The trial-compile happens once per snippet (~2.5 µs per snippet) and is paid at preload, not on the request hot path.

Use the **bytecode form** when snippets ship as build artifacts from a pipeline that runs `mrbc` separately — for example, when source bodies should not be embedded in the running process, when you want a build step that compiles and packages snippets ahead of release, or when you want `Exception#backtrace` frames attributed to the bytecode's `debug_info` filename rather than a host-supplied `name:` keyword. Structural validation (RITE version, body integrity) is deferred to the first invocation, so a malformed bytecode payload surfaces as `Kobako::BytecodeError` on the first `#eval` or `#run`, not at `#preload`.

Both forms behave identically at dispatch time and replay through the same per-invocation path, so the choice between them is about your build / distribution pipeline and where you want errors to land, not about runtime cost.

## Performance

Order-of-magnitude figures for capacity planning on macOS arm64, Ruby 3.4.7, YJIT off. Absolute values vary by hardware but the ratios are stable across machines. Detailed numbers and methodology live in [`benchmark/README.md`](benchmark/README.md).

### Lifecycle costs

| Phase                                                       | Cost                                            |
|-------------------------------------------------------------|-------------------------------------------------|
| First `Sandbox.new` in a fresh process (Engine + Module JIT) | ~600 ms one-time                                |
| Subsequent `Sandbox.new` (Engine cache warm)                | ~130 µs                                         |
| Reusing a Sandbox for one `#eval("nil")`                    | ~135 µs                                         |
| Fresh `Sandbox.new` per request                             | ~275 µs (≈ +140 µs vs reuse)                    |
| Warm `#run(:Entrypoint, ...)` dispatch                      | ~165 µs                                         |
| Per-RPC cost amortized inside one invocation                | ~6.6 µs (1 000 RPCs in one `#eval` ≈ 6.6 ms)    |
| 100 000-iteration integer XOR loop in mruby                 | ~43 ms                                          |
| 1 000 Onigmo `Regexp =~` matches                            | ~3 µs each                                      |

The ~600 ms cold start dominates the first Sandbox in a process — wasmtime JIT-compiles the precompiled `kobako.wasm` Module and the result is cached at process scope. Construct one Sandbox at boot before serving requests so the JIT cost lands off the hot path.

### Memory budget

| Allocation                                  | Cost                                                                       |
|---------------------------------------------|----------------------------------------------------------------------------|
| Process RSS after first `Sandbox.new`       | ~150-180 MB (one-time engine + module + first instance)                    |
| Per additional Sandbox                      | ~580 KB (Wasm instance + linear memory + WASI capture pipes)               |
| 1 000 isolated tenants in one process       | ~750 MB total                                                              |

Use these as upper-bound budgets for capacity planning, not lower bounds — actual RSS shifts ~30% with host process load and macOS allocator state.

### Choosing your pattern

When the script is ad-hoc (LLM-generated, untrusted user input) and only runs once, use `Sandbox#eval(source)`. Per-invocation cost is ~135 µs of setup plus the script's own runtime; mruby parses the source on every call.

When you have a fixed set of entrypoints exercised many times — a stable AI Agent tool-call protocol, a plug-in registry loaded at boot, a small library of host-side commands — preload the entrypoints via `Sandbox#preload(code:, name:)` once at setup and dispatch via `Sandbox#run(:Target, *args, **kwargs)`. The mruby source compile (~2.5 µs per snippet) lands once at preload, not on every request, and warm dispatch costs ~165 µs.

Mind the snippet replay cost. Every preloaded snippet replays into a fresh `mrb_state` before **every** invocation, whether the invocation is `#eval` or `#run`, at ~7-9 µs per snippet per invocation. Preloading 8 helpers adds ~60 µs to every subsequent invocation; preloading 64 helpers adds ~565 µs. Keep the snippet count proportionate to how often the helpers are actually used — preloading rarely-touched helpers is more expensive than inlining or re-eval'ing them.

For tenant isolation between mutually untrusted scopes, construct a fresh `Kobako::Sandbox` per scope. Per-request construction costs ~140 µs over reuse plus ~580 KB of RSS — comfortably affordable for 1 000+ isolated tenants in one Sidekiq / Puma worker. Reuse a Sandbox when all requests share one trust scope; isolate when scripts come from many.

### Concurrency

`ext/` does not release the GVL during wasmtime execution, so wasm work is GVL-serialized: aggregate throughput across N Threads stays around 7-8k `#eval`/s regardless of N. Ruby-side `#eval` setup can still overlap, so a short `#eval` running while another Thread is in a long `#eval` is slowed by ~2× (not 10×) — host-side synchronization yields the GVL and the contending Thread interleaves. Mixed short / long workloads in one process do not deadlock.

### Regression gate

A +10% regression on any of the five SPEC-mandated benchmarks (cold_start, RPC roundtrip, codec, mruby VM, HandleTable) blocks release. Full per-suite breakdown in [`benchmark/README.md`](benchmark/README.md).

```bash
bundle exec rake bench   # five gated regression benchmarks (~5-8 min, ≤ 1 MiB payloads)
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
