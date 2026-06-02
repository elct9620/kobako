# Host-side Async I/O

A self-contained script that overlaps many Sandboxes' external I/O on a **single OS thread** using a Fiber scheduler (the [`async`](https://github.com/socketry/async) gem). It is the I/O-bound companion to the [serverless demo](../serverless/README.md), whose Concurrency section explains why a pure-compute workload gains nothing from a Fiber server — this one shows the other half of the story: when the work waits on external I/O, a reactor lets one thread carry many in-flight requests at once.

## The shape, and why it has to be this shape

kobako's wasm dispatch is synchronous and runs under the GVL, and a Runtime holds at most one active Invocation per OS thread (`SPEC.md` lists async / yield-resume execution as out of scope). So you cannot do blocking I/O *inside* a Service dispatch and then yield the fiber around it — the wasm frame is still on the native stack, and suspending it would corrupt the per-thread invocation state.

The safe arrangement is host-orchestrated continuation: the guest stays a pure function, and every external fetch happens in host Ruby *between* invocations, where no wasm frame sits on the stack. At that point the fiber scheduler is free to suspend the waiting request and run a peer's compute.

```
guest #run(:Plan, id)      ->  pure compute: decide what to fetch
--- invocation unwound; no wasm frame on the native stack ---
host fetch (async I/O)     ->  the fiber yields here; another request runs
guest #run(:Render, data)  ->  pure compute: build the final result
```

Each `#run` is a short synchronous compute that never yields; the waiting lives entirely in the host gap between the two phases.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/async-io/app.rb                  # async reactor, single thread
ruby examples/async-io/app.rb --sequential     # blocking baseline
ruby examples/async-io/app.rb --count 8 --latency 250
```

From a clone of the kobako repository, prefix with `bundle exec` so the local checkout is used. CLI parsing runs before `bundler/inline`, so `--sequential` does not pull in the `async` dependency it would not use.

## Configuration

| Flag           | Purpose                                                            | Default |
|----------------|-------------------------------------------------------------------|---------|
| `--count N`    | Number of concurrent requests.                                    | `5`     |
| `--latency MS` | Simulated fetch latency per request, in milliseconds.             | `300`   |
| `--sequential` | Blocking baseline: no reactor, so the fetches serialize.          | off     |

## What to observe

The async run interleaves every request's plan and fetch on one thread, then completes them together once the overlapped waits elapse:

```
$ ruby examples/async-io/app.rb --count 4 --latency 200
mode: async reactor (single thread)

[+  628ms] [thread 584] [req 0] plan:start
[+  630ms] [thread 584] [req 0] fetch:start  resource/0
[+  630ms] [thread 584] [req 1] plan:start
[+  631ms] [thread 584] [req 1] fetch:start  resource/1
[+  631ms] [thread 584] [req 2] fetch:start  resource/2
[+  631ms] [thread 584] [req 3] fetch:start  resource/3
[+  831ms] [thread 584] [req 0] fetch:done
...
total wall time:          205ms
serialized would spend:   800ms (count x latency)
```

Two things to read off the trace. Every line carries the same thread id, so the four requests genuinely share one OS thread rather than fanning out to a thread pool. And the total wall time tracks one fetch latency, not their sum — the reactor overlapped the four waits. Running `--sequential` shows the same workload taking `count x latency`, because without a reactor each fetch fully blocks the thread.

## Why this is safe

At the `host fetch` step the script is between invocations: no wasmtime frame is on the native stack, no dispatch is active, and no Invocation occupies the thread slot. That is the only window in which a fiber may suspend without violating the per-OS-thread single-invocation invariant. Keeping I/O out of the dispatch frame is what makes the reactor overlap legal — it is a property of the program structure, not of the scheduler.

Each concurrent request owns its own Sandbox. A Sandbox carries host-side per-invocation state, so two fibers must never share one — the same exclusive-use rule the serverless example's pool enforces.

## Caveat

The fetch latency is simulated with `sleep` (intercepted by the fiber scheduler), so the script is a demonstration of the concurrency structure, not a benchmark. Real overlap depends on the I/O library being fiber-scheduler-aware; a blocking client that ignores the scheduler would serialize even under the reactor.
