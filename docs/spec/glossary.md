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

kobako itself — the side of the boundary that owns the guest. It routes what the guest asks for and decides what each outcome means.

### Guest Binary

The compiled artifact untrusted code runs inside. It is the isolation boundary: nothing crosses it except as a message.

### Guest

The contract a Guest Binary implements toward the host. mruby is one implementation of it, and any other that keeps it is indistinguishable to the host.

### Frontend

An API a Host App drives the guest through. The Ruby gem and the Rust SDK are two Frontends held to the same behavior. Each keeps its own language's idioms.

### Sandbox

The unit a Host App configures once and invokes many times. It owns a Catalog and keeps nothing from one Invocation to the next.

### Pool

A bounded set of identically set-up Sandboxes, each handed to one holder at a time.

### Ambient

Authority the guest would hold without the Host App handing it over. The guest holds none of it unless a Posture grants it.

```
handed over by the Host App  -> Services, Handles, Grants
Ambient                      -> host time, entropy, files, environment, sockets
```

### Posture

The isolation a runtime builds around the guest, one rung on an ordered ladder. The Host App requests a rung as a floor, and the runtime declares the rung it built. Rungs differ only in which Ambient authority they grant.

```
permissive  <  hermetic
   requested rung  <=  declared rung   -> accepted
```

### Grant

A host resource the Host App hands to the runtime instead of binding it by name. The guest reaches it through its system interface, such as a file it opens. The Sandbox holds it, and each Invocation receives it afresh.

```
Sandbox: Grant ──each Invocation──> runtime ──system interface──> guest
```

Each kind of Grant carries what it hands over and decides its own access. The runtime asks the Grant before the guest's call proceeds.

```
Grant ─┬─ Mount   -> a Filesystem at a guest path
       └─ ...     -> another kind, its own access
```

### Mount

The Grant placing a Filesystem at a guest path. It allows reading, or reading and writing, and a Policy may decide in that access's place. The Mount decides, so the Filesystem behind it need not.

```
Mount -> at a guest path, read or read-write, or a Policy -> a Filesystem
```

### Policy

The Host App's decision on each path a guest opens or changes beneath a Mount. It decides in the Mount's access's place. It judges the path the guest names, so a directory beneath it does not move. In Ruby it is the block a mount is given.

```
open or change at a path ─> Policy ─┬─ allows -> the call proceeds
                                    └─ refuses -> denied, the script goes on
```

### Filesystem

The files and directories behind a Mount. It answers every call the guest makes beneath that Mount's path. kobako ships four, and a Host App may bring its own or wrap another.

```
Directory  -> a host directory; nothing outside it is reachable
Memory     -> held in memory; lives as long as the Host App's object
Temporary  -> held in memory; every Invocation starts it empty
Overlay    -> Layers stacked into one view; changes land in the writable ones
```

### Layer

One Filesystem stacked in an Overlay, read-only or writable on its own. An upper Layer hides the same path in a lower one, and a whiteout hides a removed one.

```
Overlay -> Layer (writable) -> Layer (read-only) -> ...
```

### Snippet

Guest source or bytecode preloaded on a Sandbox and replayed at the start of every Invocation.

### Service

A host object the guest reaches by name. Guest code reaches a host resource only through a Service or a Grant.

```
guest code ──by name─────────────> Service
           ──system interface────> Grant
```

#### Rejected

- `adapter` - Names a translation role. What distinguishes a Service is that the guest can name it at all.

### Handle

An opaque reference the guest holds to a host object the Codec cannot carry by value. It names that object only within the Invocation that issued it. Where the capability it grants is the point, it is called a Capability Handle.

### Receiver

The host object a Call resolves to — a Service by its path, or the object a Handle names.

### Proxy

The guest-side behavior that turns a method call on a bound constant or a Handle into a Call.

### Exposure

The methods a host object lets the guest call through a reference to it, by name or as a Handle. The object may declare its Exposure itself. Otherwise the Exposure derives from what the object and its own class define, fixed when the reference is made. It only narrows: nothing it permits reopens what the boundary refuses.

#### Rejected

- `allow-list` - Names the list rather than what it governs. The subset an object permits is its Exposure, not a second concept.

### Extension

A guest idiom paired with an optional Backend, installed as one unit so guest code sees a native-style constant.

### Backend

The host side of an Extension: the object bound at its path. It is fixed for the Sandbox, supplied afresh each Invocation, or left for the Host App to fill.

### Wire Spec

The contract every message crossing the boundary answers to. Each side implements it independently, so it is an agreement rather than a shared component.

### Transport

The exchange of messages across the boundary. One Call is answered by one Reply, and both directions use that same pair.

### Call

A message asking the other side to run one method. The guest issues one to reach a Receiver; the host issues one only to re-enter a Block.

### Reply

The answer to one Call: a value, a Fault, or Unsupported. The answer to a Yield may instead be a break, or the Error Record of a Block that raised.

```
Call  -> value | Fault | Unsupported
Yield -> value | break | Error Record | Unsupported
```

### Block

Guest code passed alongside a Call. It stays in the guest, and only its presence crosses the boundary.

### Yield

One synchronous round-trip from the host into a Block whose Call is still open.

### Yielder

The host-side stand-in for a Block, valid only while the dispatch that received the Block lasts.

### Envelope

The part of a message that says where it goes and how it turned out. It is readable without a Codec, so routing and attribution never depend on one.

### Tag

The first byte of every message, naming its operation or its answer's arm. One registry covers both directions, so an assigned tag means one thing everywhere.

### Request

A message asking the receiver to run the operation its tag names. A request under a tag the registry has not assigned is unknown rather than broken.

### Downcall

A request from the host to the guest. It starts an Invocation or a Yield, or asks the guest to compile its Snippets.

### Upcall

A request from the guest to the host. It carries a Call.

### Codec

The agreement two endpoints reach about how values become bytes. It is replaceable: only the two endpoints need to share one.

### Value Budget

The most values one guest payload may hold, worked out from the memory cap. Decoding spends it, so a short payload cannot make the host build a large value.

### Outcome

The final result of one Invocation. Every Invocation answers exactly one, whether it succeeded or failed.

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

### Unsupported

The answer that the receiver does not serve the operation it was sent, returned to whoever sent it. It is an answer, not a breakage, so it is never a Trap.

### Error Record

A failure's name, message, and backtrace as one unit. Every answer that reports a guest failure carries this same shape.

### Entrypoint

The constant a Snippet defines for a Run to call.

### Run

An Invocation that names an entrypoint already loaded in the guest, instead of supplying source to execute.

### Preamble

What the guest reads before an Invocation's entry runs or its preloads compile: the Catalog's bindings, preloads, and Flags. The guest starts knowing what this run was configured with.

```
Preamble = bindings + preloads + Flags
   read before -> an Invocation's entry
               -> a compile of the preloads
```

### Flag

A named switch the host sets for an Invocation's interpreter. A Guest Binary that does not recognise it skips it, unless the host marked it critical.

### Catalog

What a Sandbox has registered: the bindings, preloads, and Flags fixed at setup. It also holds the Handle table each Invocation mints for itself.

### Invocation

One run of guest code, from entry until it settles. It is the act; the Execution is the record it leaves.

### Execution

The record one Invocation leaves — what it produced, what it wrote, and what it consumed. Frozen once the Invocation settles, and never revised.

### Channel

One of the two streams guest code writes to: output and error. Each is captured apart, so what a script printed never mixes with what it warned.
