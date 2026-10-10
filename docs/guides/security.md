# Security Model

kobako isolates untrusted guest code; it does not decide what that code may reach.
The first job is the gem's, the second is yours, and this guide draws the line.
The behavior itself is specified in [`spec/behavior/`](../spec/behavior/).

```
   kobako owns                          you own
   ───────────                          ───────
   the isolation boundary               which Services cross it
   resource caps                        what each Service may do
   wire / return-value guardrails       input validation on Service args
   per-invocation + cross-Sandbox       one Sandbox per trust context
     isolation
```

The guest's only path outward is a Service you injected. Guest code can name any
`MyService::KV` path, but a forged name resolves only to something you bound. **What
you bind, and each object's Exposure, is the real authorization gate.**

## Built-in Guarantees

These hold without host effort, so do not re-implement them. Each row names the
feature that specifies it.

| Guarantee | Specified in |
|---|---|
| a bound object exposes only what it defines itself | [transport-boundary](../spec/behavior/transport-boundary.md) |
| reflection and eval never cross, whatever an object permits | [transport-boundary](../spec/behavior/transport-boundary.md) |
| the guest cannot forge or dereference a Handle | [transport-boundary](../spec/behavior/transport-boundary.md) |
| each invocation starts from the same boot state | [sandbox](../spec/behavior/sandbox.md) |
| a binding belongs to its own Sandbox | [services](../spec/behavior/services.md) |
| no ambient filesystem, environment, or socket | [runtime](../spec/behavior/runtime.md) |
| no live time or entropy under the default `:hermetic` | [runtime](../spec/behavior/runtime.md) |
| timeout, memory cap, and output clipping fail cleanly | [sandbox](../spec/behavior/sandbox.md) |
| a value the wire cannot carry becomes a Handle or a controlled error | [transport-dispatch](../spec/behavior/transport-dispatch.md) |

## Isolation Profiles

Postures form the ladder `permissive < hermetic`; `Sandbox.new(profile:)` requests
one and defaults to `:hermetic`. The rungs differ in one thing only.

| Profile | Clocks and entropy | Everything else |
|---|---|---|
| `hermetic` | frozen clock, constant entropy | no filesystem, `ENV`, or socket |
| `permissive` | live host time and entropy | same as `hermetic` |

`hermetic` keeps the guest deterministic except for values a Service injects.
Requesting `:permissive` trades that reproducibility for real time and entropy;
every other guarantee holds on both rungs.

The request is also a floor. Keep the default when the engine is swappable. An
engine that cannot deny ambient authority is then refused at construction, not
silently weakening these guarantees. The ladder is specified in
[runtime](../spec/behavior/runtime.md).

## Service Design

A Service is the one place untrusted code touches your application, so each
binding is a capability you hand out. Ask these questions before you bind one.

| Concern | Lever |
|---|---|
| Trust Context | one Sandbox per principal |
| Method Surface | bind a purpose-built object |
| Self-gating Objects | a private `respond_to_guest?` |
| Input Validation | reject at method entry |
| External Effects | allowlist the resolved resource |
| Return Surface | return terminal values |
| Work Volume | budget each invocation |

### Trust Context

A Sandbox's bindings *are* its capability set, so one Sandbox shared across contexts
turns every binding into ambient authority for all of them. Build one per principal
— per user, agent session, or submission — and bind only what that context may
touch. Finish every `bind` and `preload` before the first invocation, which seals
the registry.

```ruby
def sandbox_for(session)
  Kobako::Sandbox.new.tap do |s|
    s.bind("KV::Store", ScopedStore.new(session.id))  # only this session's keys
  end
end
```

### Method Surface

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

#### Surface Gotchas

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
The predicate replaces the default rather than trimming it, so answer narrowly.
The reflection floor still holds beneath it; keep it private all the same. The rules
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

### Return Surface

A return the wire cannot carry crosses as a `Kobako::Handle`, and its own methods
become reachable. Return the data the guest needs as a terminal value, not a host
object it can keep calling into. When the guest must hold the object itself, give it
a `respond_to_guest?` that seals or narrows that surface.

```ruby
sandbox.bind("Search::Docs", ->(q) { index.query(q).map(&:title) })  # => ["...", "..."]
sandbox.bind("Search::Hit",  ->(q) { index.query(q).first })         # => a Handle whose own methods dispatch back
```

A returned collection holding host objects crosses as one Handle too, and the
guest cannot index into it. Map it to terminal values on the host side.

The same applies to failures. An exception a Service raises reaches the guest as
`<class>: <message>` fault text. Rescue internal errors and re-raise a clean,
guest-safe one, so secrets and internal detail stay out of the message.

### Work Volume

The timeout bounds an invocation's time, Service time included, but not the host
memory and work each call costs. A Handle costs the guest only its reference, while
its object stays in host memory until the invocation ends
([transport-dispatch](../spec/behavior/transport-dispatch.md)). For hostile input,
bound the calls and Handles a single invocation can create.

```ruby
sandbox.bind("Cur::Next")  # filled per invocation

def budgeted(cursor, limit: 1_000)
  calls = 0
  -> {
    raise "budget exhausted" if (calls += 1) > limit
    cursor.advance  # a fresh Handle each call
  }
end

sandbox.eval(script) { |ctx| ctx.bind("Cur::Next", budgeted(cursor)) }
```

A fresh budget per `ctx.bind` resets the count for every invocation. A counter
captured once at `bind` would span the Sandbox's lifetime instead.
