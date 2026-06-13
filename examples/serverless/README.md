# Serverless Demo

A self-contained Rack application that dispatches `GET /:name` to an operator-supplied mruby script. Each route's script defines a named entrypoint constant; the app preloads it into a `Kobako::Sandbox` and invokes it via `#run(:Entrypoint, Rack::Request.new(env))`, and the script returns a Rack response triplet `[status, headers, body]` directly. The Sandbox is either built fresh per request (default) or reused from a pool of preloaded Sandboxes (`--pool`).

This is the canonical demonstration of the `#preload` + `#run(:Entrypoint, ...)` pattern combined with kobako's host→guest auto-wrap (SPEC B-34): the `Rack::Request` is not wire-representable, so kobako transparently allocates a `Kobako::Handle` for it and the guest interacts with the request through normal Rack API calls that round-trip back to the host as RPC. A fixed set of entrypoints, many user scripts behind them, and a Wasm-isolated mruby interpreter (`mrb_state`) freshly created for every invocation.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/serverless/app.rb                # Puma, per-request sandbox (default)
ruby examples/serverless/app.rb --type falcon
ruby examples/serverless/app.rb --pool         # reuse a pool of preloaded Sandboxes
ruby examples/serverless/app.rb --pool --pool-size 8
```

From a clone of the kobako repository, prefix with `bundle exec` so the local kobako checkout is used.

First launch downloads `kobako`, `rack`, `rackup`, and the chosen server gem. CLI parsing runs before `bundler/inline` resolves the inline Gemfile, so `--type falcon` does not also pull in Puma's dependency tree.

```bash
$ ruby examples/serverless/app.rb --type puma
Serverless demo on http://127.0.0.1:9292 (handler: Rackup::Handler::Puma)
```

## Configuration

| Flag / Variable | Purpose                                                                                      | Default     |
|-----------------|----------------------------------------------------------------------------------------------|-------------|
| `--type TYPE`   | Rack handler to start. Accepts `puma` or `falcon`. Unknown values fail.                      | `puma`      |
| `--pool`        | Reuse a pool of preloaded Sandboxes instead of building one per request.                     | off         |
| `--pool-size N` | Number of pooled Sandboxes when `--pool` is set. Ignored otherwise.                          | `5`         |
| `HOST` env      | Interface the server binds to.                                                               | `127.0.0.1` |
| `PORT` env      | TCP port the server listens on.                                                              | `9292`      |

## Trying it out

```bash
# List available scripts
curl http://127.0.0.1:9292/

# Greet the world (or a named guest)
curl http://127.0.0.1:9292/hello
curl 'http://127.0.0.1:9292/hello?name=alice'

# Echo the request env the guest received
curl 'http://127.0.0.1:9292/echo?foo=bar'

# Do arithmetic
curl 'http://127.0.0.1:9292/sum?a=2&b=40'

# Shout a message back
curl 'http://127.0.0.1:9292/shout?msg=ready'
```

Unknown routes return `404`, and non-GET requests return `405 Method Not Allowed`. Any error inside the guest — `SandboxError` (script raised), `ServiceError` (no services bound here, but reserved), or `TrapError` (timeout / memory cap) — is rendered as `500` with the error class and message in the body.

## Adding your own script

Edit the `Serverless::ROUTES` Hash in `app.rb`. Each entry's key is the URL segment after `/`, and the value is a `[Entrypoint, source]` pair: `Entrypoint` is the constant the `source` defines (used as both the `#preload` name and the `#run` target), and the entrypoint is a callable accepting one argument — a `Kobako::Handle` proxy of the host-side `Rack::Request`. Names must be unique across routes so a single pooled Sandbox can preload them all:

```ruby
"greet" => [:Greet, <<~'MRUBY'],
  Greet = ->(req) {
    who = req.params["who"] || "stranger"
    [200, { "content-type" => "text/plain" }, ["howdy, #{who}\n"]]
  }
MRUBY
```

The guest does not see the Rack env as data — it sees a Handle, and every method call on `req` dispatches back to the host as one RPC round-trip against the real `Rack::Request` instance. That means the full Rack 3 request API is available, but each access pays a guest→host round-trip (~6.8 µs amortised, kobako benchmark `2a-empty-call`):

| Call in guest          | Runs on host                 | Returns                  |
|------------------------|------------------------------|--------------------------|
| `req.request_method`   | `Rack::Request#request_method` | `"GET"`                 |
| `req.path`             | `Rack::Request#path`           | `"/hello"`              |
| `req.params`           | `Rack::Request#params`         | `Hash[String,String]`   |
| `req.get_header("…")` | `Rack::Request#get_header`     | `String` or `nil`       |

The Handle is invalidated at the end of the invocation (SPEC B-18), so a script cannot stash `req` for the next request — the host owns the lifecycle. Anything the script does not call is never marshalled, so passing the full `Rack::Request` costs no extra wire bytes upfront; the cost lands at access time. For scripts that touch only one or two fields this is a wash against the older "build a small Hash up front" shape; for scripts that touch the request many times, cache the result of `req.params` (or any other method that returns a wire-representable Hash) in a local to avoid repeated round-trips.

The script must return a Rack 3 triplet: an Integer status, a Hash of lowercase-keyed String headers, and an Array of String body parts. Anything else raises on the host side after the guest returns.

## Switching the application server

The `--type` flag controls which Rack handler the demo starts. The CLI is parsed before `bundler/inline` runs, and the chosen gem name is interpolated into the inline Gemfile, so only that server is installed on first launch.

```bash
ruby examples/serverless/app.rb --type puma
ruby examples/serverless/app.rb --type falcon
```

The same `Serverless::App` and the same wire path serve both — switching handlers exercises Rack 3 compatibility without changing any application code.

## Per-request vs pooled sandboxes

By default each `GET /:name` constructs a fresh `Kobako::Sandbox`, preloads exactly one snippet, and invokes it once. Under B-49 the canonical boot state is baked into the artifact, so warm construction is only ~30 µs (`Sandbox.new` steady state, root [`benchmark/README.md`](../../benchmark/README.md)) and the per-request `#preload` (one-snippet compile) plus `#run` dispatch adds ~100 µs on top. Each `req.params` / `req.request_method` / `req.path` call is a guest→host Handle round-trip — a few µs amortised — the trade kobako's auto-wrap (SPEC B-34) makes in exchange for not marshalling the Rack env into a wire-friendly Hash up front. Scripts that read the request once and cache locally pay that cost once; scripts that re-read repeatedly should hoist the result into a local variable.

When the script set is fixed and the process serves many requests, rebuilding the Sandbox every time is wasted work. `--pool` builds a `Kobako::Pool` of long-lived Sandboxes — each constructed lazily on first checkout and preloaded with *every* route's entrypoint in the Pool's setup block — and a request checks one out and dispatches `#run` on it, leaving construction and preload off the hot path. The dispatch and RPC costs are unchanged; only the ~30 µs construction plus the per-snippet compile disappear from each request — a much thinner slice than the ~125 µs the pre-B-49 design paid, which is why the measured pooling win is now single-digit percent (see the appendix) rather than the large gap that design showed.

Pooling does not weaken isolation. Every `#run` executes against a fresh `mrb_state` whether the Sandbox is new or reused, so a script's globals, instance variables, and class-level mutation never survive into the next invocation on the same Sandbox (SPEC B-03; pinned by the J-02 reuse test in `test/e2e/test_lifecycle.rb`). What the pool reuses is the *host-side* Sandbox object, and `Kobako::Pool` hands each request one exclusively-held Sandbox rather than as an optimisation: that object carries per-invocation capture buffers and a Handle table, so two threads sharing one Sandbox would interleave each other's output and Handles. The pool hands each request its own Sandbox for the duration of the call; when all are busy, a request waits up to the Pool's `checkout_timeout` (5 s by default) and, on expiry, `#with` raises `Kobako::PoolTimeoutError`, which the app renders as `503`.

## Security caveats

This demo binds to `127.0.0.1` by default so the server is not reachable from the network. The mruby guest has no I/O, network, sleep, or random-seed gems built in (see `build_config/wasi.rb` for the allowlist), and no Services are bound — guest scripts can compute over the request env and that is all. Adding capabilities means binding host objects via `Sandbox#define(...).bind(...)`, at which point the operator owns the trust boundary; see `examples/codemode/` for that pattern.

## Appendix: per-request vs pooled throughput

These numbers are a *perceived* comparison for orienting a sizing decision, not a gated guarantee — they are hardware-dependent and are not part of the `rake bench` suite. Boot each mode and drive it with `ab` against the same route:

```bash
# per-request (default)
ruby examples/serverless/app.rb &
ab -n 2000 -c 10 'http://127.0.0.1:9292/hello?name=alice'

# pooled
ruby examples/serverless/app.rb --pool --pool-size 5 &
ab -n 2000 -c 10 'http://127.0.0.1:9292/hello?name=alice'
```

Measured with `ab -n 2000 -c <conc>` against `GET /hello?name=alice` on macOS arm64, Ruby 3.4.7, YJIT off, single-process Puma (5 threads), pool size 5 — zero failed requests in every cell:

| Concurrency | Per-request req/s | Per-request p99 | Pooled req/s | Pooled p99 |
|-------------|-------------------|-----------------|--------------|------------|
| 1           | 3,580             | 1 ms            | 3,720        | 1 ms       |
| 10          | 3,900             | 3 ms            | 4,080        | 3 ms       |
| 50          | 3,880             | 18 ms           | 4,060        | 13 ms      |

The wasm segment runs under the GVL ([root README §Performance](../../README.md#performance)), so `#run` is serialised process-wide in both modes and throughput still plateaus once concurrency exceeds the thread count. Pooling raises *where* that plateau sits — ~4-5% here — because `Sandbox.new` plus the per-request snippet compile is itself work done under the GVL: removing it from every request frees serialised time that the process spends serving more requests instead, and p99 stays level or slightly tighter. The win is small because B-49 already shrank `Sandbox.new` to ~30 µs, so what pooling removes is a thin slice of each request rather than the dominant cost it was pre-bake. It is a per-request constant, not a concurrency effect, so the small edge persists under load rather than washing out. A pool sized below the server's thread count reintroduces a queue on checkout; sizing it to the thread count (Puma's default is 5) is the sane starting point.

## Appendix: Puma vs Falcon under this design

Falcon is a Fiber-based reactor server and Puma is a Thread-pool server, so a natural question is whether the demo gains throughput by switching to Falcon. The short answer for *this* design is no — Puma is ~20% faster at every concurrency, because the bottleneck is not what either server is good at improving and the per-request RPC round-trips amplify Falcon's disadvantage.

Measured with `ab -n 3000 -c <conc>` against `GET /hello?name=alice` on macOS arm64, Ruby 3.4.7, YJIT off, both servers single-process, on the B-34 `Rack::Request`-as-Handle design (so each request also pays one in-script `req.params` Handle round-trip back to the host):

| Concurrency | Puma req/s | Puma p99 | Falcon req/s | Falcon p99 |
|-------------|------------|----------|--------------|------------|
| 1           | 3,260      | 1 ms     | 2,750        | 1 ms       |
| 10          | 3,850      | 3 ms     | 3,120        | 5 ms       |
| 50          | 3,860      | 15 ms    | 3,170        | 20 ms      |
| 100         | 3,820      | 30 ms    | 3,150        | 37 ms      |

Puma plateaus at ~3.85k req/s and Falcon at ~3.15k req/s from `c=10` upwards. Beyond that, additional concurrency only lengthens the queue — p99 latency rises roughly linearly with concurrency on both sides — while throughput stays flat. Puma stays ~18-23% faster than Falcon at every concurrency tested; the gap is stable rather than widening or closing under load. Falcon's tail latency is modestly worse — at `c=100` its p99 is ~37 ms against Puma's ~30 ms — but now that B-49 keeps each request's time in the GVL-held segment short, the dramatic tail blow-up the pre-bake design showed is gone.

Two properties of this design suppress Falcon's Fiber advantage:

1. **The wasm segment runs under the GVL.** The kobako native extension does not release the GVL during the `wasmtime` call ([root README §Performance](../../README.md#performance)). Every request's `#preload` + `#run` is serialised across the whole process, whether the server hands the request off via a Thread or a Fiber. Falcon's reactor cannot yield around a GVL-held segment.
2. **The demo scripts perform no I/O.** Falcon wins when application code blocks on external I/O — an outbound HTTP fetch, a database round-trip, a slow upstream — because the Fiber yields and the reactor schedules another request. The `/hello`, `/sum`, `/shout`, `/echo` scripts here are pure computation, so there is nothing for the scheduler to overlap.

Either property alone would already cap the win; both together flatten it. A design that binds I/O-doing Services (kobako Services *do* release the GVL while a host callback runs) onto a Sandbox, and runs many requests that wait on those callbacks, would let Falcon overlap the wait windows across requests — and that is where the two servers' designs diverge. The demo deliberately stays in the pure-compute corner so the wire path is easy to read, which makes the corner the wrong place to choose Falcon over Puma.
