# Async IO Demo

A self-contained script that overlaps many invocations' external I/O on a **single OS thread** using a Fiber scheduler (the [`async`](https://github.com/socketry/async) gem). It is the I/O-bound companion to the [serverless demo](../serverless/README.md), whose pure-compute scripts leave a Fiber server nothing to overlap. `--compute` shows the other side: what it takes for runs that compute to overlap.

| Workload | A Fiber reactor |
|---|---|
| waits on external I/O in a Service, as here | carries many in-flight requests on one thread |
| computes in the guest, with `--compute` | overlaps it only under the conditions in [Fiber Overlap](#fiber-overlap) |

## Request Shape

Each request is one `#run`. The guest plans and renders, and a Service it calls does the fetch:

```
guest  Request.(id)              ->  plan         (a)
host   Resource::Fetcher.fetch   ->  fiber yields (b)
guest  (same run)                ->  render       (c)
```

a. Decide what to fetch.
b. Another request runs while this one waits.
c. Build the final result.

Each guest runs on a thread kobako keeps. So a Fiber whose run waits inside a Service leaves its own Thread free for another Fiber's run. A Service waiting on scheduler-aware I/O thus yields the Fiber, and the reactor runs a peer meanwhile.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/async-io/app.rb                  # async reactor, single thread
ruby examples/async-io/app.rb --sequential     # blocking baseline
ruby examples/async-io/app.rb --count 8 --latency 250
ruby examples/async-io/app.rb --compute        # guests compute; runs serialize
ASYNC_SCHEDULER_WORKER_POOL=true ruby examples/async-io/app.rb --compute --gvl release  # Ruby 4.0: runs overlap
```

From a clone of the kobako repository, prefix with `bundle exec` so the local checkout is used. CLI parsing runs before `bundler/inline`, so `--sequential` does not pull in the `async` dependency it would not use.

## Configuration

Five flags shape the workload. In `--compute` mode each guest loops 4 million times instead of fetching, and one run alone sets the latency the serialized total counts.

| Flag           | Purpose                                                 | Default |
|----------------|---------------------------------------------------------|---------|
| `--count N`    | Number of concurrent requests                           | `5`     |
| `--latency MS` | Simulated fetch latency per request, in milliseconds    | `300`   |
| `--sequential` | Blocking baseline: no reactor, so the requests serialize | off     |
| `--compute`    | Each guest computes instead of calling the Service      | off     |
| `--gvl MODE`   | The Sandbox's `gvl:`: `hold` or `release`               | `hold`  |

## Expected Trace

The async run starts every request and its fetch on one thread, then completes them together once the overlapped waits elapse:

```
$ ruby examples/async-io/app.rb --count 4 --latency 200
mode: async reactor (single thread), gvl: hold, ruby 3.4.7, ASYNC_SCHEDULER_WORKER_POOL=false

[+    2ms] [thread 664] [req 0] run:start
[+    3ms] [thread 664] [req 0] fetch:start  resource/0
[+    3ms] [thread 664] [req 1] run:start
[+    3ms] [thread 664] [req 1] fetch:start  resource/1
[+    3ms] [thread 664] [req 2] run:start
[+    3ms] [thread 664] [req 2] fetch:start  resource/2
[+    3ms] [thread 664] [req 3] run:start
[+    3ms] [thread 664] [req 3] fetch:start  resource/3
[+  203ms] [thread 664] [req 0] fetch:done
...
total wall time:          201ms
serialized would spend:   800ms (count x latency)
```

Every line carries the same thread id, so the four requests share one OS thread. The total wall time tracks one fetch latency, not their sum, as the reactor overlapped the four waits. `--sequential` takes `count x latency`, because without a reactor each fetch blocks the thread.

## Fiber Overlap

Whether a run lets the Thread's other Fibers go on depends on what the run waits for. Five requests, each a 300 ms fetch or about 275 ms of guest compute:

| Each run | Setup | Total for five |
|---|---|---|
| waits in a Service | any Ruby, either `--gvl` | 302–304 ms |
| computes in the guest | `--gvl hold`, with or without a worker pool | 1,373–1,384 ms |
| computes in the guest | `--gvl release`, on Ruby 3.4 or without a worker pool | 1,362–1,384 ms |
| computes in the guest | `--gvl release`, Ruby 4.0, worker pool | 286 ms |

A guest keeping the GVL leaves the Thread's other Fibers waiting until its run ends. A guest releasing it hands its wait to a scheduler offering `blocking_operation_wait`. Async offers that hook with its worker pool on Ruby 4.0, set by `ASYNC_SCHEDULER_WORKER_POOL=true`. A run hands its wait over only once its guest outlasts a short spin, so a brief run never needs the pool.

### Runs Taking Turns

With the guest keeping the GVL, each run starts only once the one before it ends. The total is five runs end to end:

```
$ ruby examples/async-io/app.rb --compute
mode: async reactor (single thread), gvl: hold, ruby 3.4.7, ASYNC_SCHEDULER_WORKER_POOL=false

[+  276ms] [thread 672] [req 0] run:start
[+  549ms] [thread 672] [req 0] run:done     -> work/0 => 4000000
[+  550ms] [thread 672] [req 1] run:start
[+  824ms] [thread 672] [req 1] run:done     -> work/1 => 4000000
[+  824ms] [thread 672] [req 2] run:start
...
total wall time:         1374ms
serialized would spend:  1367ms (count x latency)
```

### Runs Overlapping

With the guest releasing the GVL to Async's worker pool on Ruby 4.0, all five runs start at once and end together. The total is one run:

```
$ ASYNC_SCHEDULER_WORKER_POOL=true ruby examples/async-io/app.rb --compute --gvl release
mode: async reactor (single thread), gvl: release, ruby 4.0.10, ASYNC_SCHEDULER_WORKER_POOL=true

[+  275ms] [thread 688] [req 0] run:start
[+  276ms] [thread 688] [req 1] run:start
...
[+  276ms] [thread 688] [req 4] run:start
[+  560ms] [thread 688] [req 0] run:done     -> work/0 => 4000000
[+  560ms] [thread 688] [req 1] run:done     -> work/1 => 4000000
...
total wall time:          286ms
serialized would spend:  1364ms (count x latency)
```

Both traces stay on one thread id: the overlap comes from the guests, never from more Ruby Threads.

## Shared Sandbox

All requests run on one Sandbox, with the fetching Service bound once:

```ruby
sandbox = Kobako::Sandbox.new(gvl: :hold)
sandbox.bind("Resource::Fetcher", fetcher)
sandbox.preload(code: REQUEST, name: :Request)
Async { 4.times.map { |id| Async { sandbox.run(:Request, id).value } }.map(&:wait) }
```

Each run gets its own guest, so the concurrent Fibers share no guest state. The bound `Fetcher` is reached by every run at once, so it keeps no per-request state.

## Caveat

The fetch latency is simulated with `sleep` (intercepted by the fiber scheduler), so the script demonstrates the concurrency structure and is no benchmark. Real overlap depends on the I/O library the Service calls:

| Fetch client | Under the reactor |
|---|---|
| fiber-scheduler-aware | the waits overlap |
| blocking, ignoring the scheduler | the waits serialize |
