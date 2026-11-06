# Kobako

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/elct9620/kobako)

Kobako is a Ruby gem that runs untrusted mruby scripts in an in-process Wasm
sandbox: LLM-generated code, user formulas, student submissions, plugins. A
script reaches no host memory, file, network, or credential you did not hand it.
A Rust SDK offers the same sandbox to hosts in other languages.

```
      Host process                    Wasm guest
+---------------------+         +---------------------+
| Kobako::Sandbox     |--eval-->| mruby interpreter   |
| Services            |<--call--| KV::Lookup.call(k)  |
| Execution           |<-result-| last expression     |
+---------------------+         +---------------------+
        trusted                       untrusted
```

## Requirements

The gem ships its Guest Binary and, on most platforms, a prebuilt native
extension.

| What | Which versions | When you need it |
|---|---|---|
| Ruby | >= 3.3.0 | always |
| Platform | Linux, macOS, or Windows | always; Windows gaps are fixed last |
| Rust / Cargo | current stable | only where no prebuilt gem matches your platform |
| WASI toolchain | vendored by the build | only when building from a source checkout |

## Installation

Add the gem to your bundle.

```bash
bundle add kobako
```

## Quick Start

A Sandbox runs guest code and returns the record of that run.

```ruby
require "kobako"

sandbox = Kobako::Sandbox.new
sandbox.eval("1 + 2").value # => 3
```

```rust
use kobako::{Options, Sandbox};

let sandbox = Sandbox::new("kobako.wasm", Options::default())?;
sandbox.eval("1 + 2")?.value()?; // => Value::Int(3)
```

These terms name what the rest of this README builds on.

| Term | Meaning |
|---|---|
| Sandbox | the unit you configure once and invoke many times |
| Invocation | one run of guest code, by `#eval` or `#run` |
| Execution | the frozen record an Invocation leaves |
| Service | a host object the guest reaches by name |
| Mount | a Filesystem placed at a guest path |

## Features

Each feature below shows its smallest use; the [guides] hold the rest.

[guides]: https://github.com/elct9620/kobako/tree/main/docs/guides

### Services

`bind` makes a host object reachable from the guest at a constant name.

```ruby
sandbox.bind("KV::Lookup", ->(key) { redis.get(key) })
sandbox.eval('KV::Lookup.call("user_42")').value
```

A Service can also be filled per Invocation, take a guest block, and hand back
host objects as opaque Handles.

### Mounts

`mount` gives the guest's `File` and `Dir` a Filesystem at one path, read-only by default.

```ruby
sandbox.mount("/config", Kobako::Filesystem::Tmpfs.new("app.yml" => yaml))
sandbox.eval('File.read("/config/app.yml")').value
```

| Kind | Holds |
|---|---|
| `Tmpfs` | files in memory |
| `Bind` | one host directory |
| `Overlay` | read-only Layers under a Tmpfs |

### Execution Records

An Execution carries the value, the output, and the usage. A failed run raises
one of three classes, each carrying its Execution.

```ruby
execution = sandbox.eval('puts "hi"; 42')
execution.stdout # => "hi\n"

begin
  sandbox.eval(script)
rescue Kobako::TrapError, Kobako::ServiceError, Kobako::SandboxError => e
  e.execution&.stderr
end
```

### Resource Limits

Each Invocation runs under caps, and reaching one ends that run alone.

```ruby
Kobako::Sandbox.new(timeout: 5.0, memory_limit: 10 * 1024 * 1024)
```

| Cap | Default |
|---|---|
| `timeout` | 60 s |
| `memory_limit` | 4 MiB |
| `stdout_limit`, `stderr_limit` | 1 MiB |
| `open_file_limit` | 1024 |

### Concurrency

A Sandbox keeps nothing between runs, so Threads may share one. `gvl:
:release` runs guest code in parallel, and `Kobako::Pool` keeps warm Sandboxes.

```ruby
pool = Kobako::Pool.new(slots: 4) { |sandbox| sandbox.bind("KV::Lookup", lookup) }
pool.with { |sandbox| sandbox.eval(script).value }
```

### Snippets

`preload` replays guest code into every Invocation, and `run` calls what it
defined. An Extension pairs such code with a host backend.

```ruby
sandbox.preload(code: "Adder = ->(a, b) { a + b }", name: :Adder)
sandbox.run(:Adder, 2, 3).value # => 5
```

### Guest Binaries

The gem bundles the pure `kobako.wasm`. Each [GitHub Release] adds variants
with Regexp or JSON, and you can build your own in Rust.

```ruby
sandbox = Kobako::Sandbox.new(wasm_path: "kobako+full.wasm")
sandbox.eval('JSON.generate({ n: 42 })').value # => "{\"n\":42}"
```

[GitHub Release]: https://github.com/elct9620/kobako/releases

## Security

kobako isolates the guest, but it reaches whatever you `bind` or `mount`. Bind
an object built for the task, since every public method its class defines is
exposed.

```ruby
class ThemeReader # only #color is reachable
  def color = AppConfig.theme.color
end

sandbox.bind("Cfg::Settings", ThemeReader.new) # not AppConfig itself
```

## Performance

Figures are order-of-magnitude on macOS arm64 with Ruby 3.4.7; ratios hold
across machines. The [benchmark] suite gates every release on a +10% regression.

| Phase | Cost |
|---|---|
| first `Sandbox.new` per machine (JIT, then disk-cached) | ~500 ms |
| later `Sandbox.new` | ~3 µs |
| warm `#eval` with one Service call | ~62 µs |

[benchmark]: https://github.com/elct9620/kobako/tree/main/benchmark

## Development

The release gate compiles everything and runs every test and lint.

```bash
bin/setup                  # install dependencies
bundle exec rake           # the release gate
bin/console                # IRB with the gem loaded
bundle exec rake install   # install the local checkout
```

The first compile builds mruby and the Guest Binary through the [beni] gem.
`AGENTS.md` maps the rake tasks and the repository layout.

[beni]: https://github.com/elct9620/beni

## Contributing

Bug reports and pull requests are welcome at the [repository]. Open an issue
before a non-trivial change, so we can agree on its scope.

[repository]: https://github.com/elct9620/kobako

## License

Kobako is released under the [Apache License 2.0](https://opensource.org/licenses/Apache-2.0).
