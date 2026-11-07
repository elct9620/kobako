# Gem interface

The names a host application writes against the Ruby gem. They are the calls it
makes, the values it reads, and the classes it rescues. Registered here is the
way in: that each name exists and takes the shape a caller writes. What happens
behind it is the behavior specification's to say.

The surface a caller reaches through `attr_reader`, `Forwardable`, or a
`Data.define` member is deliberately absent. None of those is a definition in
the syntax tree. So a contract naming one would answer undefined however
plainly the code works.

## Includes

- `lib/kobako/context.rb`
- `lib/kobako/errors.rb`
- `lib/kobako/execution.rb`
- `lib/kobako/extension.rb`
- `lib/kobako/filesystem.rb`
- `lib/kobako/handle.rb`
- `lib/kobako/network.rb`
- `lib/kobako/pool.rb`
- `lib/kobako/sandbox.rb`
- `lib/kobako/transport/error.rb`
- `lib/kobako/unresolved.rb`

## `Kobako::Sandbox`

One guest artifact, its Catalog, and the invocations run against it.

```ruby
module Kobako
  class Sandbox
  end
end
```

## `Kobako::Sandbox#bind`

Give a host object a name the guest can reach.

```ruby
module Kobako
  class Sandbox
    def bind(path, object = Unresolved)
    end
  end
end
```

## `Kobako::Sandbox#mount`

Mount a Filesystem at a guest path, its block refusing what the access allows.

```ruby
module Kobako
  class Sandbox
    def mount(at, filesystem, access: :read, &policy)
    end
  end
end
```

## `Kobako::Sandbox#network`

Give the guest a named network entry, a given block adding endpoints and
policies to it.

```ruby
module Kobako
  class Sandbox
    def network(name, entry = nil, &builder)
    end
  end
end
```

## `Kobako::Network::Endpoint#initialize`

What the guest may reach: a host, a port, a protocol, and where the host's name
may lead. An HTTP endpoint also holds the rules it judges requests by. Unless
written otherwise it speaks HTTP to public addresses and allows every request.

```ruby
module Kobako
  module Network
    class Endpoint
      def initialize(host:, port:, protocol: :http, addresses: [:public], rules: [])
      end
    end
  end
end
```

## `Kobako::Network::Rules#allow`

Allow a method, or `:any`, on the paths a pattern matches.

```ruby
module Kobako
  module Network
    class Rules
      def allow(method, path)
      end
    end
  end
end
```

## `Kobako::Network::Rules#deny`

Deny a method, or `:any`, on the paths a pattern matches. A denial wins over
any allowance.

```ruby
module Kobako
  module Network
    class Rules
      def deny(method, path)
      end
    end
  end
end
```

## `Kobako::Network::Rule`

One rule an HTTP endpoint judges a request by. It holds whether it allows, its
method or `nil` for any, and its path pattern.

```ruby
module Kobako
  module Network
    class Rule
      attr_reader :allow, :method, :path
    end
  end
end
```

## `Kobako::Network::Request`

One HTTP request the guest sends: the host, port, method, scheme, path with no
query, and header names. It never holds a header's value.

```ruby
module Kobako
  module Network
    class Request
      attr_reader :host, :port, :method, :scheme, :path, :header_names
    end
  end
end
```

## `Kobako::Network::Builder#request?`

Whether the given object's `request?` and each block's request policy allow a
Request.

```ruby
module Kobako
  module Network
    class Builder
      def request?(request)
      end
    end
  end
end
```

## `Kobako::Network::Builder#endpoint`

Let the guest reach a host on a port, as an endpoint takes them. A given block
adds the rules an HTTP endpoint allows and denies.

```ruby
module Kobako
  module Network
    class Builder
      def endpoint(host, port:, protocol: :http, addresses: [:public])
      end
    end
  end
end
```

## `Kobako::Network::Builder#endpoints`

The given object's endpoints, then the block's.

```ruby
module Kobako
  module Network
    class Builder
      def endpoints
      end
    end
  end
end
```

## `Kobako::Network::Builder#policy`

Judge each decision a hook names with the block. A falsy answer or a raised
StandardError refuses. `:resolve` hands the block the name a lookup asks for,
and `:connect` a Connection.

```ruby
module Kobako
  module Network
    class Builder
      def policy(hook, &block)
      end
    end
  end
end
```

## `Kobako::Network::Builder#resolve?`

Whether the given object's `resolve?` and each block's resolve policy allow a
lookup of a name.

```ruby
module Kobako
  module Network
    class Builder
      def resolve?(name)
      end
    end
  end
end
```

## `Kobako::Network::Builder#connect?`

Whether the given object's `connect?` and each block's connect policy allow a
Connection.

```ruby
module Kobako
  module Network
    class Builder
      def connect?(connection)
      end
    end
  end
end
```

## `Kobako::Network::Connection`

One connection the guest asks for: the host it named, its port, and its
protocol. The host is never a real address a name resolved to.

```ruby
module Kobako
  module Network
    class Connection
      attr_reader :host, :port, :protocol
    end
  end
end
```

## `Kobako::Filesystem`

The kinds of Filesystem a Host App mounts for the guest's `File`. An object
answering `open` and `stat` mounts as one written in Ruby. The methods such an
object answers are typed in `sig/kobako/filesystem.rbs`.

```ruby
module Kobako
  module Filesystem
  end
end
```

## `Kobako::Filesystem::Tmpfs.new`

A Filesystem held in memory and shared by every invocation, seeded with files. A `nil` size or count lifts that bound.

| Attribute | Value |
| --- | --- |
| unverifiable | the native extension defines it through magnus, beyond the Ruby reading |

```ruby
module Kobako
  module Filesystem
    class Tmpfs
      def self.new(files = {}, size: 64 << 20, nr_inodes: size && (size.ceildiv(4096) + 1), **seeded)
      end
    end
  end
end
```

## `Kobako::Filesystem::Bind.new`

A Filesystem over a host directory, reaching nothing outside it.

| Attribute | Value |
| --- | --- |
| unverifiable | the native extension defines it through magnus, beyond the Ruby reading |

```ruby
module Kobako
  module Filesystem
    class Bind
      def self.new(path)
      end
    end
  end
end
```

## `Kobako::Filesystem::Overlay.new`

A Filesystem stacking read-only lower Layers, topmost first, under an
optional upper Tmpfs into one view.

| Attribute | Value |
| --- | --- |
| unverifiable | the native extension defines it through magnus, beyond the Ruby reading |

```ruby
module Kobako
  module Filesystem
    class Overlay
      def self.new(lower:, upper: nil)
      end
    end
  end
end
```

## `Kobako::Sandbox#install`

Compose a guest idiom with its optional host backend.

```ruby
module Kobako
  class Sandbox
    def install(*extensions)
    end
  end
end
```

## `Kobako::Sandbox#preload`

Fix guest source or a compiled snippet into the Catalog before any invocation.

```ruby
module Kobako
  class Sandbox
    def preload(code: nil, name: nil, binary: nil)
    end
  end
end
```

## `Kobako::Sandbox#flag`

Set a Flag every invocation carries, critical unless marked advisory.

```ruby
module Kobako
  class Sandbox
    def flag(name, value = "", advisory: false)
    end
  end
end
```

## `Kobako::Sandbox#run`

Invoke an entrypoint already loaded in the guest.

```ruby
module Kobako
  class Sandbox
    def run(target, *args, **kwargs, &block)
    end
  end
end
```

## `Kobako::Sandbox#eval`

Invoke guest source supplied at the call.

```ruby
module Kobako
  class Sandbox
    def eval(code, &block)
    end
  end
end
```

## `Kobako::Pool`

A fixed set of Sandbox slots handed out one invocation at a time.

```ruby
module Kobako
  class Pool
  end
end
```

## `Kobako::Pool#with`

Check a Sandbox out for the block's duration and return it afterwards.

```ruby
module Kobako
  class Pool
    def with
    end
  end
end
```

## `Kobako::Execution`

The frozen record one invocation leaves, returned on success and carried on a failure.

```ruby
module Kobako
  class Execution
  end
end
```

## `Kobako::Execution#failed?`

Whether the invocation that left this record failed.

```ruby
module Kobako
  class Execution
    def failed?
    end
  end
end
```

## `Kobako::Execution#stdout`

What the guest wrote to its first descriptor.

```ruby
module Kobako
  class Execution
    def stdout
    end
  end
end
```

## `Kobako::Execution#stderr`

What the guest wrote to its second descriptor.

```ruby
module Kobako
  class Execution
    def stderr
    end
  end
end
```

## `Kobako::Execution#stdout_truncated?`

Whether the first descriptor's capture reached its cap.

```ruby
module Kobako
  class Execution
    def stdout_truncated?
    end
  end
end
```

## `Kobako::Execution#stderr_truncated?`

Whether the second descriptor's capture reached its cap.

```ruby
module Kobako
  class Execution
    def stderr_truncated?
    end
  end
end
```

## `Kobako::Context`

The invocation an override block is handed, spent once the block returns.

```ruby
module Kobako
  class Context
  end
end
```

## `Kobako::Context#bind`

Give a declared path a different object for this one invocation.

```ruby
module Kobako
  class Context
    def bind(path, object)
    end
  end
end
```

## `Kobako::Context#mount`

Give a mounted path a different Filesystem for this one invocation.

```ruby
module Kobako
  class Context
    def mount(at, filesystem, access: :read, &policy)
    end
  end
end
```

## `Kobako::Context#network`

Give a named network entry a different entry for this one invocation.

```ruby
module Kobako
  class Context
    def network(name, entry = nil, &builder)
    end
  end
end
```

## `Kobako::Handle`

The name a Host App matches a Handle by when one reaches it inside a failure.

```ruby
module Kobako
  class Handle
  end
end
```

## `Kobako::Unresolved`

The object a fillable path holds until a Host App fills it.

```ruby
module Kobako
  module Unresolved
  end
end
```

## `Kobako::Extension`

A guest idiom and its optional Backend, installed as one unit.

```ruby
module Kobako
  class Extension
  end
end
```

## `Kobako::Extension::Backend`

The host side of an Extension, declared by the keyword that names its kind.

```ruby
module Kobako
  class Extension
    class Backend
    end
  end
end
```

## `Kobako::Error`

The root a caller rescues to catch every failure kobako raises.

```ruby
module Kobako
  class Error
  end
end
```

## `Kobako::TrapError`

An invocation the Wasm engine stopped.

```ruby
module Kobako
  class TrapError
  end
end
```

## `Kobako::TimeoutError`

A trap the invocation's deadline caused.

```ruby
module Kobako
  class TimeoutError
  end
end
```

## `Kobako::MemoryLimitError`

A trap the invocation's memory budget caused.

```ruby
module Kobako
  class MemoryLimitError
  end
end
```

## `Kobako::SandboxError`

An invocation the guest's own code or the wire failed.

```ruby
module Kobako
  class SandboxError
  end
end
```

## `Kobako::HandleExhaustedError`

An invocation that ran out of Handle ids.

```ruby
module Kobako
  class HandleExhaustedError
  end
end
```

## `Kobako::BytecodeError`

Preloaded bytecode that will not load.

```ruby
module Kobako
  class BytecodeError
  end
end
```

## `Kobako::UnsupportedFlagError`

A critical Flag the Guest Binary does not recognise.

```ruby
module Kobako
  class UnsupportedFlagError
  end
end
```

## `Kobako::UndefinedEntrypointError`

A `#run` target the guest does not define.

```ruby
module Kobako
  class UndefinedEntrypointError
  end
end
```

## `Kobako::Transport::Error`

A wire violation, raised on either side under the same name.

```ruby
module Kobako
  module Transport
    class Error
    end
  end
end
```

## `Kobako::ServiceError`

An invocation a Service call failed.

```ruby
module Kobako
  class ServiceError
  end
end
```

## `Kobako::NoServiceError`

A dispatch that reached no Service method.

```ruby
module Kobako
  class NoServiceError
  end
end
```

## `Kobako::ServiceArgumentError`

Arguments a Service method refused.

```ruby
module Kobako
  class ServiceArgumentError
  end
end
```

## `Kobako::SetupError`

A Sandbox that could not be constructed.

```ruby
module Kobako
  class SetupError
  end
end
```

## `Kobako::ModuleNotBuiltError`

A Guest Binary not built yet at the configured path.

```ruby
module Kobako
  class ModuleNotBuiltError
  end
end
```

## `Kobako::BlockError`

A guest block that failed, raised at the Service's yield site.

```ruby
module Kobako
  class BlockError
  end
end
```

## `Kobako::YieldValueError`

A yield argument the wire cannot carry.

```ruby
module Kobako
  class YieldValueError
  end
end
```

## `Kobako::UnsupportedError`

A guest that does not serve a Service's yield, raised at that yield.

```ruby
module Kobako
  class UnsupportedError
  end
end
```

## `Kobako::PoolTimeoutError`

A Pool checkout that waited past its bound.

```ruby
module Kobako
  class PoolTimeoutError
  end
end
```
