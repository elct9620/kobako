# Serverless Demo

A self-contained Rack application that dispatches `GET /:name` to an operator-supplied mruby script. Each route's script defines a named entrypoint constant. The app preloads it into a fresh `Kobako::Sandbox` per request and calls it as shown below.

```
GET /hello ─> Sandbox.new ─> #preload(Hello) ─> #run(:Hello, GuestRequest) ─> [status, headers, body]
                                                         │
                                       req.params ─> Handle ─> host Rack::Request
```

It demonstrates `#preload` plus `#run(:Entrypoint, ...)` with kobako's host→guest auto-wrap. The request is a `Rack::Request` subclass with no wire form. So kobako allocates a `Kobako::Handle` for it, and each Rack call the guest makes round-trips to the host. Every invocation gets a freshly created, Wasm-isolated mruby interpreter (`mrb_state`).

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/serverless/app.rb                # Puma (default)
ruby examples/serverless/app.rb --type falcon
```

From a clone of the kobako repository, prefix with `bundle exec` so the local kobako checkout is used.

First launch downloads `kobako`, `rack`, `rackup`, and the chosen server gem. CLI parsing runs before `bundler/inline` resolves the inline Gemfile, so `--type falcon` does not also pull in Puma's dependency tree.

```bash
$ ruby examples/serverless/app.rb --type puma
Serverless demo on http://127.0.0.1:9292 (handler: Rackup::Handler::Puma)
```

## Configuration

One flag picks the server, and two variables pick where it listens. An unknown `--type` fails at startup.

| Flag / Variable | Purpose                         | Default     |
|-----------------|---------------------------------|-------------|
| `--type TYPE`   | Rack handler: `puma` or `falcon` | `puma`      |
| `HOST` env      | Interface the server binds to   | `127.0.0.1` |
| `PORT` env      | TCP port the server listens on  | `9292`      |

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

## Custom Scripts

Edit the `Serverless::ROUTES` Hash in `app.rb`. Each key is the URL segment after `/`, and each value is an `[Entrypoint, source]` pair. `Entrypoint` is the constant `source` defines, both the `#preload` name and the `#run` target. It is a callable taking one argument, a `Kobako::Handle` proxy of the host-side `Rack::Request`:

```ruby
"greet" => [:Greet, <<~'MRUBY'],
  Greet = ->(req) {
    who = req.params["who"] || "stranger"
    [200, { "content-type" => "text/plain" }, ["howdy, #{who}\n"]]
  }
MRUBY
```

The script must return a Rack 3 triplet: an Integer status, a Hash of lowercase-keyed String headers, and an Array of String body parts. Anything else raises on the host side after the guest returns.

### Request Handle

The guest sees the request as a Handle, not as data. Each call on `req` is one round-trip to the real request object on the host. A Handle exposes only what its object declares, so `GuestRequest` names the callable methods through a private `respond_to_guest?`:

| Call in guest          | Runs on host                   | Returns               |
|------------------------|--------------------------------|-----------------------|
| `req.request_method`   | `Rack::Request#request_method` | `"GET"`               |
| `req.path`             | `Rack::Request#path`           | `"/hello"`            |
| `req.params`           | `Rack::Request#params`         | `Hash[String,String]` |
| `req.get_header("…")`  | `Rack::Request#get_header`     | `String` or `nil`     |

Each access costs about 5.8 µs amortised (kobako benchmark `2d-1000-calls-in-one-eval`). Nothing the script skips is marshalled, so the cost lands at access time. A script that reads the request many times keeps `req.params` in a local.

The Handle is invalidated when the invocation ends, so a script cannot stash `req` for the next request.

## Switching the application server

The `--type` flag controls which Rack handler the demo starts. The CLI is parsed before `bundler/inline` runs, and the chosen gem name is interpolated into the inline Gemfile, so only that server is installed on first launch.

```bash
ruby examples/serverless/app.rb --type puma
ruby examples/serverless/app.rb --type falcon
```

The same `Serverless::App` and the same wire path serve both — switching handlers exercises Rack 3 compatibility without changing any application code.

## Per-request Sandboxes

Each `GET /:name` constructs a fresh `Kobako::Sandbox`, preloads one snippet, and invokes it once. The boot state is baked into the artifact, so the request pays little for it:

| Step | Cost |
|---|---|
| `Sandbox.new`, warm | about 3 µs |
| `#preload` plus the `#run` that replays the snippet | about 80 µs |

The construction figure is the `Sandbox.new` steady state in the root [`benchmark/README.md`](../../benchmark/README.md).

Every `#run` executes against a fresh `mrb_state`, whether its Sandbox is new or reused. So a script's globals, instance variables, and class-level changes never reach the next invocation. A Sandbox holds no state from any run, so Threads may share one. The [multi-tenant demo](../multi-tenant/README.md) takes that route, each invocation naming its own identity.

## Security caveats

This demo binds to `127.0.0.1` by default so the server is not reachable from the network. The mruby guest has no I/O, network, sleep, or random-seed gems built in (see `build_config/wasi.rb` for the allowlist), and no Services are bound — guest scripts can compute over the request env and that is all. Adding capabilities means binding host objects via `Sandbox#bind("MyService::KV", object)`, at which point the operator owns the trust boundary; see `examples/codemode/` for that pattern.

## Appendix: Puma vs Falcon under this design

Falcon is a Fiber-based reactor server and Puma is a Thread-pool server, so a natural question is whether the demo gains throughput by switching to Falcon. The short answer for *this* design is no — Puma is ~20-25% faster at every concurrency, because the bottleneck is not what either server is good at improving and the per-request RPC round-trips amplify Falcon's disadvantage.

Measured with `ab -n 3000 -c <conc>` against `GET /hello?name=alice` on macOS arm64, Ruby 3.4.7, YJIT off, kobako 0.26.0, both servers single-process with a per-request Sandbox, on the `Rack::Request`-as-Handle design (so each request also pays one in-script `req.params` Handle round-trip back to the host). Each server takes a 500-request warm-up first, and each cell is the median of three runs taken in alternating order across the two servers — zero failed requests in every run:

| Concurrency | Puma req/s | Puma p99 | Falcon req/s | Falcon p99 |
|-------------|------------|----------|--------------|------------|
| 1           | 4,220      | <1 ms    | 3,560        | 1 ms       |
| 10          | 4,940      | 2 ms     | 4,040        | 3 ms       |
| 50          | 4,960      | 17 ms    | 4,050        | 14 ms      |
| 100         | 4,980      | 22 ms    | 3,980        | 31 ms      |

Puma plateaus at ~4.95k req/s and Falcon at ~4.0k req/s from `c=10` upwards. Beyond that, additional concurrency only lengthens the queue — p99 latency rises roughly linearly with concurrency on both sides — while throughput stays flat. Puma stays ~19-25% faster than Falcon at every concurrency tested; the gap holds roughly steady, widening slightly at `c=100`. Falcon's tail latency is not uniformly worse — at `c=50` the two sit within a few ms of each other — but at `c=100` its p99 is ~31 ms against Puma's ~22 ms.

Two properties of this design suppress Falcon's Fiber advantage:

1. **The wasm segment blocks the thread that runs it.** An invocation is a synchronous call, so the fiber that issued it cannot yield until the guest returns — Falcon's reactor has nothing to overlap. Opting into `gvl: :release`, which drops the GVL while the guest runs, would let *other Threads* run their guest code in parallel, which is a Puma-shaped win; a single-threaded reactor has no other Thread to give the released time to.
2. **The demo scripts perform no I/O.** Falcon wins when application code blocks on external I/O — an outbound HTTP fetch, a database round-trip, a slow upstream — because the Fiber yields and the reactor schedules another request. The `/hello`, `/sum`, `/shout`, `/echo` scripts here are pure computation, so there is nothing for the scheduler to overlap.

Either property alone would already cap the win; both together flatten it. A design that binds I/O-doing Services (a Service that waits on real I/O releases the GVL inside Ruby's own I/O call) onto a Sandbox, and runs many requests that wait on those callbacks, would let Falcon overlap the wait windows across requests — and that is where the two servers' designs diverge. The demo deliberately stays in the pure-compute corner so the wire path is easy to read, which makes the corner the wrong place to choose Falcon over Puma.
