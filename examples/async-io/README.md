# Async IO Demo

A self-contained script that overlaps many invocations' external I/O on a **single OS thread** using a Fiber scheduler (the [`async`](https://github.com/socketry/async) gem). It is the I/O-bound companion to the [serverless demo](../serverless/README.md), whose pure-compute scripts leave a Fiber server nothing to overlap.

| Workload | A Fiber reactor |
|---|---|
| pure compute, as in serverless | has nothing to overlap |
| waits on external I/O, as here | carries many in-flight requests on one thread |

## Request Shape

Each request is one `#run`. The guest plans and renders, and a Service it calls does the fetch:

```
guest  Request.(id)              ->  plan: decide what to fetch
host   Resource::Fetcher.fetch   ->  the fiber yields here; another request runs
guest  (same run)                ->  render the final result
```

Each guest runs on a thread kobako keeps. So a Fiber whose run waits inside a Service leaves its own Thread free for another Fiber's run. A Service waiting on scheduler-aware I/O thus yields the Fiber, and the reactor runs a peer meanwhile.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/async-io/app.rb                  # async reactor, single thread
ruby examples/async-io/app.rb --sequential     # blocking baseline
ruby examples/async-io/app.rb --count 8 --latency 250
```

From a clone of the kobako repository, prefix with `bundle exec` so the local checkout is used. CLI parsing runs before `bundler/inline`, so `--sequential` does not pull in the `async` dependency it would not use.

## Configuration

Three flags shape the workload. Each request's fetch sleeps for the given latency.

| Flag           | Purpose                                                 | Default |
|----------------|---------------------------------------------------------|---------|
| `--count N`    | Number of concurrent requests                           | `5`     |
| `--latency MS` | Simulated fetch latency per request, in milliseconds    | `300`   |
| `--sequential` | Blocking baseline: no reactor, so the fetches serialize | off     |

## Expected Trace

The async run starts every request and its fetch on one thread, then completes them together once the overlapped waits elapse:

```
$ ruby examples/async-io/app.rb --count 4 --latency 200
mode: async reactor (single thread)

[+    2ms] [thread 648] [req 0] run:start
[+    3ms] [thread 648] [req 0] fetch:start  resource/0
[+    3ms] [thread 648] [req 1] run:start
[+    3ms] [thread 648] [req 1] fetch:start  resource/1
[+    3ms] [thread 648] [req 2] run:start
[+    3ms] [thread 648] [req 2] fetch:start  resource/2
[+    3ms] [thread 648] [req 3] run:start
[+    3ms] [thread 648] [req 3] fetch:start  resource/3
[+  205ms] [thread 648] [req 0] fetch:done
...
total wall time:          203ms
serialized would spend:   800ms (count x latency)
```

Every line carries the same thread id, so the four requests share one OS thread. The total wall time tracks one fetch latency, not their sum, as the reactor overlapped the four waits. `--sequential` takes `count x latency`, because without a reactor each fetch blocks the thread.

## Shared Sandbox

All requests run on one Sandbox, with the fetching Service bound once:

```ruby
sandbox = Kobako::Sandbox.new
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
