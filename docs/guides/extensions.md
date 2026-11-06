# Extensions

An **Extension** teaches guest code a native-style constant. It bundles a guest idiom
(mruby `source`) with an optional host `backend`. Pure operations such as `Storage.join`
stay in-guest; privileged ones such as `Storage.read` dispatch to the host backend like
any bound Service.

| Part | Lives in |
|---|---|
| behavior, setup refusals included | [`spec/behavior/extension.md`](../spec/behavior/extension.md) |
| the contract in use, a worked example | this document |
| a concrete Extension | the Host App or a third-party gem |

kobako ships no concrete Extension, only the contract and the `#install` consumer.
The [DSL example](../../examples/dsl/) is a worked builder idiom whose every dialect lives on the host.
The `Storage` used below is illustrative; a Guest Binary's own `File` is reached
through a Mount instead.

## Extension Contract

An Extension is any object exposing four readers. `Kobako::Extension` is the bundled
value type; a conforming object of your own is equally valid.

| Reader | Type | Meaning |
|---|---|---|
| `name` | Symbol or String matching `/\A[A-Z]\w{0,65533}\z/` | snippet name and `depends_on` key |
| `source` | String, mandatory | the mruby idiom, preloaded as a snippet |
| `backend` | `Kobako::Extension::Backend` or `nil` | the host attachment; `nil` stays pure-guest |
| `depends_on` | Array of Symbol or String | Extensions that must also be installed |

The idiom's locally defined methods run in the guest, and the rest fall through to
the backend. The `name` is the Extension's identity, independent of any bound path.

## Backend Kinds

A backend pairs a `path`, the guest constant the idiom routes to, with how the object
behind it is found. `Kobako::Extension::Backend` is the bundled type; `install`
duck-types on `path`, `object`, and `provider`.

| Declaration | Bound object | Choose it for |
|---|---|---|
| `object:` | the object itself | a shared read-only backend |
| `provider:` | a no-argument callable's return value | writable state, fresh each Invocation |
| neither | `Kobako::Unresolved` | a per-Invocation `ctx.bind` to fill |

The kind is a keyword, never inferred from whether the value is callable, so a
callable static object is simply `object:`. Per-Invocation freshness is a correctness
tool: a writable backend uses `provider:` so its state cannot leak across
Invocations. Pass one `provider:` to several Extensions to share one resource
across their paths.

## Composition

An installed Extension is a Snippet and, when it has a backend, a Service, both
held in the Sandbox's Catalog. It adds no wire, Codec, or Guest Binary surface.

```text
Extension            Catalog
  name, source  --->   Snippet
  backend       --->   Service at path
```

Install every Extension before the first Invocation, which seals `#bind`,
`#preload`, `#install`, `#mount`, and `#flag` and checks each `depends_on`. The seal and the dependency
check are specified in [extension](../spec/behavior/extension.md).

## Design Boundaries

Two rules keep an Extension one idiom over at most one host object.

| Rule | Otherwise |
|---|---|
| `source` is mandatory | bind a host object with `#bind` directly |
| at most one backend per Extension | link several Extensions by `depends_on` |

The first rule is the `install` / `bind` divide. A capability spanning several
host-backed constants composes as one idiom and one backend each, joined by
reference rather than a single aggregate.

## Storage Example

The guest idiom `extend`s `Kobako::Proxy` for host-forwarding. The capability is
mixed in rather than inherited, so `Storage` keeps its own superclass free. It runs path
arithmetic locally and routes I/O to the host.

```ruby
STORAGE_SOURCE = <<~RUBY
  class Storage
    extend Kobako::Proxy

    def self.join(*parts) = parts.join("/")
    def self.basename(p)  = p.split("/").last || ""
    # read / write are not defined locally, so they dispatch to the host

    def self.open(path, mode = "r")
      buf = Buffer.new(read(path))   # one host round-trip, then all-local
      return buf unless block_given?
      begin  yield buf ensure buf.close end
    end

    class Buffer
      def initialize(content) = (@s = content; @pos = 0)
      def read       = @s
      def each_line(&blk) = @s.each_line(&blk)
      def close      = nil
    end
  end
RUBY
```

The host backend is an ordinary duck-typed object, here a fresh in-memory store per
Invocation so writes never leak.

```ruby
storage_ext = Kobako::Extension.new(
  name: :Storage,
  source: STORAGE_SOURCE,
  backend: Kobako::Extension::Backend.new(
    path: "Storage",
    provider: -> { InMemoryStore.new },  # provider: → fresh each invocation
  ),
)

sandbox.install(storage_ext)

sandbox.eval(<<~RUBY)
  Storage.write("a.txt", "hello")
  Storage.join("dir", Storage.basename("x/a.txt"))  #=> "dir/a.txt"  (local, no round-trip)
RUBY
```

A read-only, shared backend supplies a static object with `object:` instead.

```ruby
backend: Kobako::Extension::Backend.new(path: "Storage", object: read_only_store)
```

## Rust SDK

The Rust SDK, `crates/kobako`, reifies the same contract idiomatically. Behavior is
parity-pinned to the Ruby Frontend; only the API shape differs.

```rust
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn source(&self) -> &str;
    fn depends_on(&self) -> &[&str] { &[] }
    fn backend(&self) -> Option<Backend> { None }
}

pub struct Backend { pub path: String, pub provider: Provider }

pub enum Provider {
    Static(Arc<dyn Receiver>),                                       // fixed
    PerInvocation(Arc<dyn Fn() -> Arc<dyn Receiver> + Send + Sync>), // fresh each invocation
    Fillable,                                                        // unresolved until ctx.bind fills it
}

// Sandbox::install(&mut self, extension: Arc<dyn Extension>) -> Result<(), Error>
```

As in Ruby, one provider is one resource: Extensions handed the same `Arc` share one
object per Invocation, and distinct providers yield distinct objects.
