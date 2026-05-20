# Serverless Demo

A self-contained Rack application that dispatches `GET /:name` to an operator-supplied mruby script. Each request constructs a fresh `Kobako::Sandbox`, preloads the script as the `:App` entrypoint, and invokes it with a wire-friendly Rack env. The script returns a Rack response triplet `[status, headers, body]` directly.

This is the canonical demonstration of the `#preload` + `#run(:Entrypoint, ...)` pattern from kobako 0.3.0: a fixed protocol (the `App` constant), many user scripts behind it, and one fresh Wasm-isolated mruby interpreter per request.

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

Edit the `Serverless::ROUTES` Hash in `app.rb`. Each entry's key is the URL segment after `/`, and the value is an mruby source string that defines `App` as a callable accepting one Hash argument:

```ruby
"greet" => <<~MRUBY
  App = ->(env) {
    who = env["query"]["who"] || "stranger"
    [200, { "content-type" => "text/plain" }, ["howdy, #{who}\n"]]
  }
MRUBY
```

The env Hash the guest receives is intentionally minimal:

| Key       | Type                  | Source                           |
|-----------|-----------------------|----------------------------------|
| `method`  | `String`              | `env["REQUEST_METHOD"]`          |
| `path`    | `String`              | `env["PATH_INFO"]`               |
| `query`   | `Hash[String,String]` | Parsed from `env["QUERY_STRING"]`|

Anything else in the Rack env — `rack.input`, `rack.errors`, middleware callables, host objects — is deliberately omitted because it does not survive the host↔guest wire codec. Add explicit keys here if your script needs them.

The script must return a Rack 3 triplet: an Integer status, a Hash of lowercase-keyed String headers, and an Array of String body parts. Anything else raises on the host side after the guest returns.

## Switching the application server

The `--type` flag controls which Rack handler the demo starts. The CLI is parsed before `bundler/inline` runs, and the chosen gem name is interpolated into the inline Gemfile, so only that server is installed on first launch.

```bash
ruby examples/serverless/app.rb --type puma
ruby examples/serverless/app.rb --type falcon
```

The same `Serverless::App` and the same wire path serve both — switching handlers exercises Rack 3 compatibility without changing any application code.

## Why per-request sandboxes

Each `GET /:name` constructs a fresh `Kobako::Sandbox`, preloads exactly one snippet, and invokes it once. Concurrent requests therefore cannot share guest state through globals, instance variables, or class-level mutation — every request gets its own `mrb_state`. This is the strongest isolation kobako offers, and it is cheap: warm Sandbox construction is ~130 µs and the per-request setup adds ~165 µs of `#run` dispatch on top.

If your workload is the opposite shape — a stable set of entrypoints, many invocations per process — preload all snippets once at boot into a long-lived Sandbox and dispatch via `#run` per request. The dispatch cost stays at ~165 µs and the Sandbox construction lands off the hot path.

## Security caveats

This demo binds to `127.0.0.1` by default so the server is not reachable from the network. The mruby guest has no I/O, network, sleep, or random-seed gems built in (see `build_config/wasi.rb` for the allowlist), and no Services are bound — guest scripts can compute over the request env and that is all. Adding capabilities means binding host objects via `Sandbox#define(...).bind(...)`, at which point the operator owns the trust boundary; see `examples/codemode/` for that pattern.

## Appendix: Puma vs Falcon under this design

Falcon is a Fiber-based reactor server and Puma is a Thread-pool server, so a natural question is whether the demo gains throughput by switching to Falcon. The short answer for *this* design is no — both servers plateau at the same number, because the bottleneck is not what either server is good at improving.

Measured with `ab -n 3000 -c <conc>` against `GET /hello?name=alice` on macOS arm64, Ruby 3.4.7, YJIT off, both servers single-process:

| Concurrency | Puma req/s | Puma p99 | Falcon req/s | Falcon p99 |
|-------------|------------|----------|--------------|------------|
| 1           | 2,119      | 2 ms     | 1,725        | 2 ms       |
| 10          | 2,540      | 5 ms     | 2,248        | 6 ms       |
| 50          | 2,501      | 23 ms    | 2,349        | 24 ms      |
| 100         | 2,355      | 55 ms    | 2,306        | 47 ms      |

Both servers saturate at ~2.3-2.5k req/s from `c=10` upwards. Beyond that, additional concurrency only lengthens the queue — p99 latency rises roughly linearly with concurrency on both sides — while throughput stays flat. Puma is slightly faster at `c=1` because its Rack adapter pipeline is shorter than Falcon's; the gap disappears under load.

Two properties of this design suppress Falcon's Fiber advantage:

1. **The wasm segment runs under the GVL.** The kobako native extension does not release the GVL during the `wasmtime` call ([root README §Concurrency](../../README.md#concurrency)). Every request's `#preload` + `#run` is serialised across the whole process, whether the server hands the request off via a Thread or a Fiber. Falcon's reactor cannot yield around a GVL-held segment.
2. **The demo scripts perform no I/O.** Falcon wins when application code blocks on external I/O — an outbound HTTP fetch, a database round-trip, a slow upstream — because the Fiber yields and the reactor schedules another request. The `/hello`, `/sum`, `/shout`, `/echo` scripts here are pure computation, so there is nothing for the scheduler to overlap.

Either property alone would already cap the win; both together flatten it. A design that binds I/O-doing Services (kobako Services *do* release the GVL while a host callback runs) onto a Sandbox, and runs many requests that wait on those callbacks, would let Falcon overlap the wait windows across requests — and that is where the two servers' designs diverge. The demo deliberately stays in the pure-compute corner so the wire path is easy to read, which makes the corner the wrong place to choose Falcon over Puma.
