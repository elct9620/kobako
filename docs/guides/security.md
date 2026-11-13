# Security Model

kobako isolates untrusted guest code; it does not decide what that code may reach.
The first job is the gem's, the second is yours, and this guide draws the line.
The behavior itself is specified in [`spec/behavior/`](../spec/behavior/).
Mounts have their own guide in [mounts](mounts.md).

```
+-- kobako owns --------+     +-- you own ------------+
| isolation boundary    |     | Services, Mounts (a)  |
| resource caps         |     | Service behavior      |
| wire guardrails (b)   |     | input validation      |
| Sandbox isolation (c) |     | trust contexts (d)    |
|                       |     | bytecode origin       |
+-----------------------+     +-----------------------+
```

- (a) Which Services and Mounts cross the boundary.
- (b) The wire and return-value guardrails.
- (c) Isolation per Invocation and across Sandboxes.
- (d) One Sandbox per trust context.

The guest reaches outward only through a Service you bound, a Filesystem you
mounted, or a network entry you gave it. Guest code can name any `MyService::KV`
path, but a forged name resolves only to something you bound. **What you bind,
mount, and give as network, and each object's Exposure, is the real authorization
gate.**

## Built-in Guarantees

These hold without host effort, so do not re-implement them. Each row names the
feature that specifies it.

| Guarantee | Specified in |
|---|---|
| a bound object exposes only what it defines itself | [transport-boundary](../spec/behavior/transport-boundary.md) |
| reflection, eval, and methods mixed into every object never cross, whatever an object permits | [transport-boundary](../spec/behavior/transport-boundary.md) |
| the guest cannot forge or dereference a Handle | [transport-boundary](../spec/behavior/transport-boundary.md) |
| each Invocation starts from the same boot state | [sandbox](../spec/behavior/sandbox.md) |
| a binding belongs to its own Sandbox | [services](../spec/behavior/services.md) |
| no ambient filesystem, environment, or socket | [runtime](../spec/behavior/runtime.md) |
| a path the guest gives stays within its Mount, and the Mount's access holds | [runtime](../spec/behavior/runtime.md) |
| a socket reaches only what an endpoint admits | [network](../spec/behavior/network.md) |
| a name is looked up only when an endpoint names it | [network](../spec/behavior/network.md) |
| a credential leaves only over HTTPS, to the host it binds | [network](../spec/behavior/network.md) |
| no live time or entropy under the default `:hermetic` | [runtime](../spec/behavior/runtime.md) |
| timeout, memory cap, and output clipping fail cleanly | [sandbox](../spec/behavior/sandbox.md) |
| the guest holds no more files open than its cap | [sandbox](../spec/behavior/sandbox.md) |
| a Tmpfs holds no more than its size | [filesystem](../spec/behavior/filesystem.md) |
| a value the wire cannot carry becomes a Handle or a controlled error | [transport-dispatch](../spec/behavior/transport-dispatch.md) |

## Profiles

Profiles are ordered `permissive < hermetic`; `Sandbox.new(profile:)` requests
one and defaults to `:hermetic`. The profiles differ in what the guest reads of time, entropy, and a Mount's entries.

| Profile | Clocks and entropy | A Mount's entries | Everything else |
|---|---|---|---|
| `hermetic` | frozen clock, constant entropy | every time at the epoch, inode number 0 | no ambient filesystem, `ENV`, or socket |
| `permissive` | live host time and entropy | real times, an inode number per entry | same as `hermetic` |

A permissive guest's inode number tells entries apart, and nothing outside the process can compute it from a host's numbers.

`hermetic` keeps the guest deterministic except for values a Service injects or a
network entry brings.
Requesting `:permissive` trades that reproducibility for real time and entropy;
every other guarantee holds on both profiles. Live time and entropy reach only a
third-party guest. The bundled Guest Binaries read no clock, using one only to
bound a wait, and give scripts no randomness.

The request is also a floor. Keep the default when the engine is swappable. An
engine that cannot deny ambient authority is then refused at construction, not
silently weakening these guarantees. The order is specified in
[runtime](../spec/behavior/runtime.md).

## Uncapped Limits

Passing `nil` turns a cap off, and with it the bound that cap kept. Each one then
leaves the host exposed in its own way.

| `nil` cap | What the guest can then cost the host |
|---|---|
| `timeout` | a thread held for as long as the guest runs |
| `timeout`, with a network | a socket a silent peer holds open as long as the run |
| `setup_timeout` | `Sandbox.new` held for as long as the Guest Binary takes to start |
| `memory_limit` | up to 4 GiB of memory, no value budget, every listing a guest holds |
| `stdout_limit`, `stderr_limit` | host memory without bound, read back after the run ends |
| `open_file_limit` | host descriptors for each file or directory open beneath a Bind |
| `open_file_limit`, with a network | sockets bounded only by `open_connection_limit` |
| `open_connection_limit` | host sockets bounded only by the process's limit |
| `connection_limit` | a run dialing again and again, each closed connection making room |
| a Tmpfs's `size:` | host memory without bound; one write far past a file's end fills the gap |
| a Tmpfs's `nr_inodes:` | an entry for every file and directory it makes |

An uncapped output is the sharpest: a guest writing one buffer in a loop grows the
capture on the host, not in its own memory. Reading that capture back also runs
after the timeout fires, so the call returns late. Keep both output caps set for
untrusted code.

## Network

kobako keeps a socket to what an endpoint admits. What the endpoint itself does
with what the guest sends is yours to judge, as a Service's behavior is.

```
guest name --> endpoint names it? --> host resolver --> address in its sets?
                 | no: NXDOMAIN, unasked                  | no: EACCES
                                                          +--> connect, past the policies
```

### Names and Lookups

A name an endpoint names goes to the host's resolver, so each lookup is egress.

| Endpoint | What the guest can still do |
|---|---|
| a wildcard, `*.example.com` | spell data into labels the zone's server reads, even when the connect is refused |
| an exact name | look up that one name |
| a name the host resolves to a private address, as split-horizon DNS does | reach it only if the endpoint lists `:private` or a CIDR holding it |

A wildcard leads only to `:public` addresses or a CIDR you write, so a name under it never reaches your own network.

An endpoint on port 53 or 853 lets the guest ask a DNS server directly, past every endpoint. The addresses it learns there still meet the endpoints when it connects. The server still sees every name the guest asks, so name only a server you trust.

### Socket Options

A guest tunes its own sockets through the options WASI defines, and each stays within what kobako already grants.

| Option | What the guest can still do |
|---|---|
| keepalive probes | keep an admitted connection open while idle, within the run's deadline |
| `SO_RCVBUF`, `SO_SNDBUF` | shrink a socket's queues, never past the memory each socket reserved |
| `IP_TTL` | count the hops to a destination an endpoint admits, by sending with a short limit |

A read answers the socket's own setting, so the host's defaults never reach the guest.

### Echoing Endpoints

A credential is swapped in only on the way out, and a response passes untouched.
An endpoint that echoes a request back, in a header or a body, hands the secret to
the guest. Bind a credential only to an endpoint that never echoes it.

| Endpoint | Bind a credential to it |
|---|---|
| an API that only authenticates the header | yes |
| a debug, echo, or proxy endpoint | no |

### Timing Reference

A network entry hands the guest a clock. A script sends a request the endpoint
answers after a known delay, calls a Service, then asks with a zero timeout
whether the answer arrived. That tells it whether the Service ran longer than
the delay.

| Service time difference | What one run tells apart |
|---|---|
| about 10 ms | each call, with ten connections |
| about 1.5 ms | nothing until many samples are averaged |

kobako adds no jitter: noise only raises the samples an attacker averages, as
Cloudflare Workers and the Lucky 13 authors found. The counts are your levers
instead. `connection_limit` bounds the samples one run takes, so lower it for a
script that needs few requests. You decide how many runs a script gets. Keep a
Service's run time independent of any secret it does not return, or give a
script that holds a network entry few runs.

### Trust Roots

TLS trusts the OS's own store, so a revoked or added root follows the host's
patches, not a kobako release.

| Platform | Store | Revocation |
|---|---|---|
| macOS | Security.framework | online, end certificate |
| Windows | the system certificate store | online, end certificate |
| Linux | the distribution's CA bundle, or `SSL_CERT_FILE` | none |

The store is read once per process, when the first network entry is made. Revocation
fetches on macOS and Windows are host egress no endpoint judges. Setting
`SSL_CERT_FILE` changes what every guest trusts, and an inspection root installed
on the host sees each credential swapped in.

### Image Requirements

A container image without a CA bundle refuses every TLS connection.

| Image | Do |
|---|---|
| Debian, Ubuntu | install `ca-certificates` |
| Alpine | install `ca-certificates` |
| `scratch` | copy a CA bundle in, or set `SSL_CERT_FILE` |

## Preloaded Bytecode

`preload(binary:)` runs its blob as guest code without compiling it. mruby checks
the blob's structure, not its instructions, so trust a blob as you trust host
code. A blob outside that trust stays inside the Wasm sandbox. It can corrupt only
the guest's own memory, which the host already treats as hostile.

| Blob | Pass it to `preload(binary:)` |
|---|---|
| unmodified compiler output of the Guest Binary's mruby release | yes |
| anything else, such as an upload or a hand-edited dump | no; preload the source instead |

## Service Design

A Service is the one place untrusted code touches your application, so each
binding is a capability you hand out. Ask these questions before you bind one.

| Concern | Lever |
|---|---|
| Trust Context | one Sandbox per principal |
| Exposed Methods | bind a purpose-built object |
| Self-gating Objects | a private `respond_to_guest?` |
| Input Validation | reject at method entry |
| External Effects | allowlist the resolved resource |
| Returned Values | return terminal values |
| Work Volume | budget each Invocation |

### Trust Context

A Sandbox's bindings *are* its capability set, so one Sandbox shared across contexts
turns every binding into ambient authority for all of them. Build one per principal
— per user, agent session, or submission — and bind only what that context may
touch. Finish every `bind`, `install`, `mount`, `preload`, and `flag` before the
first Invocation; the Sandbox refuses each of them from then on.

```ruby
def sandbox_for(session)
  Kobako::Sandbox.new.tap do |s|
    s.bind("KV::Store", ScopedStore.new(session.id))  # only this session's keys
  end
end
```

### Exposed Methods

`bind` exposes every public method the object's own class and the object itself
define, not just the one you had in mind. What it inherits or mixes in stays out, as
[transport-boundary](../spec/behavior/transport-boundary.md) specifies. Bind a
purpose-built object rather than a capable one whose other methods leak more than
you intend.

```ruby
# Either binding, not both: one path takes one object.
sandbox.bind("Cfg::Settings", AppConfig.current)  # reachable: secret_key, database_url, writers, ...

class ThemeReader
  def color = AppConfig.current.theme.color
end
sandbox.bind("Cfg::Settings", ThemeReader.new)    # reachable: only #color
```

#### Exposure Gotchas

Two cases surprise hosts that follow the rule above. kobako cannot tell your
classes from a gem's or the standard library's, and reflective names never cross.

| Case | What the guest meets | Do instead |
|---|---|---|
| a class you did not write, such as `Pathname` | its Ruby-source methods, `#rmtree` and `#mkpath` among them | bind your own class, or narrow with `respond_to_guest?` |
| a Service method named `send`, `eval`, `binding`, `method`, … | a proxy refusal, or for `send` the guest's own `Kernel#send` | rename it; never reuse names across trust layers |

### Self-gating Objects

A bound object, or anything crossing back as a `Kobako::Handle`, may define a private
`respond_to_guest?(name)` that decides which names the guest may call. Answer
`false` for every name and the object is **opaque**: the guest carries it to another
Service but reads nothing. That is the bearer-token shape a credential wants.

Answer `true` for a subset, such as `name == :public_id`, to expose exactly those.
The predicate chooses among the methods written in Ruby and the names the object
answers dynamically. A method built into Ruby stays refused, however it was reached.
So does one a library mixes into every object, such as ActiveSupport's `try` or
`to_json`; define a Service's own `to_json` to expose one. Keep the predicate
private all the same. The rules
live in [transport-boundary](../spec/behavior/transport-boundary.md).

```ruby
class ApiCredential
  def headers = { authorization: "Bearer #{token}" }   # host-side callers only

  private

  def token = Vault.fetch(:api_key)
  def respond_to_guest?(_name) = false                 # opaque: carried, never read
end

# A Service issues the credential; being non-wire it crosses as an opaque Handle.
sandbox.bind("Secret::Issue", -> { ApiCredential.new })

# guest:  cred = Secret::Issue.call            # a Handle it holds but cannot read
#         WebFetch::Get.call(url, cred: cred)  # forwards it to another Service
# host:   WebFetch receives the real ApiCredential and calls #headers
```

### Dynamic Backends

A name a `method_missing` backend answers dynamically is not one its class defines,
so the default leaves it out. Answer those names in `respond_to_missing?`, name
the callable ones in a private `respond_to_guest?`, and keep that list narrow. A predicate answering `true` for
everything sends every non-reflection name to `method_missing`.

```ruby
class StoreBackend
  def initialize(store) = @store = store
  def method_missing(name, *args) = @store.public_send(name, *args)
  def respond_to_missing?(name, include_private = false) = @store.respond_to?(name) || super

  private

  def respond_to_guest?(name) = %i[get put].include?(name)  # the whole vocabulary
end
```

### Input Validation

Every argument arrives from untrusted code. It may pass `2.5` where you expect an
Integer, a negative count, or a value large enough to exhaust memory. Reject bad
type, range, and encoding (CR/LF, NUL) at the method entry. A quiet coercion is a
host-side defect the sandbox cannot catch.

```ruby
sandbox.bind("Text::Repeat", ->(str, n) {
  raise ArgumentError, "n must be 1..100" unless n.is_a?(Integer) && (1..100).cover?(n)
  str.to_str * n
})
```

### External Effects

An allowlisted name can resolve to an internal address at use time (DNS rebinding).
A Service that reaches the network, disk, or another system should allowlist what it
permits rather than denylist what it forbids. Verify the *resolved resource*, not the
name the guest handed you, and re-check on every redirect hop.

```ruby
ALLOWED = { "api.example.com" => 443 }.freeze

sandbox.bind("Net::Get", ->(url) {
  uri = URI(url)
  ip  = Resolv.getaddress(uri.host)                       # resolve first
  raise "host not allowed" unless ALLOWED[uri.host] == uri.port && public_ip?(ip)  # then verify the IP
  Net::HTTP.get(uri)
})
```

### Returned Values

A return the wire cannot carry crosses as a `Kobako::Handle`, and its own methods
become reachable. Return the data the guest needs as a terminal value, not a host
object it can keep calling into. When the guest must hold the object itself, give it
a `respond_to_guest?` that seals or narrows its Exposure.

```ruby
sandbox.bind("Search::Docs", ->(q) { index.query(q).map(&:title) })  # => ["...", "..."]
sandbox.bind("Search::Hit",  ->(q) { index.query(q).first })         # => a Handle whose own methods dispatch back
```

A returned collection holding host objects crosses as one Handle too, and the
guest cannot index into it. Map it to terminal values on the host side.

The same applies to failures. An exception a Service raises reaches the guest as
`<class>: <message>` fault text, and an `ArgumentError` as its message alone. Rescue
internal errors and re-raise a clean, guest-safe one, so secrets and internal detail
stay out of the message.

### Work Volume

The timeout bounds an Invocation's time, Service time included, but not the host
memory and work each call costs. A Handle costs the guest only its reference, while
its object stays in host memory until the Invocation ends
([transport-dispatch](../spec/behavior/transport-dispatch.md)). For hostile input,
bound the calls and Handles a single Invocation can create.

Each payload the guest sends holds at most one value per 16 bytes of `memory_limit`
([payload-msgpack](../wire/payload-msgpack.md)). Raising the memory cap raises what one
call may make the host build.

```ruby
sandbox.bind("Cur::Next")  # filled per Invocation

def budgeted(cursor, limit: 1_000)
  calls = 0
  -> {
    raise "budget exhausted" if (calls += 1) > limit
    cursor.advance  # a fresh Handle each call
  }
end

sandbox.eval(script) { |ctx| ctx.bind("Cur::Next", budgeted(cursor)) }
```

A fresh budget per `ctx.bind` resets the count for every Invocation. A counter
captured once at `bind` would span the Sandbox's lifetime instead.

## Rust SDK

The Rust SDK keeps the same boundary. A `Receiver` exposes no method by
reflection, so its surface is exactly what its `call` answers.

| Ruby | Rust |
|---|---|
| a purpose-built object | a `Receiver` matching only the methods it serves |
| a private `respond_to_guest?` | `Receiver::respond_to_guest` |
| a Service raising a clean error | `Err(Fault::new(kind, message))` |
| `profile:` | `Options::profile` |
| a block on `mount` | `mount_with_policy` |

A `Fault` message reaches the guest as written, so keep secrets out of it.
