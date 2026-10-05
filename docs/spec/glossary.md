# Glossary

The words this project keeps, and the ones it turns down in their place.

## Everywhere

### Includes

- `README.md`
- `docs/**/*.md`
- `lib/**/*.rb`
- `ext/**/*.rs`
- `crates/**/*.rs`
- `wasm/**/*.rs`
- `examples/**/*.rb`
- `examples/**/*.md`

### Host App

The application that embeds kobako. It holds every credential and policy decision, and chooses which of its own objects the guest may reach.

### Host Gem

kobako itself — the side of the boundary that owns the guest, routes what it asks for, and decides what each outcome means.

### Guest Binary

The compiled artifact untrusted code runs inside. It is the isolation boundary: nothing crosses it except as a message.

### Guest

The contract a Guest Binary implements toward the host. mruby is one implementation of it, and any other that keeps it is indistinguishable to the host.

### Frontend

An API a Host App drives the guest through. The Ruby gem and the Rust SDK are two Frontends held to the same behavior, each keeping its own language's idioms.

### Sandbox

The unit a Host App configures once and invokes many times. It owns a Catalog and keeps nothing from one Invocation to the next.

### Pool

A bounded set of identically set-up Sandboxes, each handed to one holder at a time.

### Snippet

Guest source or bytecode preloaded on a Sandbox and replayed at the start of every Invocation.

### Service

A host object the guest reaches by name. It is the only route from guest code to a host resource.

#### Rejected

- `adapter` - Names a translation role. What distinguishes a Service is that the guest can name it at all.

### Handle

An opaque reference the guest holds to a host object the Codec cannot carry by value. It names that object only within the Invocation that issued it, and is called a Capability Handle where the capability it grants is the point.

### Receiver

The host object a Call resolves to — a Service by its path, or the object a Handle names.

### Proxy

The guest-side behavior that turns a method call on a bound constant or a Handle into a Call.

### Exposure

The methods a host object lets the guest call through a reference to it, whether the guest names it or holds it as a Handle. The object may declare it itself; otherwise it is derived from what the object's own class and the object itself define, fixed when the reference is made. It only narrows: nothing it permits reopens what the boundary refuses.

#### Rejected

- `allow-list` - Names the list rather than what it governs: the subset an object permits is its Exposure, not a second concept beside it.

### Extension

A guest idiom paired with an optional Backend, installed as one unit so guest code sees a native-style constant.

### Backend

The host side of an Extension: the object bound at its path, either fixed for the Sandbox, supplied afresh each Invocation, or left for the Host App to fill.

### Wire Spec

The contract every message crossing the boundary answers to. Each side implements it independently, so it is an agreement rather than a shared component.

### Transport

The exchange of messages across the boundary. One Call is answered by one Reply, and both directions use that same pair.

### Call

A message asking the other side to run one method. The guest issues one to reach a Receiver; the host issues one only to re-enter a Block.

### Reply

The answer to one Call: a value, or a Fault. The answer to a Yield may instead be a break.

### Block

Guest code passed alongside a Call. It stays in the guest, and only its presence crosses the boundary.

### Yield

One synchronous round-trip from a Service method into the Block it received.

### Yielder

The host-side stand-in for a Block, valid only while the dispatch that received the Block lasts.

### Envelope

The part of a message that says where it goes and how it turned out. It is readable without a Codec, so routing and attribution never depend on one.

### Codec

The agreement two endpoints reach about how values become bytes. It is replaceable: only the two endpoints need to share one.

### Outcome

The final result of one Invocation. Every Invocation writes exactly one, whether it succeeded or failed.

### Panic

The failed arm of an Outcome. It attributes the failure to a side of the boundary and carries the Error Record describing it.

### Trap

A failed Invocation the engine is answerable for: a cap was reached, or no Outcome could be framed.

### Sandbox failure

A failed Invocation the guest's own code or the wire is answerable for.

### Service failure

A failed Invocation a Service is answerable for.

### Fault

The reason a Call is refused, returned to whoever issued it. It is kobako's own data rather than the caller's, so it rides the Envelope.

### Error Record

A failure's name, message, and backtrace as one unit. Every channel that reports a guest failure carries this same shape.

### Entrypoint

The constant a Snippet defines for a Run to call.

### Run

An Invocation that names an entrypoint already loaded in the guest, instead of supplying source to execute.

### Frame

Setup data handed to the guest before an Invocation begins, so the guest starts knowing what this run was configured with.

### Catalog

What a Sandbox has registered — the bindings and preloads fixed at setup, together with the Handle table each Invocation mints for itself.

### Invocation

One run of guest code, from entry until it settles. It is the act; the Execution is the record it leaves.

### Execution

The record one Invocation leaves — what it produced, what it wrote, and what it consumed. Frozen once the Invocation settles, and never revised.
