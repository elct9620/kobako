# Extensions — installing a guest idiom with an optional host backend

An **Extension** teaches guest code a native-style constant. It bundles
a guest idiom (mruby `source`) with an optional host `backend`, and
`Sandbox#install` composes the two onto a Sandbox through the existing
`#preload` and `#bind` verbs. Pure operations (`File.join`) run in-guest
with no round-trip; privileged operations (`File.read`) dispatch to the
host backend under the same isolation and reflection guarantees as any
bound Service.

The behavior is governed by B-55..B-57 in
[`behavior/extension.md`](behavior/extension.md) and the setup errors
E-51..E-53 in [`behavior/errors.md`](behavior/errors.md). This document
is the contract-in-use and a worked example; the anchors are the
authority.

kobako ships **no concrete Extension** — only the contract and the
`#install` consumer. A guest idiom such as `File` is authored by the
Host App or a third-party gem. The `File` used below is illustrative.

## The contract

An Extension is any object exposing four readers. `Kobako::Extension`
is the bundled value type; a conforming object of your own is equally
valid.

| Reader | Type | Meaning |
|--------|------|---------|
| `name` | Symbol matching `/\A[A-Z]\w*\z/` | Identity: the preloaded snippet's canonical name (its `(snippet:Name)` backtrace label) and the `depends_on` match key. Independent of any bound path. |
| `source` | String (mandatory) | The mruby idiom, preloaded as a snippet. Its methods that are defined locally run in-guest; the rest fall through to the host backend. |
| `backend` | `Kobako::Extension::Backend` or `nil` | The host attachment — a `path` the backend binds at plus a `provider`. `nil` for a pure-guest Extension. |
| `depends_on` | Array of Symbol | Names of Extensions that must also be installed; presence-checked at the first invocation. |

The `backend` pairs a `path` (the constant path the backend binds at —
single-segment `"File"` or nested `"MyApp::Store"`, spelling the guest
constant the idiom routes to) with a `provider`. `Kobako::Extension::Backend`
is the bundled value type; `install` duck-types on `path` / `provider`,
so a conforming object of your own is equally valid.

| Provider form | Bound object | Lifetime |
|---------------|--------------|----------|
| Not itself callable (fixed) | The object itself | Resolved once; identical across every invocation |
| A callable (`respond_to?(:call)`) | The callable's return value | Invoked once per invocation; a fresh object backs the path each invocation |

Callability is the sole discriminator, so a fixed backend that itself
responds to `:call` (rare for a filesystem/store object) is inexpressible
directly — wrap it in a non-callable holder. The Rust SDK removes this
edge by making the choice explicit through the `Provider` enum.

Provider identity is resource identity: pass the **same** provider to
several Extensions and it resolves once per invocation to one object
shared by their paths; pass distinct providers for distinct objects.

## Composition and lifecycle

`install` decomposes each Extension into the two existing verbs and adds
no wire, codec, or Guest Binary surface:

- `preload(code: source, name: name)` registers the idiom.
- When `backend` is present, `backend.path` enters the sealed Service
  registry (Frame 1); the object behind it is resolved from `provider`
  at each invocation.

The first invocation seals installation (as it seals `#bind` / `#preload`):
a later `install` raises `ArgumentError`. At the seal, every installed
Extension's `depends_on` must be present among the installed names — an
unmet dependency raises `ArgumentError` before the guest runs. The check
is presence-only, so dependency cycles are permitted; cross-Extension
references resolve at guest call time, after every snippet has replayed.

## Boundaries

- **`source` is mandatory** — this is the `install` / `bind` divide. A
  host object with no guest idiom is bound with `#bind` directly.
- **At most one backend per Extension.** A capability spanning several
  host-backed constants composes as several Extensions linked by
  `depends_on` — one idiom + one backend each, joined by reference, not
  a single aggregate.
- **Per-invocation freshness is a correctness tool.** A writable backend
  must use a callable provider so its state cannot leak across
  invocations; a shared read-only backend is a fixed object.

## Worked example — a native `File`

The guest idiom `extend`s `Kobako::Proxy` for host-forwarding — a capability
mixed in rather than inherited, so `File` keeps its own superclass free — runs
path arithmetic locally, and routes I/O to the host:

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

The host backend is an ordinary duck-typed object — here a fresh
in-memory store per invocation, so writes never leak:

```ruby
file_ext = Kobako::Extension.new(
  name: :File,
  source: FILE_SOURCE,
  backend: Kobako::Extension::Backend.new(
    path: "File",
    provider: -> { InMemoryFileSystem.new },  # callable → fresh each invocation
  ),
)

sandbox.install(file_ext)

sandbox.eval(<<~RUBY)
  File.write("a.txt", "hello")
  File.join("dir", File.basename("x/a.txt"))  #=> "dir/a.txt"  (local, no round-trip)
RUBY
```

A read-only, shared backend supplies a fixed object instead of a
callable:

```ruby
backend: Kobako::Extension::Backend.new(path: "File", provider: read_only_store)
```

## Rust SDK

The `crates/kobako` host SDK reifies the same contract idiomatically;
behavior is parity-pinned to the Ruby frontend, only the API shape
differs.

```rust
pub trait Extension {
    fn name(&self) -> &str;
    fn source(&self) -> &str;
    fn depends_on(&self) -> &[&str] { &[] }
    fn backend(&self) -> Option<Backend> { None }
}

pub struct Backend { pub path: String, pub provider: Provider }

pub enum Provider {
    Static(Arc<dyn Receiver>),                                       // fixed
    PerInvocation(Arc<dyn Fn() -> Arc<dyn Receiver> + Send + Sync>), // fresh each invocation
}

// sandbox.install(ext: Arc<dyn Extension>) -> Result<(), Error>
```

Provider identity maps to `Arc::ptr_eq`: the same `Arc` shared by
several Extensions resolves once per invocation to one object, mirroring
the Ruby object-identity rule.
