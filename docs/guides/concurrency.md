# Concurrency

A Sandbox keeps no state from any run, so Threads may share one Sandbox or use
their own. Each Invocation owns its Handles, captures, and usage either way.

```
Thread A --eval--> +---------+ --> Execution A
                   | Sandbox |
Thread B --eval--> +---------+ --> Execution B
```

One Thread still runs one Invocation at a time.

## Shared Bindings

Sharing a Sandbox adds one obligation, and it falls on what you bound once.

| What you bind | Who reaches it | What it owes |
|---|---|---|
| bound once at setup | every Thread sharing the Sandbox | its own thread safety |
| `ctx.bind`, or an Extension `provider:` | that Invocation alone | nothing |

Choosing the object per Invocation keeps concurrent identities apart, such as
one request's user from another's.

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
