# Snippets

A Snippet is guest code the Sandbox replays at the start of every Invocation.
`#preload` registers one, and `#run` calls into what the snippets defined.

```
boot state (a)
  +--> replay :Adder
  +--> replay :Greeter
  +--> entry code (b)
          |
          v
     Execution (c)
```

- (a) The canonical boot state, baked into the Guest Binary at build time.
- (b) `eval(source)`, or `run(:Target, ...)` on a constant a snippet defined.
- (c) Returned to the host, and then the instance is discarded.

An Extension builds on the same replay, adding a host backend; it is in
[Extensions](extensions.md).

## Entrypoints

`#run(:Target, *args, **kwargs)` calls `call` on a top-level constant a snippet
defined, and returns the Execution.

```ruby
sandbox.preload(code: "Adder = ->(a, b) { a + b }", name: :Adder)
sandbox.preload(code: 'Greeter = ->(opts) { "hello, #{opts[:name]}" }', name: :Greeter)

sandbox.run(:Adder, 2, 3).value            # => 5
sandbox.run(:Greeter, name: "world").value # => "hello, world"
```

Keyword arguments arrive as a trailing positional Hash, because mruby's C call
path carries no keywords. Declare a Hash parameter and unpack it yourself.

| A target that | Raises |
|---|---|
| no snippet defined | `Kobako::UndefinedEntrypointError` |
| is no constant name | `ArgumentError` |

An `UndefinedEntrypointError` lists the defined constants on `#available`, so
the name is fixed from the error.

## Payload Forms

`#preload` takes source or precompiled bytecode. Both replay through the same
per-Invocation path.

| Form | Call | Fails at the first replay as |
|---|---|---|
| source | `preload(code: "...", name: :Const)` | `Kobako::SandboxError` |
| bytecode | `preload(binary: bytes)` | `Kobako::BytecodeError` |

Use source for snippets kept in your repository. Use bytecode when snippets ship
from a separate `mrbc` pipeline. Source compiles once before the first
Invocation, so bytecode saves only that one compile.

Bytecode runs as host code, so it must come from your own build. Why is in
[Security Model](security.md).

## Snippet Validation

No snippet fails at `#preload`, since content is checked when it first replays.
Run a no-op Invocation to validate before real traffic.

```ruby
sandbox.eval("nil") # raises now if a snippet does not load
```

## Rust SDK

The Rust SDK splits `#preload` by form and takes a `RunPayload` for `run`.

| Ruby | Rust |
|---|---|
| `preload(code:, name:)` | `preload(name, source)` |
| `preload(binary:)` | `preload_binary(bytes)` |
| `run(:Adder, 2, 3)` | `run("Adder", RunPayload::values(args, kwargs))` |
| `UndefinedEntrypointError#available` | `Failure::available` |

```rust
sandbox.preload("Adder", "Adder = ->(a, b) { a + b }")?;
let sum = sandbox
    .run("Adder", RunPayload::values(vec![Value::Int(2).into(), Value::Int(3).into()], vec![]))?
    .value()?; // => Value::Int(5)
```
