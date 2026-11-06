# Services

A Service is a host object the guest reaches by a constant name. It is the
guest's only route to host code, so what you bind is what the guest may call.

```
   Host App                        guest script
+-------------+   bind    +---------------------------+
| any object  |---------->| MyService::KV (proxy)     |
+-------------+           +---------------------------+
       ^                     |
       |   call over wire    |
       +---------------------+
```

Which methods a bound object exposes, and how to narrow them, is in
[Security Model](security.md).

## Binding Paths

`bind` places an object at a constant path, and the guest calls its public
methods through a proxy of that name.

```ruby
class User
  attr_reader :name

  def initialize(name:) = @name = name
end

sandbox.bind("Project::User", User.new(name: "alice"))
sandbox.bind("KV::Lookup", ->(key) { redis.get(key) })

sandbox.eval(<<~RUBY)
  Project::User.name         # => "alice"
  KV::Lookup.call("user_42") # => "..."
RUBY
```

Symbol keyword arguments reach the host method's keywords unchanged.

| A path that | Raises |
|---|---|
| has a segment not matching `/\A[A-Z]\w{0,65533}\z/` | `ArgumentError` |
| sits under `Kobako`, kept by the Guest Binary | `ArgumentError` |
| arrives after the first invocation | `ArgumentError` |

## Per-Invocation Bindings

Some objects belong to one run: the current request, the acting user, a
per-tenant store. `bind(path)` without an object declares a fillable, and the
`#eval` or `#run` block fills it through the Context.

```ruby
sandbox.bind("Req::Current")  # declared, unfilled

sandbox.eval("Req::Current.user_id") { |ctx| ctx.bind("Req::Current", request) }
```

| Binding | Lives for | Unfilled call |
|---|---|---|
| `bind(path, object)` | the Sandbox | — |
| `bind(path)` | nothing until filled | `Kobako::ServiceError` |
| `ctx.bind(path, object)` | one invocation, shadowing either | — |

The Context is spent once the block returns, so a `ctx` kept past it raises
`ArgumentError`. The [multi-tenant example](../../examples/multi-tenant/) runs
this shape end to end.

## Service Blocks

A Service method may take a guest block with `&blk` and `yield` into it. The
block runs inside the guest, and its `break`, `next`, `return`, and exceptions
land where they would through a plain method.

```ruby
sandbox.bind("Seq::Map", ->(items, &blk) { items.map(&blk) })

sandbox.eval("Seq::Map.call([1, 2, 3]) { |x| x * 2 }").value # => [2, 4, 6]
```

A `yield` that cannot complete raises at the Service's call site, so the
Service may rescue it. Left unrescued, a `BlockError` hands the guest its own
exception back, and the others fail the Service call.

| Class | Raised at `yield` when |
|---|---|
| `Kobako::BlockError` | the guest block raised |
| `Kobako::YieldValueError` | an argument cannot cross to the guest |
| `Kobako::UnsupportedError` | the Guest Binary serves no block |
| `ThreadError` | the block runs off the calling Fiber |

### Block Fiber

A block runs only on the Fiber that called the Service. The guest's Wasm stack
lives on that Fiber's native stack, so kobako narrows what CRuby allows.

```
calling Fiber
  +-- blk.call / yield                 --> guest runs
  +-- Thread.new { blk.call }          --> ThreadError
  +-- Enumerator.new { blk.call }.next --> ThreadError
```

`Enumerator#next` counts as another Fiber, because it runs the body on one.
Internal iteration such as `each` or `map` stays on the calling Fiber.

## Handles

A host object the wire cannot carry crosses as an opaque `Kobako::Handle`. It
is restored to the original object before host code sees it again.

```ruby
class Greeter
  def initialize(name) = @name = name
  def greet = "hi, #{@name}"
end

sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

sandbox.eval('Factory::Make.call("Bob").greet').value # => "hi, Bob"
sandbox.eval('Factory::Make.call("Bob")').value       # => #<Greeter @name="Bob">
```

| Where it crosses | Becomes |
|---|---|
| a Service's return, or a `#run` argument | a Handle |
| a value the guest returns | the original object |
| any other value the wire cannot carry | `Kobako::SandboxError` |

A Handle names its object only within the invocation that issued it. A `break`
value from a guest block stays in the guest, so a Handle in it stays a Handle.

### Per-dispatch Handles

Each dispatch that returns an object allocates a new Handle, never deduplicated
by identity. A fluent chain such as an `ActiveRecord::Relation` binds one per hop.

```
guest chain                       host (one invocation)
User.where(active: true) --call--> Relation 1 --> Handle 1
    .order(:created_at)  --call--> Relation 2 --> Handle 2
    .limit(10)           --call--> Relation 3 --> Handle 3
```

| Consequence | Why |
|---|---|
| one object returned twice is two Handles | the guest cannot tell they alias |
| every Handle stays live to the invocation's end | there is no per-Handle release |
| IDs run to 2^31 - 1, reset per invocation | deep chains stay far inside the range |

## Rust SDK

The Rust SDK binds an `Arc<dyn Receiver>`. A `msgpack::ValueReceiver` speaks the
bundled payload shape, and `into_receiver` presents it at that seam.

| Ruby | Rust |
|---|---|
| `bind(path, object)` | `bind(path, object.into_receiver())` |
| `bind(path)` | `bind_fillable(path)` |
| `eval(code) { \|ctx\| ctx.bind(...) }` | `eval_with(code, \|ctx\| ctx.bind(...))` |
| `yield` / `blk.call` | `Yielder::call_values` on the `block` argument |
| a returned object becoming a Handle | `Handles::alloc`, answered as `Value::Handle(id)` |
| a private `respond_to_guest?` | `Receiver::respond_to_guest` |

```rust
impl ValueReceiver for Lookup {
    fn call(&self, method: &str, args: &[Value], _kwargs: &[(String, Value)],
            _block: Option<&mut Yielder<'_>>, _handles: &Handles<'_>) -> Result<Value, Fault> {
        match method {
            "call" => Ok(self.get(&args[0])),
            _ => Err(Fault::new(FaultKind::Undefined, method)),
        }
    }
}

sandbox.bind("KV::Lookup", Lookup::new().into_receiver())?;
```
