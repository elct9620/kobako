# Resource Limits

Each Invocation runs under caps fixed when the Sandbox is built. A cap reached
stops that run alone, and the Sandbox serves the next one unharmed.

```ruby
sandbox = Kobako::Sandbox.new(
  timeout: 5.0,                   # seconds
  setup_timeout: 2.0,             # seconds
  memory_limit: 10 * 1024 * 1024, # bytes
  stdout_limit: 64 * 1024,        # bytes
  stderr_limit: 64 * 1024,        # bytes
  open_file_limit: 16             # descriptors
)
```

Pass `nil` to any cap to switch it off. [Security Model](security.md) says
which caps stay uncapped either way.

## Cap Defaults

Each cap has a default and its own way to end a run.

| Cap | Default | Reached |
|---|---|---|
| `timeout` | 60.0 s | `Kobako::TimeoutError` |
| `setup_timeout` | 10.0 s | `Kobako::SetupError` at `new` |
| `memory_limit` | 4 MiB | `Kobako::MemoryLimitError` |
| `stdout_limit`, `stderr_limit` | 1 MiB each | output clipped, no raise |
| `open_file_limit` | 16 | `Errno::EMFILE` in the guest |
| `open_connection_limit` | 6 | `Errno::EAGAIN` in the guest |
| `connection_limit` | 50 | `Errno::EAGAIN` in the guest |

## Cap Scope

Each cap measures one span, so a reused Sandbox never accumulates against a
global budget.

| Cap | Measures |
|---|---|
| `timeout` | wall time inside the guest, Service calls included |
| `setup_timeout` | instantiating a Guest Binary and reading its ABI version, not compiling it |
| `memory_limit` | memory, table, and GC heap growth past what the Guest Binary declares |
| `open_file_limit` | each descriptor, socket, lookup, HTTP request, and header list one run holds |

`setup_timeout` applies the first time a path is loaded, and later Sandboxes on
that path skip the check.

## Host Overhead

What the host holds for a run sits outside `memory_limit`, as a virtual
machine's runtime sits outside its RAM. Each kind is a count the run already has
times a fixed size. A Host App adds them up for one run's worst case.

| Held for the run | Count | Each at most |
|---|---|---|
| a socket's queues | `open_connection_limit` | 64 KiB each way |
| an HTTP request body | `open_connection_limit` | 64 KiB |
| an HTTP response body | `open_connection_limit` | 64 KiB and one 128 KiB read |
| an HTTP response's head | `open_connection_limit` | 128 KiB |
| a header list, an unsent request | `open_file_limit` | 128 KiB |
| a TLS session | `open_connection_limit` | rustls's bounded buffers |
| a listing | `open_file_limit` | the directory's names |
| a name's virtual addresses | the run | 131070 addresses |
| one host call's copy of guest data | one at a time | `memory_limit`, or 128 MiB with none |

A nested run from a Service is a run of its own, counted apart. The host
kernel's socket buffers grow as the host's `tcp_rmem` and `tcp_wmem` allow, up
to about 10 MiB per connection on Linux.

## Host Descriptors

Every run and the Host App share one process's descriptors, so the default of
16 keeps one run from taking them all. The cap counts descriptors, as Linux's
`RLIMIT_NOFILE` does, so the host holds more than it counts in two places.

| Held by the run | Counts | The host holds |
|---|---|---|
| the standard three and the root | 1 each | — |
| a file, a socket, an HTTP request, a lookup | 1 each | — |
| an Overlay directory | 1 | a directory for each Bind Layer holding it |
| a connection closed with bytes unsent | 0 | its socket and its place in `open_connection_limit`, until the peer takes them |

Sixteen leaves twelve past the standard three and the root, so six files stay
open with every connection open.

### Process Descriptors

Besides each run's own, the process's Binds keep up to 64 host directories
between them for walks. Those give way whenever the host runs out, and a Bind
the Host App let go of holds its root until Ruby collects it.

| Choose | Because |
|---|---|
| `open_file_limit` above what the host leaves | each run's descriptors stay its own |
| a higher process limit, as `ulimit -n` | collected Binds and runs fit beside the Host App |

## Network Caps

Three limits bound the guest's connections and its name lookups. A
connection past any of them raises `Errno::EAGAIN` in the guest, a lookup
`Socket::ResolutionError` for `EAI_AGAIN`, and the script goes on.

| Limit | Measures |
|---|---|
| `open_connection_limit` | each dialed socket, request, and lookup one run holds open at once |
| `connection_limit` | each lookup, dial, and request one run makes, failed ones included |
| the process's | every Sandbox's open connections together, none by default |

`connection_limit` counts what leaves the host, as a Worker counts its
subrequests. A request spends one for its lookup and first address, and one more
for each further address. An attempt a policy or the endpoints refuse spends
nothing.

| Use | Spends | Fits in 50 |
|---|---|---|
| a request to a name | 1 | 50 requests |
| a request whose kth address answers | k | 50 / k requests |
| a socket to a new name | 2, its lookup and dial | 25 sockets |
| a socket to a name already looked up | 1 | 49 sockets after the first |

What every Sandbox holds together is the Host App's to size, through how many
runs it lets in at once times `open_connection_limit`. Only a Rust Host App sets
the process's limit. Lowering it closes nothing already open, and the next
connection is refused.

```rust
kobako::network::process_connections().set_limit(Some(2048));
```

## Usage Report

`Execution#usage` reports what a run spent, measured as its caps measure it. A
rescued trap reports it just as a completed run does.

```ruby
usage = sandbox.eval(script).usage
usage.wall_time   # => Float seconds inside the guest
usage.memory_peak # => Integer bytes grown, never past memory_limit
```

## Isolation Profile

`profile:` requests the isolation profile, and the default `:hermetic` is the
strictest. Construction raises `Kobako::SetupError` on a runtime weaker than
the request.

```
:permissive   <   :hermetic (default)
live clock        frozen clock
live entropy      constant entropy
```

Neither profile gives the guest an ambient filesystem, environment, or network.
What each profile reveals is in [Security Model](security.md).

## Rust SDK

`Options` holds the same caps as `Option` fields of its `limits`, and
`Options::default()` carries the Ruby defaults. `None` switches a cap off.
`Options` is non-exhaustive, so start from the default and set each field.

```rust
let mut options = Options::default();
options.limits.timeout = Some(Duration::from_secs(5));
options.limits.memory_limit = Some(10 * 1024 * 1024);
options.limits.open_file_limit = Some(16);
options.profile = Profile::Hermetic;
let sandbox = Sandbox::new("kobako.wasm", options)?;
```

| Ruby | Rust |
|---|---|
| `TimeoutError`, `MemoryLimitError` | `Error::Timeout`, `Error::MemoryLimit` |
| `SetupError` | `Error::Setup` |
| `usage.wall_time`, `usage.memory_peak` | `execution.usage().wall_time`, `.memory_peak` |
