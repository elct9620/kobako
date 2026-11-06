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
across Threads runs one at a time. `gvl: :release` drops the GVL while the guest
runs and takes it back for each Service call.

```ruby
sandbox = Kobako::Sandbox.new(gvl: :release)
```

| Mode | Throughput | Pays |
|---|---|---|
| `:hold` (default) | about 15-20k `#eval`/s in aggregate, at any Thread count | nothing extra |
| `:release` | scales compute-bound scripts with Threads | a handoff at every Service call |

The mode is fixed per Sandbox and changes scheduling only. Isolation, Handles,
captures, and outcomes stay the same. Dispatch-heavy scripts match or trail
`:hold` under `:release`; `rake bench:gvl_scheduling` measures both on your
hardware.

## Pooling

`Kobako::Pool` keeps a bounded set of warm, identically set-up Sandboxes. It
hands each one to a single holder at a time.

```ruby
pool = Kobako::Pool.new(slots: 4) do |sandbox|
  sandbox.bind("KV::Lookup", ->(key) { redis.get(key) })
end

pool.with { |sandbox| sandbox.eval('KV::Lookup.call("user_42")').value }
```

| Option | Meaning | Default |
|---|---|---|
| `slots:` | the most Sandboxes it builds | required |
| `checkout_timeout:` | seconds `#with` waits; `nil` waits forever | 5.0 |
| any `Sandbox.new` keyword | passed to each Sandbox | — |

A Pool builds each Sandbox on first demand, and the block sets it up once.
`#with` returns the block's value, and the Sandbox goes back to the pool
whatever the block raised. A checkout that waits past `checkout_timeout`
raises `Kobako::PoolTimeoutError`. There is no teardown verb, since a Pool
releases everything with its own reachability.

### Pool Trade-offs

A Pool is experimental. It gives warm reuse, not isolation or throughput.

| It gives you | It does not give you |
|---|---|
| Sandboxes set up once, checked out exclusively | isolation beyond what a Sandbox already has |
| a bound on how many exist | a faster Invocation |

A Pool saves only the host-side `Sandbox.new`, about 3 µs. Each script still
compiles and runs per Invocation, an order of magnitude more.

## Rust SDK

An `Arc<Sandbox>` serves concurrent `&self` invocations from many threads. The
SDK has no Pool and no GVL; the borrow checker proves the shared setup is only
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
