# Kobako

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/elct9620/kobako)

Kobako is a Ruby gem that embeds a Wasm-isolated mruby interpreter inside your application, so you can execute untrusted Ruby scripts (LLM-generated code, user formulas, student submissions, third-party plugins) in-process without giving them access to host memory, files, network, or credentials.

The host (`wasmtime`) runs a precompiled `kobako.wasm` guest containing mruby and a Transport proxy. The only way a guest script can reach the outside world is through Host App-declared **Services** — named Ruby objects you explicitly inject into the sandbox; the guest sees each one as a proxy that forwards calls back to the host over the Transport wire.

```
        Host process                       Wasm guest
   ┌──────────────────────┐         ┌──────────────────────┐
   │  Kobako::Sandbox     │ ─eval─▶ │  mruby interpreter   │
   │                      │ ─run──▶ │                      │
   │  Services            │ ◀─call─ │  KV::Lookup.call(k)  │
   │   KV::Lookup         │ ─resp─▶ │                      │
   │                      │         │                      │
   │  stdout / stderr buf │ ◀─pipe─ │  puts / warn         │
   │                      │         │                      │
   │  return value        │ ◀─last─ │  last expression     │
   └──────────────────────┘         └──────────────────────┘
            trusted                       untrusted
```

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

result # => 3
```

The script executes inside the Wasm guest. It cannot read your filesystem, open sockets, or touch your `ENV`.

## Glossary

| Term | Meaning |
|------|---------|
| Sandbox | The runtime unit (`Kobako::Sandbox`) that runs guest code and returns a result or raises a typed error. |
| Service | A host Ruby object injected under `<Namespace>::<Member>` — the guest's only path to host resources. |
| Namespace / Member | A guest-visible Ruby module, and a named binding (a module constant) within it. |
| Invocation | One `#eval` or `#run`; capability state resets between invocations. |
| Snippet | Named mruby code (source or bytecode) replayed into a fresh state before every invocation. |
| Handle | An opaque token the guest holds for a host object the wire cannot transmit directly. |
| Block | A guest mruby block passed to a Service; each `yield` is a synchronous round-trip into the guest. |

## Usage

### Services

Declare a Namespace, then `bind` any Ruby object as a Member; the guest reaches it as a `<Namespace>::<Member>` proxy and invokes its public methods through the Transport wire. See [`docs/behavior/registration.md`](docs/behavior/registration.md) B-07..B-12.

```ruby
class User
  attr_reader :name

  def initialize(name:)
    @name = name
  end
end

sandbox.define(:Project).bind(:User,   User.new(name: "alice"))
sandbox.define(:KV)     .bind(:Lookup, ->(key) { redis.get(key) })

sandbox.eval(<<~RUBY)
  Project::User.name         # => "alice"
  KV::Lookup.call("user_42") # => "..."
RUBY
```

Names must match `/\A[A-Z]\w*\z/`. Symbol kwargs travel transparently to the host method's keyword arguments. The registry seals at the first invocation (see [Invocation Lifecycle](#invocation-lifecycle)); later `#define` raises `ArgumentError`.

### Output Capture

Guest writes through `puts` / `print` / `p` / `$stdout` / `$stderr` are buffered per-channel and exposed independently of the return value ([`docs/behavior/lifecycle.md`](docs/behavior/lifecycle.md) B-04). Buffers clear at the start of each invocation; overflow is clipped at the cap and flagged by `#stdout_truncated?` / `#stderr_truncated?`.

```ruby
result = sandbox.eval(<<~RUBY)
  puts "hello"
  warn "be careful"
  42
RUBY

result          # => 42
sandbox.stdout  # => "hello\n"
sandbox.stderr  # => "be careful\n"
```

### Error Handling

Every invocation either returns a value or raises exactly one of three classes, so you can route faults without inspecting messages. The full taxonomy lives in [`lib/kobako/errors.rb`](lib/kobako/errors.rb).

```ruby
begin
  sandbox.eval(script)
rescue Kobako::TrapError
  # Wasm engine fault or cap exhaustion. Discard the Sandbox.
rescue Kobako::ServiceError
  # A host Service call failed and the script did not rescue it.
rescue Kobako::SandboxError
  # The script raised, failed to compile, or returned an unrepresentable value.
end
```

| Class                           | Parent         | Trigger                                              |
|---------------------------------|----------------|------------------------------------------------------|
| `Kobako::TimeoutError`          | `TrapError`    | Per-invocation `timeout` exhausted                   |
| `Kobako::MemoryLimitError`      | `TrapError`    | Per-invocation `memory_limit` exhausted              |
| `Kobako::HandlerExhaustedError` | `SandboxError` | Handle counter reached its 2³¹ − 1 cap               |
| `Kobako::BytecodeError`         | `SandboxError` | `#preload(binary:)` failed RITE validation at replay |

`SandboxError` and `ServiceError` carry structured `origin` / `klass` / `backtrace_lines` / `details` fields when the guest produced a panic envelope.

### Resource Limits

Each invocation enforces a wall-clock `timeout` and a per-invocation linear-memory `memory_limit`; exhaustion raises a `TrapError` subclass. Pass `nil` to `timeout` / `memory_limit` to disable that cap. Read [`Sandbox#usage`](lib/kobako/sandbox.rb) after the call — populated on every outcome including traps — for actual consumption ([`docs/behavior/lifecycle.md`](docs/behavior/lifecycle.md) B-35).

```ruby
sandbox = Kobako::Sandbox.new(
  timeout:      5.0,              # seconds, default 60.0
  memory_limit: 10 * 1024 * 1024, # bytes,   default 1 MiB
  stdout_limit: 64 * 1024,        # bytes,   default 1 MiB
  stderr_limit: 64 * 1024
)
```

| Cap            | Raises                     | Default |
|----------------|----------------------------|---------|
| `timeout`      | `Kobako::TimeoutError`     | 60.0 s  |
| `memory_limit` | `Kobako::MemoryLimitError` | 1 MiB   |
| `stdout_limit` | output clipped (no raise)  | 1 MiB   |
| `stderr_limit` | output clipped (no raise)  | 1 MiB   |

`memory_limit` covers the per-invocation `memory.grow` delta from the entry baseline, so a Sandbox reused across invocations does not silently accumulate against a global budget.

### Invocation Lifecycle

One Sandbox serves many invocations. Service bindings and preloaded snippets persist across calls; capability state (Handles, stdout, stderr, memory delta) resets between them.

```
   ───────────── setup phase (mutable) ─────────────

     sandbox = Kobako::Sandbox.new
     sandbox.define(:KV).bind(:Lookup, ...)
     sandbox.preload(code: ..., name: :Adder)
     sandbox.preload(code: ..., name: :Greeter)

                          │
                          ▼

   ═════════════════ seal point ═════════════════
   First #eval or #run freezes the Service registry
   and snippet table. Further define / preload now
   raise ArgumentError.

                          │
                          ▼

   ──────────────── invocation N ───────────────────

     1. start from the canonical boot state
        (mruby pre-initialized into the artifact at build time)

     2. replay snippets (in insertion order):
          :Adder     → defines Adder
          :Greeter   → defines Greeter

     3. dispatch:  eval(source)  or  run(:Target, *args)

     4. return value to host

     5. discard the instance; reset per-invocation state:
          · Handles invalidated
          · stdout / stderr buffers cleared
          · memory delta zeroed

     Services + snippets persist; invocation N+1 repeats.
```

For workloads that must be isolated from each other (one Sandbox per tenant, per student submission, per agent session), construct a fresh `Kobako::Sandbox` per scope — wasmtime's Engine and the compiled Module are cached at process scope, so additional Sandboxes amortize cold-start cost automatically.

### Pooling

For hosts that serve many short invocations, `Kobako::Pool` keeps a bounded set of warm, identically set-up Sandboxes and hands each one to a single exclusive holder at a time ([`docs/behavior/runtime.md`](docs/behavior/runtime.md) B-46..B-48). Construction forwards every `Sandbox.new` keyword verbatim; the optional block is the per-Sandbox setup window and runs exactly once per constructed Sandbox.

`Kobako::Pool` is experimental today and is best treated as a convenience for warm, pre-configured reuse rather than a throughput optimisation. B-49 bakes the shared boot state into the artifact and every dynamic script still compiles and runs per invocation, so all a pool actually saves is the ~30 µs host-side `Sandbox.new`. For the workload kobako is built for — many small, short-lived Sandboxes running dynamic scripts — that is not a significant gain (~4-5% in the [serverless example](examples/serverless/README.md), and proportionally less once the script itself does real work).

```ruby
pool = Kobako::Pool.new(slots: 4) do |sandbox|
  sandbox.define(:KV).bind(:Lookup, ->(key) { redis.get(key) })
end

pool.with { |sandbox| sandbox.eval(%(KV::Lookup.call("user_42"))) }
```

| Option | Meaning | Default |
|--------|---------|---------|
| `slots:` | Upper bound on constructed Sandboxes | required |
| `checkout_timeout:` | Seconds `#with` waits for a free Sandbox; `nil` waits indefinitely | 5.0 |

Sandboxes construct lazily on first demand. `#with` yields a Sandbox with empty output buffers and returns the block's value; at block exit the Sandbox returns to the pool, except a block that raises `Kobako::TrapError` discards its Sandbox and the slot refills by a fresh construction on next demand. A checkout that waits past `checkout_timeout` raises `Kobako::PoolTimeoutError`. There is no teardown verb — a Pool releases everything with its own reachability.

### Service Blocks

A Service method can accept a guest-supplied block via `&blk` and `yield` into it. The block body runs inside the Wasm guest; `break` / `next` / exceptions follow normal Ruby semantics, scoped to the single dispatch. See [`docs/behavior/yield.md`](docs/behavior/yield.md) B-23..B-30.

```ruby
sandbox.define(:Seq).bind(:Map, ->(items, &blk) { items.map(&blk) })

sandbox.eval('Seq::Map.call([1, 2, 3]) { |x| x * 2 }')
# => [2, 4, 6]
```

### Handle Management

A non-wire-representable host object — returned from a Service (B-14), passed to `#run` (B-34), or handed back from the guest (B-37) — crosses the boundary as an opaque `Kobako::Handle` proxy and is restored to the original object before host code sees it; any other unrepresentable value raises `Kobako::SandboxError`. Handles are scoped to a single invocation ([`docs/behavior/dispatch.md`](docs/behavior/dispatch.md) B-13..B-21, B-34, B-37).

```ruby
class Greeter
  def initialize(name) = @name = name
  def greet            = "hi, #{@name}"
end

sandbox.define(:Factory).bind(:Make, ->(name) { Greeter.new(name) })

sandbox.eval('Factory::Make.call("Bob").greet')  # => "hi, Bob"  (Handle round-trip inside guest)
sandbox.eval('Factory::Make.call("Bob")')        # => #<Greeter @name="Bob">  (B-37 restoration)
```

A `break` value from a guest block is the one exception: it unwinds back to the guest Member call rather than to host code, so a Handle in it stays a Handle — restoring would just re-wrap the same object into a new id on the return trip.

Each dispatch that hands back a non-wire-representable object allocates a *new* Handle — kobako never deduplicates by object identity (B-15, B-17). This is most visible with fluent / builder APIs. An `ActiveRecord::Relation` chain `spawn`s a fresh relation at each step, so every hop is an independent dispatch that binds its own Handle:

```
   guest chain                        host  (Catalog::Handles, one invocation)
   ───────────                        ─────────────────────────────────────────
   User.where(active: true)  ─call──▶ Relation #1 (fresh clone)  bound ▶ Handle 1
                             ◀─Handle 1
       .order(:created_at)   ─call──▶ Relation #2 (fresh clone)  bound ▶ Handle 2
                             ◀─Handle 2
       .limit(10)            ─call──▶ Relation #3 (fresh clone)  bound ▶ Handle 3
                             ◀─Handle 3

   3 hops ─▶ 3 dispatches ─▶ 3 distinct relations ─▶ 3 Handles
   all stay live until the invocation ends, then reset together
```

This is deliberate, not a leak. Handle IDs run to 2³¹ − 1 per invocation and reset between invocations, so even deep chains stay far inside the range. Two consequences are worth keeping in mind: the same host object handed back twice yields two *different* Handles — the guest cannot tell they alias — and every intermediate Handle stays live until the invocation ends, since there is no per-Handle release (B-19).

### Snippets & Entrypoints

`Sandbox#preload` registers named mruby snippets that replay into every invocation's canonical boot state; `Sandbox#run(:Target, *args, **kwargs)` dispatches into a top-level `Object` constant defined by those snippets ([`docs/behavior/invocation.md`](docs/behavior/invocation.md) B-31..B-33).

```ruby
sandbox = Kobako::Sandbox.new
sandbox.preload(code: "Adder   = ->(a, b)  { a + b }",          name: :Adder)
sandbox.preload(code: 'Greeter = ->(name:) { "hello, #{name}" }', name: :Greeter)

sandbox.run(:Adder, 2, 3)            # => 5
sandbox.run(:Greeter, name: "world") # => "hello, world"
```

```
   per-invocation replay (every #eval / #run, snippets in insertion order):

      canonical boot state
            │
            ├──▶ replay :Adder            (defines Adder)
            │
            ├──▶ replay :Greeter          (defines Greeter)
            │
            └──▶ eval(source)  -or-  run(:Target, *args, **kwargs)
                       │
                       ▼
                  return value, then instance discarded
```

`#preload` accepts two payload forms:

| Form     | Signature                            | Snippet name source                   | Validation timing                                                          |
|----------|--------------------------------------|---------------------------------------|----------------------------------------------------------------------------|
| Source   | `preload(code: "...", name: :Const)` | The `name:` keyword                   | Trial-compiled at preload; compile errors raise immediately                |
| Bytecode | `preload(binary: bytes)`             | Read from the bytecode's `debug_info` | Deferred to first invocation; failure raises `Kobako::BytecodeError`       |

Use the source form for snippets authored in your repo (compile errors fail fast at `#preload`); use the bytecode form when snippets ship as build artifacts from a separate `mrbc` pipeline. Both replay through the same per-invocation path.

## Security

kobako isolates the guest, but **what it may reach is whatever you `bind`** — and `bind`
exposes *every* public method of the object. So bind a purpose-built object scoped to the
task, not a capable one whose other methods leak more than you intend.

```ruby
class ThemeReader          # only #color is reachable; AppConfig.secret_key is not
  def color = AppConfig.theme.color
end

sandbox = Kobako::Sandbox.new
sandbox.define(:Cfg).bind(:Settings, ThemeReader.new)  # not: bind(:Settings, AppConfig)

sandbox.eval('Cfg::Settings.color')  # => "#3366ff"  — every other method raises NoMethodError
```

When a purpose-built wrapper is more than you need, an object can gate its own surface in
place: a private `respond_to_guest?(name)` answers, per method, whether the guest may call
it. Returning `false` for every name makes the object opaque — a credential the guest
forwards to another Service but never reads — while a named subset becomes an allow-list.

Guest code can name any `<Namespace>::<Member>` path, but a forged name only resolves to
something you bound — the real authorization gate is this host-side allowlist. Give each
trust context its own Sandbox, and see [`docs/security-model.md`](docs/security-model.md) for the rest
as security-design concerns: validating untrusted input, default-deny external effects,
and controlling the return surface.

## Performance

Order-of-magnitude figures on macOS arm64, Ruby 3.4.7, YJIT off. Absolute values vary by hardware but ratios are stable across machines. Full numbers, methodology, and the +10%-regression gate live in [`benchmark/README.md`](benchmark/README.md).

| Phase                                                        | Cost                  |
|--------------------------------------------------------------|-----------------------|
| First `Sandbox.new` ever for a Guest Binary (Module JIT, then disk-cached) | ~500 ms once per machine |
| First `Sandbox.new` in a fresh process (`.cwasm` cache warm) | ~5 ms one-time        |
| Subsequent `Sandbox.new` (caches warm)                       | ~30 µs                |
| Warm `#eval("nil")` on a reused Sandbox                      | ~73 µs                |
| Warm `#run(:Entrypoint, ...)` dispatch                       | ~104 µs               |
| Service call amortized inside one invocation                 | ~6.8 µs               |
| Snippet replay per invocation                                | ~8 µs each            |
| Per additional idle Sandbox (RSS)                            | ~1 KB                 |

The Cranelift JIT runs once per machine and gem version — the compiled artifact persists in a `.cwasm` disk cache, so later processes deserialize in milliseconds. An idle Sandbox holds no wasm instance (the canonical boot state is baked into the artifact and instantiated per invocation), which is why a thousand idle tenants cost ~32 MB total. `ext/` does not release the GVL during wasmtime execution, so wasm work is GVL-serialized: aggregate throughput stays around 16k `#eval`/s regardless of Thread count, though Ruby-side `#eval` setup still overlaps. A +10% regression on any of the six SPEC-mandated benchmarks blocks release.

Regexp is an opt-in capability gem, excluded from the default binary and the gated set; its throughput is tracked in a separate non-gated characterization (`#10` in [`benchmark/README.md`](benchmark/README.md)). There `=~` (~5 µs/match) costs about 4× `match?` (~1.2 µs), because `=~` eagerly builds the `MatchData` and match globals — prefer `match?` for boolean tests.

```bash
bundle exec rake bench  # six gated regression benchmarks (~5-8 min)
```

## Development

After checking out the repo:

```bash
bin/setup         # install dependencies
bundle exec rake  # default: compile + test + rubocop + steep
```

Building from source requires a WASI-capable Rust toolchain in addition to the standard host toolchain; the first compile walks the full chain — the [beni](https://github.com/elct9620/beni) gem vendors wasi-sdk + mruby and builds `libmruby.a` (`rake beni:build`), then `rake wasm:build` produces the Guest Binary. See [`CLAUDE.md`](CLAUDE.md) for the rake task map and pipeline layout. `bin/console` opens an IRB session with the gem preloaded; `bundle exec rake install` installs the local checkout as a gem.

## Contributing

Bug reports and pull requests are welcome at <https://github.com/elct9620/kobako>. Please open an issue before starting on non-trivial changes so we can align on scope.

## License

Kobako is released under the [Apache License 2.0](https://opensource.org/licenses/Apache-2.0).
