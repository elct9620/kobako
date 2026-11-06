# Resource Limits

Each Invocation runs under caps fixed when the Sandbox is built. A cap reached
stops that run alone, and the Sandbox serves the next one unharmed.

```ruby
sandbox = Kobako::Sandbox.new(
  timeout: 5.0,                   # seconds
  setup_timeout: 2.0,             # seconds
  memory_limit: 10 * 1024 * 1024, # bytes
  stdout_limit: 64 * 1024,        # bytes
  stderr_limit: 64 * 1024,        # bytes
  open_file_limit: 16             # descriptors
)
```

Pass `nil` to any cap to switch it off. [Security Model](security.md) says
which caps stay uncapped either way.

## Cap Defaults

Each cap has a default and its own way to end a run.

| Cap | Default | Reached |
|---|---|---|
| `timeout` | 60.0 s | `Kobako::TimeoutError` |
| `setup_timeout` | 10.0 s | `Kobako::SetupError` at `new` |
| `memory_limit` | 4 MiB | `Kobako::MemoryLimitError` |
| `stdout_limit`, `stderr_limit` | 1 MiB each | output clipped, no raise |
| `open_file_limit` | 1024 | `Errno::EMFILE` in the guest |

## Cap Scope

Each cap measures one span, so a reused Sandbox never accumulates against a
global budget.

| Cap | Measures |
|---|---|
| `timeout` | wall time inside the guest, Service calls included |
| `setup_timeout` | instantiating a Guest Binary and reading its ABI version, not compiling it |
| `memory_limit` | the run's `memory.grow` past its starting memory |
| `open_file_limit` | every descriptor one run holds, the standard three and Mounts' included |

`setup_timeout` applies the first time a path is loaded, and later Sandboxes on
that path skip the check. A Pool builds each Sandbox at its first checkout, so
its `SetupError` raises from `with`.

## Usage Report

`Execution#usage` reports what a run spent, measured as its caps measure it. A
rescued trap reports it just as a completed run does.

```ruby
usage = sandbox.eval(script).usage
usage.wall_time   # => Float seconds inside the guest
usage.memory_peak # => Integer bytes grown, never past memory_limit
```

## Isolation Profile

`profile:` requests the isolation posture, and the default `:hermetic` is the
strictest. Construction raises `Kobako::SetupError` on a runtime weaker than
the request.

```
:permissive   <   :hermetic (default)
live clock        frozen clock
live entropy      constant entropy
```

Neither profile gives the guest an ambient filesystem, environment, or network.
What each rung reveals is in [Security Model](security.md).

## Rust SDK

`Options` holds the same caps as `Option` fields, and `Options::default()`
carries the Ruby defaults. `None` switches a cap off. `Options` is
non-exhaustive, so start from the default and set each field.

```rust
let mut options = Options::default();
options.timeout = Some(Duration::from_secs(5));
options.memory_limit = Some(10 * 1024 * 1024);
options.open_file_limit = Some(16);
options.profile = Profile::Hermetic;
let sandbox = Sandbox::new("kobako.wasm", options)?;
```

| Ruby | Rust |
|---|---|
| `TimeoutError`, `MemoryLimitError` | `Error::Timeout`, `Error::MemoryLimit` |
| `SetupError` | `Error::Setup` |
| `usage.wall_time`, `usage.memory_peak` | `execution.usage().wall_time`, `.memory_peak` |
