# Security Model & Host Hardening

kobako isolates untrusted guest code; it does not decide what that code is allowed to
reach. The first job is the gem's, the second is yours — this document draws the line.

## Shared responsibility

The guest runs in a Wasm cell with no access to host memory, files, sockets, or `ENV`,
and its only path outward is a Service you injected. Ambient wall-clock time and host
entropy are denied at the WASI layer — `wasi:clocks` is frozen at the Unix epoch and
`wasi:random` is a constant stream — so the guarantee holds at the boundary, not just
because the mrbgem allowlist omits the time / random gems. **The real authorization gate is your
host-side allowlist:** guest code can name any `MyService::KV` path, but a forged
name only ever resolves to something you bound.

```
   kobako owns                          you own
   ───────────                          ───────
   the isolation boundary               which Services cross it
   resource caps                        what each Service may do
   wire / return-value guardrails       input validation on Service args
   per-invocation + cross-Sandbox       one Sandbox per trust context
     isolation
```

## What kobako guarantees

These hold without any host effort — do not re-implement them.

| Guarantee | Scenario |
|-----------|----------|
| A bound object or Handle exposes only what its own class and the object itself define — nothing inherited, mixed in, forwarded, or built into the platform — unless it defines its own `respond_to_guest?`, which then decides. Ruby's ambient reflection / eval surface is rejected host-side regardless, and reflective objects never cross as Handles — a bound lambda keeps only the callable allowlist (`call` / `[]` / `yield` / `arity` / `lambda?`). What counts as reflection is [`transport-boundary.md`](spec/behavior/transport-boundary.md)'s to define. | [`T-208`](spec/behavior/transport-boundary.md), [`T-117`](spec/behavior/transport-boundary.md), [`T-122`](spec/behavior/transport-boundary.md) |
| The guest cannot construct a `Kobako::Handle` ([`T-110`](spec/behavior/transport-boundary.md)); a bound-constant proxy it constructs is capability-inert; neither can be dereferenced to a value — the host's `Catalog::Handles` membership and path resolution gate every dispatch. | [`T-109`](spec/behavior/transport-boundary.md), [`T-042`](spec/behavior/transport-dispatch.md) |
| Each invocation starts from the canonical boot state; Handles, stdout / stderr, and memory delta reset between calls. Monkeypatching and globals do not persist. | [`MR-001`](spec/behavior/mruby.md), [`S-013`](spec/behavior/sandbox.md) |
| Services and state on different Sandbox instances are fully isolated. | [`S-017`](spec/behavior/sandbox.md), [`T-039`](spec/behavior/transport-dispatch.md) |
| Under the default `hermetic` profile, guest code observes no ambient wall-clock time or host entropy; `wasi:clocks` is frozen and `wasi:random` is constant, so the guest is deterministic but for values a Service injects. | [`RT-047`](spec/behavior/runtime.md), [`RT-048`](spec/behavior/runtime.md) |
| `Sandbox.new(profile:)` requests the isolation posture on the `permissive < hermetic` ladder — `hermetic` by default; the runtime builds it, declares what it built, and construction fails cleanly when the declaration falls below the request. | [`RT-009`](spec/behavior/runtime.md), [`RT-012`](spec/behavior/runtime.md) |
| Per-invocation `timeout`, linear-memory cap, and stdout / stderr clipping, all with clean errors. | [`OC-034`](spec/behavior/outcome.md), [`S-024`](spec/behavior/sandbox.md) |
| Only the type allowlist serializes; an unrepresentable, over-deep, cyclic, or NUL-bearing value becomes a controlled error — `Kobako::SandboxError` for a guest value or `#run` argument, a Service failure for a Service's answer or yield arguments — never a host crash. | [`S-135`](spec/behavior/sandbox.md), [`S-073`](spec/behavior/sandbox.md), [`wire/payload-msgpack.md`](wire/payload-msgpack.md) § Structural Nesting Depth |

## Isolation profiles

Isolation postures form the ordered ladder `permissive < hermetic` ([`RT-014`](spec/behavior/runtime.md)). `hermetic` is
the full ambient-denial posture this document describes: frozen clocks and constant
entropy ([`RT-051`](spec/behavior/runtime.md), [`RT-048`](spec/behavior/runtime.md)), no filesystem, `ENV`, or network reachability, and no host import beyond the wire
ABI's single `__kobako_dispatch` — so the guest's only paths outward are the Services you
inject and the stdout / stderr capture. `permissive` relaxes exactly one thing: the guest's
`wasi:clocks` and `wasi:random` read live host time and entropy, giving up reproducible
execution for that Sandbox; filesystem, `ENV`, network, and the host-import set stay as at
`hermetic`.

`Sandbox.new(profile:)` requests the posture, defaulting to `:hermetic`; the runtime builds
the request and declares the posture it actually built, and construction fails with
`Kobako::SetupError` rather than run guest code on a runtime that declares less than you
requested. Requesting `:permissive` is an explicit trade — you accept ambient
nondeterminism in exchange for guest code that reads real time and entropy; every other
guarantee in this document holds on both rungs. The request doubles as a floor when the
runtime is swappable: pin `:hermetic` (or keep the default) and an alternative engine that
cannot deny ambient authority is refused at construction instead of weakening the
guarantees above silently.

## Designing a Service

A Service is the one place untrusted code touches your application, so designing one is a
security exercise. Each binding is a capability you hand out; the concerns below are the
questions to ask before you do.

### Least privilege — scope the Sandbox to one trust context

A Sandbox's bindings *are* its capability set, so one Sandbox shared across contexts turns
every binding into ambient authority for all of them. Build one per principal — per user,
agent session, or submission — bind only what that context may touch, and finish all
`bind` / `preload` before the first dispatch, where the registry seals ([`SV-031`](spec/behavior/services.md)).

```ruby
def sandbox_for(session)
  Kobako::Sandbox.new.tap do |s|
    s.bind("KV::Store", ScopedStore.new(session.id))  # only this session's keys
  end
end
```

### Least privilege — expose the smallest method surface

`bind` exposes the public methods the object's own class and the object itself define — not
the one you had in mind, but every one of them. What it inherits from a superclass, mixes in
(`Comparable`, `Enumerable`, or a concern module of your own), or gets from the platform stays
unreachable, and a class, module, or forwarder bound directly exposes nothing ([`T-208`](spec/behavior/transport-boundary.md), [`T-214`](spec/behavior/transport-boundary.md)).
Bind a purpose-built object rather than a capable one whose other methods leak more than you
intend.

```ruby
sandbox.bind("Cfg::Settings", AppConfig.current)  # reachable: secret_key, database_url, writers, ...

class ThemeReader
  def color = AppConfig.current.theme.color
end
sandbox.bind("Cfg::Settings", ThemeReader.new)    # reachable: only #color
```

> **Gotcha — a class you did not write still exposes what it defines.** The default is drawn
> around whoever wrote the object's class, and kobako cannot tell your classes from a gem's or
> the standard library's: a `Pathname` handed to the guest exposes `#rmtree`, `#mkpath`,
> `#children`, and the rest of what `Pathname` defines in Ruby. Hand over objects of classes you
> wrote, return a terminal value, or narrow the object with `respond_to_guest?` (below).

> **Gotcha:** a Service method named after Ruby's reflection / eval surface (`send`, `eval`,
> `binding`, `instance_eval`, `method`, …) is rejected rather than dispatched — the guest
> proxy raises and the host refuses it ([`T-114`](spec/behavior/transport-boundary.md), [`T-117`](spec/behavior/transport-boundary.md)) — so it is never reachable. Rename it,
> and never reuse member / method names across trust layers.

### Least privilege — let a crossing object gate its own surface

A purpose-built wrapper is one lever for the smallest surface; a second is to let the object
decide for itself. A bound object — a Service, or anything that crosses back as a
`Kobako::Handle` — may define a private `respond_to_guest?(name)` that answers, per method
name, whether the guest may call it. Return `false` for every name and the object is
**opaque**: the guest holds it and forwards it to another Service, but can call nothing on
it — the bearer-token shape a credential or Vault handle wants, without hand-building a
wrapper that exposes nothing. Return `true` for a chosen subset and it exposes exactly those.
The predicate replaces the default rather than trimming it, so a name it permits is reachable
even when the object inherits that method — answer `true` for everything and the whole public
surface is back. It still composes beneath the reflection floor, so even a buggy predicate can
never re-open `send` / `eval`; keep it private so the guest cannot probe it
([`T-130`](spec/behavior/transport-boundary.md)).

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
# host:   WebFetch receives the real ApiCredential and calls #headers;
#         any cred.<method> the guest attempts raises instead
```

> An opaque object's calls are rejected with the same `undefined` fault as a name that
> resolves to nothing ([`T-197`](spec/behavior/transport-boundary.md)), so the guest learns nothing about which methods it defines. To
> expose a safe subset instead, answer `true` only for those names:
> `def respond_to_guest?(name) = name == :public_id`.

> **Gotcha — a `method_missing` backend reaches nothing until it draws its own vocabulary.** A
> name it answers dynamically is not one its class defines, so the default leaves it out
> ([`T-215`](spec/behavior/transport-boundary.md)). Define a private `respond_to_guest?` that names the callable methods — and
> keep it that narrow: a predicate answering `true` for everything takes every
> non-reflection name straight to `method_missing`.

### Untrusted input — validate at the boundary

Every argument arrives from untrusted code that may pass `2.5` where you expect an
Integer, a negative count, or a value large enough to exhaust memory. Reject bad type,
range, and encoding (CR/LF, NUL) at the method entry rather than coercing silently — a
quiet coercion is a host-side defect the sandbox cannot catch for you.

```ruby
sandbox.bind("Text::Repeat", ->(str, n) {
  raise ArgumentError, "n must be 1..100" unless n.is_a?(Integer) && (1..100).cover?(n)
  str.to_str * n
})
```

### Fail-safe defaults — default-deny external effects

An allowlisted name can resolve to an internal address at use time (DNS rebinding), so a
Service that reaches the network, disk, or another system should allowlist what it permits
— not denylist what it forbids — and verify the *resolved resource* rather than the name
the guest handed you, re-checking on every redirect hop.

```ruby
ALLOWED = { "api.example.com" => 443 }.freeze

sandbox.bind("Net::Get", ->(url) {
  uri = URI(url)
  ip  = Resolv.getaddress(uri.host)                       # resolve first
  raise "host not allowed" unless ALLOWED[uri.host] == uri.port && public_ip?(ip)  # then verify the IP
  Net::HTTP.get(uri)
})
```

### Minimal disclosure — control the return surface

A non-wire-representable return crosses as a `Kobako::Handle`, which makes every public
method the object's own class defines reachable and mints a fresh Handle at each hop with no
identity dedup ([`T-007`](spec/behavior/transport-dispatch.md)). Return the data the guest needs as a terminal value, not a host object it
can keep calling into. When the guest must hold the object itself — a capability it forwards
to another Service rather than reads — give it a `respond_to_guest?` that seals or narrows that
surface (above) instead of leaving its own methods reachable.

```ruby
sandbox.bind("Search::Docs", ->(q) { index.query(q).map(&:title) })  # => ["...", "..."]
#                                            index.query(q)                 # => a Handle whose own methods dispatch back
```

The same applies to failures: an exception a Service raises crosses to the guest as
`<class>: <message>` fault text ([`T-140`](spec/behavior/transport-dispatch.md)), so keep secrets and internal detail out of
raisable messages — rescue internal errors and re-raise a clean, guest-safe one.

### Availability — bound work volume under abuse

Caps limit the *rate* of dispatch, not its total *volume*: tens of thousands of Handles
can mint inside one invocation, living in host memory — outside the guest's Wasm cap —
until that invocation ends and releases its table ([`T-038`](spec/behavior/transport-dispatch.md)). For hostile input, bound the amount of work and the number of
Handles a single invocation can create.

```ruby
calls = 0
sandbox.bind("Cur::Next", -> {
  raise "budget exhausted" if (calls += 1) > 1_000
  cursor.advance  # a fresh Handle each call
})
```
