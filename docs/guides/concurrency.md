# Concurrency

A Sandbox keeps no state from any run, so Threads may share one Sandbox or use
their own. Each Invocation owns its Handles, captures, and usage either way.

```
Thread A --eval--> +---------+ --> Execution A
                   | Sandbox |
Thread B --eval--> +---------+ --> Execution B
```

One Thread still runs one Invocation at a time.

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

## GVL Modes

By default an Invocation holds Ruby's GVL for its whole span, so guest code
across Threads runs one at a time. Each wait on the host gives the GVL up in
either mode, so a sleeping guest never stalls other Threads. `gvl: :release`
drops the GVL while the guest runs and takes it back for each Service call.

```ruby
sandbox = Kobako::Sandbox.new(gvl: :release)
```

| Mode | Throughput | Pays |
|---|---|---|
| `:hold` (default) | about 15-20k `#eval`/s in aggregate, at any Thread count | nothing extra |
| `:release` | scales compute-bound scripts with Threads | a handoff at every Service call |

The mode is fixed per Sandbox. Isolation, Handles, captures, and outcomes stay
the same. Dispatch-heavy scripts match or trail `:hold` under `:release`;
`rake bench:gvl_scheduling` measures both on your hardware.

### Interrupts Under Release

`:release` defers Ruby's interrupts while the guest runs, and each Service call
takes them as they come. Three things differ from `:hold`.

| Case | Under `:release` |
|---|---|
| A `Thread.handle_interrupt` mask around the run | a Service call does not inherit it |
| A main-Thread `trap` block that raises or exits | may stop the run where it cannot unwind |
| On Ruby 3.4+, another Ractor's `require` whose Thread fails to start | may stop the run the same way |

`:hold` keeps all three away. A trap that only sets a flag keeps the second away.
Ruby runs trap handlers and a Ractor's `require` on the main Thread alone. So a
run on another Thread is outside the last two cases, and the first run releasing
the GVL on the main Thread warns once.

## Waits in Callbacks

A Ruby Filesystem's callback, or a mount's block, runs while the run holds
that Filesystem's directory lock. Waiting there on another Thread that may
reach the same Filesystem deadlocks both.

| In a callback | Effect |
|---|---|
| a Mutex another Thread holds while it runs a Sandbox on the Filesystem | both Threads wait |
| `Thread#join` or `Queue#pop` on such a Thread | the same |
| a run whose Mounts lead back to this Filesystem's callbacks | two such runs may wait on each other |
| work on the calling Thread alone | safe |

Only the run waiting for the directory lock can end it, at its `timeout` or an
interrupt of its Thread. The callback's own wait never ends alone.

## Forking

A forked child keeps only the Thread that forked. A lock a run held stays held
in the child, so fork while no run is in progress.

| Fork | Child |
|---|---|
| outside any run, as a preloading server does | runs Sandboxes as usual |
| while a run is in progress, a Service's included | may wait forever on a lock that run held |

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
