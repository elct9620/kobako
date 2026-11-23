# Concurrency

A Sandbox keeps no state from any run, so Threads may share one Sandbox or use
their own. Each Invocation owns its Handles, captures, and usage either way.

```
Thread A --eval--> +---------+ --> Execution A
                   | Sandbox |
Thread B --eval--> +---------+ --> Execution B
```

A Fiber runs one Invocation at a time. Each guest runs on a thread kobako keeps.
So a Fiber suspended inside a Service leaves its Thread free for another
Fiber's run, and the two end in either order.

## Shared Setup

Sharing a Sandbox makes its setup a template every Invocation starts from.

| What you set up | Who reaches it | What it owes |
|---|---|---|
| `bind`, `mount`, `network` on the Sandbox | every Thread sharing the Sandbox | thread safety; only what every request may reach |
| `ctx.bind`, `ctx.mount`, `ctx.network`, or an Extension `provider:` | that Invocation alone | nothing |

Keep the template minimal and give each request the rest through its
Context. A Context may grant more than the Sandbox does, but only at a path
or name the Sandbox declared.

```ruby
sandbox = Kobako::Sandbox.new.mount("/data", Kobako::Filesystem::Tmpfs.new)
sandbox.eval(script) { |ctx| ctx.mount("/data", tenant_files, access: :read_write) }
```

Choosing per Invocation keeps concurrent requests apart, such as one
tenant's files or user from another's.

## Run Count

kobako runs as many Invocations at once as your Threads and Fibers start. Each
takes a guest thread and address space from the process, so one the process
has no room for raises at once.

| Out of | Raises |
|---|---|
| threads, building the first Sandbox | `Kobako::SetupError` |
| threads, starting a run | `Kobako::TrapError` |
| address space, starting a run | `Kobako::TrapError` |

The next run goes on once room is free. Bound how many run at once with a pool
of your own, sized below what the process allows.

## GVL Modes

By default an Invocation holds Ruby's GVL while its guest runs. It hands the GVL
to a waiting Thread each quantum, as Ruby's own Threads do. The quantum is
100 ms, or `RUBY_THREAD_TIMESLICE` on Ruby 3.4 and later. Set that before Ruby
starts, as Ruby reads it then and kobako as it loads. A guest goes on while
another Thread holds the GVL. So a long run under `:hold` may overlap others
and use more than one core. Each wait on the host gives the GVL up in either mode,
so a sleeping guest never stalls other Threads. `gvl: :release`
drops the GVL while the guest runs and takes it back for each Service call.

```ruby
sandbox = Kobako::Sandbox.new(gvl: :release)
```

| Mode | Throughput | Pays |
|---|---|---|
| `:hold` (default) | about 15-20k `#eval`/s in aggregate, at any Thread count | a long run waits for its turn at the GVL |
| `:release` | scales compute-bound scripts with Threads | a handoff at every Service call |

The mode is fixed per Sandbox. Isolation, Handles, captures, and outcomes stay
the same. Dispatch-heavy scripts match or trail `:hold` under `:release`;
`rake bench:gvl_scheduling` measures both on your hardware.

### Interrupts

A run takes what Ruby delivers while the guest runs, within a quantum under
`:hold`, and stops the guest at what raises. A callback takes interrupts as the
Host App's own mask allows.

| Arrives | In either mode |
|---|---|
| while the guest runs, as a raising `trap` block does | the guest stops, and the run raises it; no callback sees it |
| while a Service or a mount's block runs | that callback takes it, as its mask allows |

A run waiting on the host goes on through a `Thread#wakeup` or a
trap that raises nothing. A wakeup that raises ends the run's waits, and the run
raises it once it ends.

## Fiber Schedulers

Each guest runs on a thread kobako keeps, so Fibers of one Thread overlap their
runs where a Fiber scheduler can switch them.

| While a run | Other Fibers of the Thread |
|---|---|
| is in a Service doing scheduler-aware I/O | run |
| waits on a guest releasing the GVL, under a scheduler with `blocking_operation_wait` | run |
| waits on a guest keeping the GVL | wait, until the run ends |

A run hands its wait over only once its guest is not back within a short spin.
A script calling Services often pays nothing extra. Async offers the
hook with its worker pool, on Ruby 4.0 and later:

```bash
ASYNC_SCHEDULER_WORKER_POOL=true ruby app.rb
```

A scheduler stopping the waiting Fiber ends the run, which raises what the
scheduler raised.

## Waits in Callbacks

A Ruby Filesystem's callback, or a mount's block, runs while the run holds
that Filesystem's directory lock. Waiting there on another Thread that may
reach the same Filesystem deadlocks both.

The lock is shared only where the Filesystem is. One mounted directly is built
afresh for each run, so no other Thread reaches its lock. One stacked as an
Overlay's Layer is built once with the Overlay and shared by every run.

| In a callback | Effect |
|---|---|
| a Mutex another Thread holds while it runs a Sandbox on the Filesystem | both Threads wait |
| `Thread#join` or `Queue#pop` on such a Thread | the same |
| a run whose Mounts lead back to this Filesystem's callbacks | two such runs may wait on each other |
| a run on a Fiber it starts, as an `Enumerator` does, reaching this Filesystem | refused at once as a deadlock |
| the same Fiber made with `storage: nil` | waits as another Thread's run does |
| work on the calling Thread alone | safe |

A run marks each Fiber it starts through Fiber storage, under `:kobako_run`. A
Fiber inheriting the mark holds locks as that run.

Only the run waiting for the directory lock can end it, at its `timeout` or an
interrupt of its Thread. The callback's own wait never ends alone.

## Forking

A forked child keeps only the Thread that forked, so a lock a run held would
stay held in the child. Once a Sandbox is built, every fork through Ruby
raises while any run is in progress. `Kernel#fork`, `IO.popen("-")`, and
`Process.daemon` are all checked.

| Fork | Outcome |
|---|---|
| while no run is in progress, as a preloading server does | forks at once |
| while a run on any Thread is in progress | raises `RuntimeError`; fork again once it ends |
| on macOS, after a guest's HTTPS | raises `RuntimeError` naming that server |

A run is in progress until its guest ends, its caller collected or not. A run
that starts while a fork is under way waits for that fork to return.

The refusals raise `RuntimeError`, not a `Kobako::Error`. A `rescue` for a
failed run never catches one by mistake.

macOS verifies a certificate through system services whose threads a child
does not inherit. A child that touches them crashes. So a preloading server
forks before any guest makes an HTTPS request. Under Pitchfork, kobako calls
`Pitchfork::Info.no_longer_fork_safe!` once the first such run ends, and
Pitchfork forks from another worker.

| Watch for | Because |
|---|---|
| a `fork(2)` another C extension makes | it skips the check; make it while no run is in progress |
| a `_fork` hook prepended after the first Sandbox that stops a service | it runs before the check; restart the service in `ensure` |
| a `_fork` hook prepended before the first Sandbox that awaits a run | that run waits for the fork, which waits for it |
| a trap handler that awaits a run while a fork is under way | the same |
| a Fiber left suspended in a run | every fork raises until the Fiber is collected and its guest ends |
| Threads forking so often their forks always overlap | a starting run waits until they stop |

## Rust SDK

An `Arc<Sandbox>` serves concurrent `&self` invocations from many threads. The
SDK has no GVL; the borrow checker proves the shared setup is only
read.

```rust
let sandbox = Arc::new(sandbox);
let handles: Vec<_> = (0..4)
    .map(|_| {
        let sandbox = Arc::clone(&sandbox);
        thread::spawn(move || sandbox.eval("1 + 1"))
    })
    .collect();
```

A bound `Receiver` is `Send + Sync`, so the type system asks for the thread
safety that Ruby leaves to you.

### Forked Children

On macOS the bundled engine takes a guest's faults over a Mach port by
default, and a forked child cannot run a Sandbox. A host that forks and runs
Sandboxes in the child turns on the `posix-signals-on-macos` feature. The Ruby
gem always has it on.

| Feature | Forked child | A Mach port crash reporter |
|---|---|---|
| off, the default | cannot run a Sandbox | sees the guest's faults apart |
| on | runs Sandboxes as usual | takes the guest's faults as crashes |

The choice holds for the whole process. A wasmtime Engine the host builds
itself must make the same one, or building it panics.

The SDK does not check a fork, as the Ruby gem does. A Rust host forks only
while no run is in progress. On macOS it forks before any guest makes an
HTTPS request.
