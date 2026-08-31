# Gem interface

The calls a host application writes against the Ruby gem. Registered here is
the way in — that each name exists and takes the shape a caller writes; what
happens behind it is the behavior specification's to say.

The surface a caller reaches through `attr_reader`, `Forwardable`, or a
`Data.define` member is deliberately absent: none of those is a definition in
the syntax tree, so a contract naming one would answer undefined however
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
