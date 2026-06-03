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

Declare a Namespace, then `bind` any Ruby object as a Member; the guest reaches it as a `<Namespace>::<Member>` proxy and invokes its public methods through the Transport wire. See [`docs/behavior.md`](docs/behavior.md) B-07..B-12.

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

Guest writes through `puts` / `print` / `p` / `$stdout` / `$stderr` are buffered per-channel and exposed independently of the return value ([`docs/behavior.md`](docs/behavior.md) B-04). Buffers clear at the start of each invocation; overflow is clipped at the cap and flagged by `#stdout_truncated?` / `#stderr_truncated?`.

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

Each invocation enforces a wall-clock `timeout` and a per-invocation linear-memory `memory_limit`; exhaustion raises a `TrapError` subclass. Pass `nil` to `timeout` / `memory_limit` to disable that cap. Read [`Sandbox#usage`](lib/kobako/sandbox.rb) after the call — populated on every outcome including traps — for actual consumption ([`docs/behavior.md`](docs/behavior.md) B-35).

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

     1. allocate fresh mrb_state

     2. replay snippets (in insertion order):
          :Adder     → defines Adder
          :Greeter   → defines Greeter

     3. dispatch:  eval(source)  or  run(:Target, *args)

     4. return value to host

     5. discard mrb_state; reset per-invocation state:
          · Handles invalidated
          · stdout / stderr buffers cleared
          · memory delta zeroed

     Services + snippets persist; invocation N+1 repeats.
```

For workloads that must be isolated from each other (one Sandbox per tenant, per student submission, per agent session), construct a fresh `Kobako::Sandbox` per scope — wasmtime's Engine and the compiled Module are cached at process scope, so additional Sandboxes amortize cold-start cost automatically.

### Service Blocks

A Service method can accept a guest-supplied block via `&blk` and `yield` into it. The block body runs inside the Wasm guest; `break` / `next` / exceptions follow normal Ruby semantics, scoped to the single dispatch. See [`docs/behavior.md`](docs/behavior.md) B-23..B-30.

```ruby
sandbox.define(:Seq).bind(:Map, ->(items, &blk) { items.map(&blk) })

sandbox.eval('Seq::Map.call([1, 2, 3]) { |x| x * 2 }')
# => [2, 4, 6]
```

### Handle Management

A non-wire-representable host object — returned from a Service (B-14), passed to `#run` (B-34), or handed back from the guest (B-37) — crosses the boundary as an opaque `Kobako::Handle` proxy and is restored to the original object before host code sees it; any other unrepresentable value raises `Kobako::SandboxError`. Handles are scoped to a single invocation ([`docs/behavior.md`](docs/behavior.md) B-13..B-21, B-34, B-37).

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

`Sandbox#preload` registers named mruby snippets that replay against the fresh `mrb_state` before every invocation; `Sandbox#run(:Target, *args, **kwargs)` dispatches into a top-level `Object` constant defined by those snippets ([`docs/behavior.md`](docs/behavior.md) B-31..B-33).

```ruby
sandbox = Kobako::Sandbox.new
sandbox.preload(code: "Adder   = ->(a, b)  { a + b }",          name: :Adder)
sandbox.preload(code: 'Greeter = ->(name:) { "hello, #{name}" }', name: :Greeter)

sandbox.run(:Adder, 2, 3)            # => 5
sandbox.run(:Greeter, name: "world") # => "hello, world"
```

```
   per-invocation replay (every #eval / #run, snippets in insertion order):

      fresh mrb_state
            │
            ├──▶ replay :Adder            (defines Adder)
            │
            ├──▶ replay :Greeter          (defines Greeter)
            │
            └──▶ eval(source)  -or-  run(:Target, *args, **kwargs)
                       │
                       ▼
                  return value, then mrb_state discarded
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

Guest code can name any `<Namespace>::<Member>` path, but a forged name only resolves to
something you bound — the real authorization gate is this host-side allowlist. Give each
trust context its own Sandbox, and see [`docs/security.md`](docs/security.md) for the rest
as security-design concerns: validating untrusted input, default-deny external effects,
and controlling the return surface.

## Performance

Order-of-magnitude figures on macOS arm64, Ruby 3.4.7, YJIT off. Absolute values vary by hardware but ratios are stable across machines. Full numbers, methodology, and the +10%-regression gate live in [`benchmark/README.md`](benchmark/README.md).

| Phase                                                        | Cost                  |
|--------------------------------------------------------------|-----------------------|
| First `Sandbox.new` in a fresh process (Engine + Module JIT) | ~600 ms one-time      |
| Subsequent `Sandbox.new` (Engine cache warm)                 | ~125 µs               |
| Warm `#eval("nil")` on a reused Sandbox                      | ~135 µs               |
| Warm `#run(:Entrypoint, ...)` dispatch                       | ~165 µs               |
| Service call amortized inside one invocation                 | ~6.7 µs               |
| Snippet replay per invocation                                | ~7-9 µs each          |
| Per additional Sandbox (RSS)                                 | ~570 KB               |

Construct one Sandbox at boot so the ~600 ms JIT cost lands off the request hot path. `ext/` does not release the GVL during wasmtime execution, so wasm work is GVL-serialized: aggregate throughput stays around 7-8k `#eval`/s regardless of Thread count, though Ruby-side `#eval` setup still overlaps. A +10% regression on any of the six SPEC-mandated benchmarks blocks release.

```bash
bundle exec rake bench  # six gated regression benchmarks (~5-8 min)
```

## Development

After checking out the repo:

```bash
bin/setup         # install dependencies
bundle exec rake  # default: compile + test + rubocop + steep
```

Building from source requires a WASI-capable Rust toolchain in addition to the standard host toolchain; the first compile walks the full vendor / mruby / wasm chain. See [`CLAUDE.md`](CLAUDE.md) for the rake task map and pipeline layout. `bin/console` opens an IRB session with the gem preloaded; `bundle exec rake install` installs the local checkout as a gem.

## Contributing

Bug reports and pull requests are welcome at <https://github.com/elct9620/kobako>. Please open an issue before starting on non-trivial changes so we can align on scope.

## License

Kobako is released under the [Apache License 2.0](https://opensource.org/licenses/Apache-2.0).
