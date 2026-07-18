# Security Model & Host Hardening

kobako isolates untrusted guest code; it does not decide what that code is allowed to
reach. The first job is the gem's, the second is yours — this document draws the line.

## Shared responsibility

The guest runs in a Wasm cell with no access to host memory, files, sockets, or `ENV`,
and its only path outward is a Service you injected. Ambient wall-clock time and host
entropy are denied at the WASI layer too — the guest's `wasi:clocks` is frozen at the
Unix epoch and `wasi:random` is a constant stream — so the no-ambient-authority guarantee
holds even if a future guest gem reaches for libc time or randomness, not only because the
mrbgem allowlist omits those gems. **The real authorization gate is your
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

| Guarantee | Anchor |
|-----------|--------|
| Only a bound object's own Service methods are reachable; Ruby's ambient reflection / eval surface — the `send` family, `eval` / `instance_eval`, `binding`, `method`, `define_method`, `instance_variable_get`, and the `Proc` / `Method` / `Binding` gadget methods — is rejected host-side, leaving for a bound lambda only the callable allowlist (`call` / `[]` / `yield` / `arity` / `lambda?`). Reflective objects (`Binding` / `Method` / `UnboundMethod`) never cross as Handles. | B-12, B-42, B-43 |
| The guest cannot fabricate a `Kobako::Handle` or a bound-constant proxy, nor dereference a Handle to a value. | B-20, B-38, B-39 |
| Each invocation starts from the canonical boot state; Handles, stdout / stderr, and memory delta reset between calls. Monkeypatching and globals do not persist. | B-03, B-18, B-19, B-49 |
| Services and state on different Sandbox instances are fully isolated. | B-09 |
| Under the default `hermetic` profile, guest code observes no ambient wall-clock time or host entropy; `wasi:clocks` is frozen and `wasi:random` is constant, so the guest is deterministic but for values a Service injects. | B-45 |
| `Sandbox.new(profile:)` requests the isolation posture on the `permissive < hermetic` ladder — `hermetic` by default; the runtime builds it, declares what it built, and construction fails cleanly when the declaration falls below the request. | B-54 |
| Per-invocation `timeout`, linear-memory cap, and stdout / stderr clipping, all with clean errors. | B-01, B-35 |
| Only the type allowlist serializes; an unrepresentable, over-deep, cyclic, or NUL-bearing value becomes a controlled `Kobako::SandboxError`, never a host crash. | B-06, E-06, E-54, [`wire-codec.md`](wire-codec.md) § Structural Nesting Depth |

## Isolation profiles

Isolation postures form the ordered ladder `permissive < hermetic` (B-54). `hermetic` is
the full ambient-denial posture this document describes: B-45's frozen clocks and constant
entropy, no filesystem, `ENV`, or network reachability, and no host import beyond the wire
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
`bind` / `preload` before the first dispatch, where the registry seals (B-33).

```ruby
def sandbox_for(session)
  Kobako::Sandbox.new.tap do |s|
    s.bind("KV::Store", ScopedStore.new(session.id))  # only this session's keys
  end
end
```

### Least privilege — expose the smallest method surface

`bind` exposes *every* public method of the object, not the one you had in mind; since
private / protected are already unreachable, the only lever is the public surface itself.
Bind a purpose-built object rather than a capable one whose other methods leak more than
you intend.

```ruby
sandbox.bind("Cfg::Settings", AppConfig)        # reachable: secret_key, database_url, writers, ...

class ThemeReader
  def color = AppConfig.theme.color
end
sandbox.bind("Cfg::Settings", ThemeReader.new)  # reachable: only #color
```

> **Gotcha:** a Service method named after Ruby's reflection / eval surface (`send`, `eval`,
> `binding`, `instance_eval`, `method`, …) is rejected rather than dispatched — the guest
> proxy raises and the host refuses it (B-42, B-44) — so it is never reachable. Rename it,
> and never reuse member / method names across trust layers.

### Least privilege — let a crossing object gate its own surface

A purpose-built wrapper is one lever for the smallest surface; a second is to let the object
decide for itself. A bound object — a Service, or anything that crosses back as a
`Kobako::Handle` — may define a private `respond_to_guest?(name)` that answers, per method
name, whether the guest may call it. Return `false` for every name and the object is
**opaque**: the guest holds it and forwards it to another Service, but can call nothing on
it — the bearer-token shape a credential or Vault handle wants, without hand-building a
wrapper that exposes nothing. Return `true` for a chosen subset and it is an allow-list. The
predicate composes beneath the reflection floor and can only narrow, so even a buggy
predicate can never re-open `send` / `eval`; keep it private so the guest cannot probe it
(B-50).

```ruby
class ApiCredential
  def headers = { authorization: "Bearer #{token}" }   # host-side callers only

  private

  def token = Vault.fetch(:api_key)
  def respond_to_guest?(_name) = false                 # opaque: carried, never read
end

# A Service issues the credential; being non-wire it crosses as an opaque Handle (B-14).
sandbox.bind("Secret::Issue", -> { ApiCredential.new })

# guest:  cred = Secret::Issue.call            # a Handle it holds but cannot read
#         WebFetch::Get.call(url, cred: cred)  # forwards it to another Service (B-16)
# host:   WebFetch receives the real ApiCredential and calls #headers;
#         any cred.<method> the guest attempts raises instead
```

> An opaque object's calls are rejected with the same `undefined` fault as a name that
> resolves to nothing (B-50), so the guest learns nothing about which methods it defines. To
> expose a safe subset instead, answer `true` only for those names:
> `def respond_to_guest?(name) = name == :public_id`.

> **Gotcha — a permissive backend has no vocabulary ceiling until you draw one.** The floor
> lets an otherwise-unknown method name through when the bound object answers `respond_to?`
> truthy for it — the escape hatch dynamic `method_missing` Services rely on. An object whose
> `respond_to?` answers *everything* (a builder, a proxy, anything routing through
> `method_missing`) therefore takes every name the floor does not independently reject
> straight to its `method_missing`: the reflection / eval surface stays blocked, but the
> object's own dynamic surface is wide open. Bind such an object behind a `respond_to_guest?`
> that names the methods the guest may call, or wrap it so `respond_to?` answers honestly for
> the methods it actually defines.

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

A non-wire-representable return crosses as a `Kobako::Handle`, which makes the object's
*entire* public surface reachable and mints a fresh Handle at each hop with no identity
dedup (B-15). Return the data the guest needs as a terminal value, not a host object it can
keep calling into. When the guest must hold the object itself — a capability it forwards to
another Service rather than reads — give it a `respond_to_guest?` that seals or narrows that
surface (above) instead of leaving every method reachable.

```ruby
sandbox.bind("Search::Docs", ->(q) { index.query(q).map(&:title) })  # => ["...", "..."]
#                                            index.query(q)                 # => a Handle whose every method dispatches back
```

The same applies to failures: an exception a Service raises crosses to the guest as
`<class>: <message>` fault text (B-12), so keep secrets and internal detail out of
raisable messages — rescue internal errors and re-raise a clean, guest-safe one.

### Availability — bound work volume under abuse

Caps limit the *rate* of dispatch, not its total *volume*: tens of thousands of Handles
can mint inside one invocation, living in host memory — outside the guest's Wasm cap —
until it resets (B-19). For hostile input, bound the amount of work and the number of
Handles a single invocation can create.

```ruby
calls = 0
sandbox.bind("Cur::Next", -> {
  raise "budget exhausted" if (calls += 1) > 1_000
  cursor.advance  # a fresh Handle each call
})
```
