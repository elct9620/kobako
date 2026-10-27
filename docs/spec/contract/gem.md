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

- `lib/**/*.rb`

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

Mount a Filesystem at a guest path, its block deciding in the access's place.

```ruby
module Kobako
  class Sandbox
    def mount(at, filesystem, access: :read, &policy)
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

## `Kobako::Filesystem::Memory.new`

A Filesystem held in memory and shared by every invocation, seeded with files. A `nil` limit or count lifts that bound.

| Attribute | Value |
| --- | --- |
| unverifiable | the native extension defines it through magnus, beyond the Ruby reading |

```ruby
module Kobako
  module Filesystem
    class Memory
      def self.new(files = {}, limit: 64 << 20, inodes: limit && (limit.ceildiv(4096) + 1), **seeded)
      end
    end
  end
end
```

## `Kobako::Filesystem::Directory.new`

A Filesystem over a host directory, reaching nothing outside it.

| Attribute | Value |
| --- | --- |
| unverifiable | the native extension defines it through magnus, beyond the Ruby reading |

```ruby
module Kobako
  module Filesystem
    class Directory
      def self.new(path)
      end
    end
  end
end
```

## `Kobako::Filesystem::Overlay.new`

A Filesystem stacking Layers into one view, empty until a Layer is added.

| Attribute | Value |
| --- | --- |
| unverifiable | the native extension defines it through magnus, beyond the Ruby reading |

```ruby
module Kobako
  module Filesystem
    class Overlay
      def self.new
      end
    end
  end
end
```

## `Kobako::Filesystem::Overlay#layer`

Add a Filesystem below the Layers so far, with its own access.

| Attribute | Value |
| --- | --- |
| unverifiable | the native extension defines it through magnus, beyond the Ruby reading |

```ruby
module Kobako
  module Filesystem
    class Overlay
      def layer(filesystem, access: :read)
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
