# Invocations

An Invocation is one run of guest code, by `#eval` or `#run`. It leaves an
Execution, the frozen record of what the run produced, wrote, and consumed.

```
setup (a) --> seal --> invocation 1 --> Execution
                   +-> invocation 2 --> Execution
                   +-> ...
```

- (a) `bind`, `mount`, `network`, `preload`, `install`, and `flag`.

A Sandbox keeps nothing from one Invocation to the next, so the same Sandbox
serves many of them.

## Invocation Lifecycle

The first `#eval` or `#run` seals the Sandbox. Services, mounts, network
entries, snippets, and flags then stay fixed, and a later registration raises
`ArgumentError`.

Each Invocation then takes these steps:

1. Resolve this run's bindings: `ctx.bind`, `ctx.mount`, `ctx.network`, and Extension providers.
2. Start from the canonical boot state, baked into the Guest Binary at build time.
3. Replay the preloaded snippets in insertion order.
4. Run `eval(source)` or `run(:Target, *args, **kwargs)`.
5. Return the run's Execution.
6. Discard the instance, which invalidates its Handles and zeroes its memory.

For workloads that must stay apart, build a Sandbox per scope: per tenant, per
submission, per agent session. The Engine and the compiled Guest Binary are
cached per process, so another Sandbox costs little.

## Execution Record

An Execution carries the guest value beside the run's output and usage.

```ruby
execution = sandbox.eval(<<~RUBY)
  puts "hello"
  warn "be careful"
  42
RUBY

execution.value  # => 42
execution.stdout # => "hello\n"
execution.stderr # => "be careful\n"
execution.usage.memory_peak # => bytes the run grew
```

| Reader | Holds |
|---|---|
| `#value` | the last expression; `nil` on a failed run |
| `#stdout`, `#stderr` | what `puts`, `print`, `p`, `warn`, `$stdout`, and `$stderr` wrote |
| `#stdout_truncated?`, `#stderr_truncated?` | whether a channel reached its cap |
| `#usage` | the wall time and peak memory the run spent |
| `#failed?` | whether the run failed |

`#failed?` tells apart two `nil` values: a last expression that was `nil`, and
a run that produced none.

## Error Handling

A run either returns an Execution or raises one of three classes. You route
faults by class, never by message.

```ruby
begin
  sandbox.eval(script)
rescue Kobako::TrapError
  # The engine stopped the guest, a cap included.
rescue Kobako::ServiceError
  # A Service call failed and the script did not rescue it.
rescue Kobako::SandboxError => e
  # The script raised, did not compile, or returned an unrepresentable value.
  logger.warn(e.execution.stderr)
end
```

Each error carries the failed run's Execution on `#execution`, with its output
and usage. A refusal before the guest ran leaves `#execution` `nil`.

### Error Classes

Each subclass narrows one of the three roots.

| Class | Parent | Raised when |
|---|---|---|
| `TimeoutError` | `TrapError` | `timeout` ran out |
| `MemoryLimitError` | `TrapError` | `memory_limit` ran out |
| `NoServiceError` | `ServiceError` | nothing callable is bound there |
| `ServiceArgumentError` | `ServiceError` | the call would not bind |
| `HandleExhaustedError` | `SandboxError` | Handles reached 2^31 - 1 |
| `BytecodeError` | `SandboxError` | preloaded bytecode failed validation |
| `UnsupportedFlagError` | `SandboxError` | a critical flag is unrecognised |
| `UndefinedEntrypointError` | `SandboxError` | `#run` named no snippet's constant |

A `NoServiceError` also covers a dead Handle and a method the guest may not
call. A `ServiceArgumentError` also covers a Service raising `ArgumentError`.

### Guest Failures

A `SandboxError` or `ServiceError` from the guest carries where it happened.

| Reader | Holds |
|---|---|
| `#origin` | which side the failure came from |
| `#klass` | the guest exception's class name |
| `#backtrace_lines` | the guest backtrace |

### Host Exceptions

A Service, a mount's block, a Ruby Filesystem, or a network entry's policy or
credential is your code running inside the run. What it raises splits by class.

| Raised by your code | Result |
|---|---|
| a `StandardError` | the guest sees a failure it may rescue |
| `Interrupt`, `exit`, `throw`, `Timeout` | the run ends; the invocation raises it as is |

## Rust SDK

`eval` and `run` return `Result<Execution, Error>`. The outer `Err` means the
guest never started, while a guest failure rides inside the Execution.

```rust
let execution = sandbox.eval(script)?;     // Err: the run never started
match execution.value() {                  // Err: the run failed
    Ok(value) => println!("{value:?}"),
    Err(kobako::Error::Service(failure)) => eprintln!("{}", failure.message),
    Err(other) => eprintln!("{other}"),
}
let _ = (execution.stdout(), execution.usage());
```

| Ruby | Rust `Error` |
|---|---|
| `TimeoutError`, `MemoryLimitError`, `TrapError` | `Timeout`, `MemoryLimit`, `Trap` |
| `SandboxError`, `BytecodeError`, `UnsupportedFlagError` | `Sandbox`, `Bytecode`, `UnsupportedFlag` |
| `ServiceError` | `Service` |
| `ArgumentError` before the run | `Argument`, or `Sealed` after the seal |
| `SetupError` | `Setup` |

A `Failure` carries `name`, `message`, and `backtrace`. `Error` is
non-exhaustive, so keep a wildcard arm.
