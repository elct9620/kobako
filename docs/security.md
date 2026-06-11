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
host-side allowlist:** guest code can name any `<Namespace>::<Member>` path, but a forged
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
| The guest cannot fabricate a `Kobako::Handle` or a Member proxy, nor dereference a Handle to a value. | B-20, B-38, B-39 |
| Each invocation gets a fresh `mrb_state`; Handles, stdout / stderr, and memory delta reset between calls. Monkeypatching and globals do not persist. | B-03, B-18, B-19 |
| Services and state on different Sandbox instances are fully isolated. | B-09 |
| Per-invocation `timeout`, linear-memory cap, and stdout / stderr clipping, all with clean errors. | B-01, B-35 |
| Only the type allowlist serializes; an unrepresentable, over-deep, cyclic, or NUL-bearing value becomes a controlled `Kobako::SandboxError`, never a host crash. | B-06, E-06, [`wire-codec.md`](wire-codec.md) § Structural Nesting Depth |

## Designing a Service

A Service is the one place untrusted code touches your application, so designing one is a
security exercise. Each binding is a capability you hand out; the concerns below are the
questions to ask before you do.

### Least privilege — scope the Sandbox to one trust context

A Sandbox's bindings *are* its capability set, so one Sandbox shared across contexts turns
every binding into ambient authority for all of them. Build one per principal — per user,
agent session, or submission — bind only what that context may touch, and finish all
`define` / `bind` / `preload` before the first dispatch, where the registry seals (B-33).

```ruby
def sandbox_for(session)
  Kobako::Sandbox.new.tap do |s|
    s.define(:KV).bind(:Store, ScopedStore.new(session.id))  # only this session's keys
  end
end
```

### Least privilege — expose the smallest method surface

`bind` exposes *every* public method of the object, not the one you had in mind; since
private / protected are already unreachable, the only lever is the public surface itself.
Bind a purpose-built object rather than a capable one whose other methods leak more than
you intend.

```ruby
sandbox.define(:Cfg).bind(:Settings, AppConfig)        # reachable: secret_key, database_url, writers, ...

class ThemeReader
  def color = AppConfig.theme.color
end
sandbox.define(:Cfg).bind(:Settings, ThemeReader.new)  # reachable: only #color
```

> **Gotcha:** a Service method named after Ruby's reflection / eval surface (`send`, `eval`,
> `binding`, `instance_eval`, `method`, …) is rejected rather than dispatched — the guest
> proxy raises and the host refuses it (B-42, B-44) — so it is never reachable. Rename it,
> and never reuse member / method names across trust layers.

### Untrusted input — validate at the boundary

Every argument arrives from untrusted code that may pass `2.5` where you expect an
Integer, a negative count, or a value large enough to exhaust memory. Reject bad type,
range, and encoding (CR/LF, NUL) at the method entry rather than coercing silently — a
quiet coercion is a host-side defect the sandbox cannot catch for you.

```ruby
sandbox.define(:Text).bind(:Repeat, ->(str, n) {
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

sandbox.define(:Net).bind(:Get, ->(url) {
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
keep calling into.

```ruby
sandbox.define(:Search).bind(:Docs, ->(q) { index.query(q).map(&:title) })  # => ["...", "..."]
#                                            index.query(q)                 # => a Handle whose every method dispatches back
```

### Availability — bound work volume under abuse

Caps limit the *rate* of dispatch, not its total *volume*: tens of thousands of Handles
can mint inside one invocation, living in host memory — outside the guest's Wasm cap —
until it resets (B-19). For hostile input, bound the amount of work and the number of
Handles a single invocation can create.

```ruby
calls = 0
sandbox.define(:Cur).bind(:Next, -> {
  raise "budget exhausted" if (calls += 1) > 1_000
  cursor.advance  # a fresh Handle each call
})
```
