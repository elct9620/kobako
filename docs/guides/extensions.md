# Extensions

An **Extension** teaches guest code a native-style constant. It bundles a guest idiom
(mruby `source`) with an optional host `backend`. Pure operations such as `File.join`
stay in-guest; privileged ones such as `File.read` dispatch to the host backend like
any bound Service.

| Part | Lives in |
|---|---|
| behavior, setup refusals included | [`spec/behavior/extension.md`](../spec/behavior/extension.md) |
| the contract in use, a worked example | this document |
| a concrete Extension | the Host App or a third-party gem |

kobako ships no concrete Extension, only the contract and the `#install` consumer.
The `File` used below is illustrative.

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
| `provider:` | a no-argument callable's return value | writable state, fresh each invocation |
| neither | `Kobako::Unresolved` | a per-eval `ctx.bind` to fill |

The kind is a keyword, never inferred from whether the value is callable, so a
callable static object is simply `object:`. Per-invocation freshness is a correctness
tool: a writable backend uses `provider:` so its state cannot leak across
invocations. Pass one `provider:` to several Extensions to share one resource
across their paths.

## Composition

`install` decomposes each Extension into the two existing verbs. It adds no wire,
codec, or Guest Binary surface.

```text
install(ext) ──► preload(code: ext.source, name: ext.name)
             └─► ext.backend.path joins the Service registry   (backend only)
```

Install every Extension before the first invocation, which seals installation along
with `#bind` and `#preload` and checks each `depends_on`. The seal and the dependency
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

## Native File Example

The guest idiom `extend`s `Kobako::Proxy` for host-forwarding. The capability is
mixed in rather than inherited, so `File` keeps its own superclass free. It runs path
arithmetic locally and routes I/O to the host.

```ruby
FILE_SOURCE = <<~RUBY
  class File
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
invocation so writes never leak.

```ruby
file_ext = Kobako::Extension.new(
  name: :File,
  source: FILE_SOURCE,
  backend: Kobako::Extension::Backend.new(
    path: "File",
    provider: -> { InMemoryFileSystem.new },  # provider: → fresh each invocation
  ),
)

sandbox.install(file_ext)

sandbox.eval(<<~RUBY)
  File.write("a.txt", "hello")
  File.join("dir", File.basename("x/a.txt"))  #=> "dir/a.txt"  (local, no round-trip)
RUBY
```

A read-only, shared backend supplies a static object with `object:` instead.

```ruby
backend: Kobako::Extension::Backend.new(path: "File", object: read_only_store)
```

## Rust SDK

The `crates/kobako` host SDK reifies the same contract idiomatically. Behavior is
parity-pinned to the Ruby frontend; only the API shape differs.

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

Provider identity maps to `Arc::ptr_eq`, mirroring the Ruby object-identity rule.
