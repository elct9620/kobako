# Serverless Demo

A self-contained Rack application that dispatches `GET /:name` to an operator-supplied mruby script. Each request constructs a fresh `Kobako::Sandbox`, preloads the script as the `:App` entrypoint, and invokes it with a `Rack::Request` wrapping the Rack env. The script returns a Rack response triplet `[status, headers, body]` directly.

This is the canonical demonstration of the `#preload` + `#run(:Entrypoint, ...)` pattern combined with kobako 0.4.0's host→guest auto-wrap (SPEC B-34): the `Rack::Request` is not wire-representable, so kobako transparently allocates a `Kobako::Handle` for it and the guest interacts with the request through normal Rack API calls that round-trip back to the host as RPC. A fixed protocol (the `App` constant), many user scripts behind it, and one fresh Wasm-isolated mruby interpreter per request.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/serverless/app.rb                # Puma (default)
ruby examples/serverless/app.rb --type puma
ruby examples/serverless/app.rb --type falcon
```

From a clone of the kobako repository, prefix with `bundle exec` so the local kobako checkout is used.

First launch downloads `kobako`, `rack`, `rackup`, and the chosen server gem. CLI parsing runs before `bundler/inline` resolves the inline Gemfile, so `--type falcon` does not also pull in Puma's dependency tree.

```bash
$ ruby examples/serverless/app.rb --type puma
Serverless demo on http://127.0.0.1:9292 (handler: Rackup::Handler::Puma)
```

## Configuration

| Flag / Variable | Purpose                                                                 | Default     |
|-----------------|-------------------------------------------------------------------------|-------------|
| `--type TYPE`   | Rack handler to start. Accepts `puma` or `falcon`. Unknown values fail. | `puma`      |
| `HOST` env      | Interface the server binds to.                                          | `127.0.0.1` |
| `PORT` env      | TCP port the server listens on.                                         | `9292`      |

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

Edit the `Serverless::ROUTES` Hash in `app.rb`. Each entry's key is the URL segment after `/`, and the value is an mruby source string that defines `App` as a callable accepting one argument — a `Kobako::Handle` proxy of the host-side `Rack::Request`:

```ruby
"greet" => <<~MRUBY
  App = ->(req) {
    who = req.params["who"] || "stranger"
    [200, { "content-type" => "text/plain" }, ["howdy, #{who}\n"]]
  }
MRUBY
```

The guest does not see the Rack env as data — it sees a Handle, and every method call on `req` dispatches back to the host as one RPC round-trip against the real `Rack::Request` instance. That means the full Rack 3 request API is available, but each access costs ~140 µs of RPC dispatch (kobako benchmark `2a-empty-rpc`):

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

## Why per-request sandboxes

Each `GET /:name` constructs a fresh `Kobako::Sandbox`, preloads exactly one snippet, and invokes it once. Concurrent requests therefore cannot share guest state through globals, instance variables, or class-level mutation — every request gets its own `mrb_state`. This is the strongest isolation kobako offers, and it is cheap: warm Sandbox construction is ~125 µs and the per-request setup adds ~160 µs of `#run` dispatch on top.

Per-script `req.params` / `req.request_method` / `req.path` calls add ~140 µs each of RPC round-trip on top of the dispatch cost — the trade kobako 0.4.0's auto-wrap (SPEC B-34) makes in exchange for not having to marshal the Rack env into a wire-friendly Hash up front. Scripts that read the request once and cache locally pay that cost once; scripts that re-read repeatedly should hoist the result into a local variable.

If your workload is the opposite shape — a stable set of entrypoints, many invocations per process — preload all snippets once at boot into a long-lived Sandbox and dispatch via `#run` per request. The dispatch cost stays at ~160 µs and the Sandbox construction lands off the hot path.

## Security caveats

This demo binds to `127.0.0.1` by default so the server is not reachable from the network. The mruby guest has no I/O, network, sleep, or random-seed gems built in (see `build_config/wasi.rb` for the allowlist), and no Services are bound — guest scripts can compute over the request env and that is all. Adding capabilities means binding host objects via `Sandbox#define(...).bind(...)`, at which point the operator owns the trust boundary; see `examples/codemode/` for that pattern.

## Appendix: Puma vs Falcon under this design

Falcon is a Fiber-based reactor server and Puma is a Thread-pool server, so a natural question is whether the demo gains throughput by switching to Falcon. The short answer for *this* design is no — Puma is ~20-30% faster at every concurrency, because the bottleneck is not what either server is good at improving and the per-request RPC round-trips amplify Falcon's disadvantage.

Measured with `ab -n 3000 -c <conc>` against `GET /hello?name=alice` on macOS arm64, Ruby 3.4.7, YJIT off, both servers single-process, on the 0.4.0 / B-34 `Rack::Request`-as-Handle design (so each request also pays one in-script `req.params` RPC round-trip, ~140 µs):

| Concurrency | Puma req/s | Puma p99 | Falcon req/s | Falcon p99 |
|-------------|------------|----------|--------------|------------|
| 1           | 1,882      | 2 ms     | 1,429        | 2 ms       |
| 10          | 2,268      | 6 ms     | 1,696        | 8 ms       |
| 50          | 2,213      | 27 ms    | 1,734        | 49 ms      |
| 100         | 2,083      | 56 ms    | 1,748        | 81 ms      |

Puma plateaus at ~2.2k req/s and Falcon at ~1.7k req/s from `c=10` upwards. Beyond that, additional concurrency only lengthens the queue — p99 latency rises roughly linearly with concurrency on both sides — while throughput stays flat. Puma stays ~19-34% faster than Falcon at every concurrency tested; the gap narrows under load (the queue length dominates both sides) but does not close. Falcon's tail latency is the more notable difference: at `c=50` Falcon's p99 jumps to 49 ms while p98 is only 37 ms, and at `c=100` p99=81 ms vs p95=68 ms — Puma's tail stays tight (p99=56 ms vs p95=54 ms at the same load).

Two properties of this design suppress Falcon's Fiber advantage:

1. **The wasm segment runs under the GVL.** The kobako native extension does not release the GVL during the `wasmtime` call ([root README §Concurrency](../../README.md#concurrency)). Every request's `#preload` + `#run` is serialised across the whole process, whether the server hands the request off via a Thread or a Fiber. Falcon's reactor cannot yield around a GVL-held segment.
2. **The demo scripts perform no I/O.** Falcon wins when application code blocks on external I/O — an outbound HTTP fetch, a database round-trip, a slow upstream — because the Fiber yields and the reactor schedules another request. The `/hello`, `/sum`, `/shout`, `/echo` scripts here are pure computation, so there is nothing for the scheduler to overlap.

Either property alone would already cap the win; both together flatten it. A design that binds I/O-doing Services (kobako Services *do* release the GVL while a host callback runs) onto a Sandbox, and runs many requests that wait on those callbacks, would let Falcon overlap the wait windows across requests — and that is where the two servers' designs diverge. The demo deliberately stays in the pure-compute corner so the wire path is easy to read, which makes the corner the wrong place to choose Falcon over Puma.
