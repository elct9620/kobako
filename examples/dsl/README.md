# Host-owned DSL — the guest writes the builder, the host owns it

A self-contained script that gives the guest an idiomatic, receiver-less
builder DSL — `header "..."`, `body do ... end` — whose every dialect
lives on the **host**. No builder logic is reimplemented in the guest:
one small generic wrapper forwards each call onto a Capability Handle, so
the host's `Card` / `Section` / `Image` classes stay the single source of
truth. It is the [`Kobako::Extension`](../../docs/extensions.md) companion
to the [vfs](../vfs/README.md) demo — that installs a native `File`; this
installs a builder idiom over host dialects.

## The shape, and why it has to be this shape

A host-object DSL needs no new kobako feature. It composes three
behaviours that already exist:

```
Studio.card              ->  a Service returns a stateful Card (crosses as a Handle)
card.body                ->  a method on that Handle returns a child Section Handle
Build.new(handle) { ... }->  a guest-LOCAL instance_eval descends into each child
```

The reflection denial is scoped to guest→host dispatch and to
`Kobako::Proxy` / Handle proxies ([`T-117`](../../docs/spec/behavior/transport-boundary.md),
[`T-114`](../../docs/spec/behavior/transport-boundary.md)), so `instance_eval` on a
plain guest-local object — the `Build` wrapper — is permitted. That is
the whole trick: the wrapper rebinds `self` to each returned child, so
the guest writes a receiver-less DSL, while the vocabulary at each level
is exactly what the host object at that level defines.

Because the host resolves every forwarded call, a method no dialect
defines is refused host-side ([`T-121`](../../docs/spec/behavior/transport-boundary.md)). The wrapper can never widen the
reachable surface — the DSL's vocabulary is bounded by the host's method
set, not by the wrapper.

## The one wrapper rule that matters

The wrapper wraps a returned Handle only **when a block is given** — a
container to descend into. A block-less call returns the **raw** Handle:

```ruby
def method_missing(name, *args, **kwargs, &blk)
  result = @handle.method_missing(name, *args, **kwargs)
  return result unless blk        # leaf / value -> raw Handle

  (result.is_a?(Kobako::Handle) ? Build.new(result) : result).instance_eval(&blk)
  self
end
```

That rule is what lets a value object travel as an argument. A leaf like
an image is fetched block-lessly (`logo = Studio.image(...)`) and then
passed on (`image logo`), where it crosses as a Handle and is restored to
the real host `Image` ([`T-003`](../../docs/spec/behavior/transport-dispatch.md)).
A guest wrapper object has no wire representation, so passing one as an
argument is refused ([`T-148`](../../docs/spec/behavior/transport-dispatch.md)) — keeping
leaves raw is what keeps them passable.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on
first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/dsl/app.rb
```

From a clone of the kobako repository, prefix with `bundle exec` so the
local checkout is used instead of the released gem.

## What to observe

The guest writes a receiver-less card, and the host returns the fully
assembled structure — built entirely host-side across a single
invocation:

```
guest DSL (receiver-less, every dialect lives on the host):
  logo = Studio.image(url: "...")   # leaf -> raw Handle
  Build.new(Studio.card).instance_eval do
    header "Welcome aboard"
    body do
      text "Thanks for joining."
      image logo                    # Handle passed as an argument
      group { ... }                 # a nested section dialect
    end
  end
```

Two things to read off the run. The `body`, `group`, and `footer` blocks
each switch `self` to a different host dialect with a different
vocabulary, yet no dialect class is written in the guest — the wrapper is
generic. And the final line shows that `marquee`, a verb no host dialect
defines, is refused: the host method set, not the wrapper, is the DSL's
boundary.

## Why this is safe

Nothing about the DSL widens the guest's authority. Every builder call is
an ordinary guest→host dispatch, resolved and reflection-checked host-side
([`T-117`](../../docs/spec/behavior/transport-boundary.md)) exactly like a plain Service call; the generic wrapper only adds
block-scoped `self`-rebinding in the guest, which touches no host state.
The host holds the growing tree in its own memory for the invocation — so
a Service exposing a builder should bound its own accumulation the way it
would any stateful Service. Per-invocation freshness (the callable
provider) rebuilds the factory each call, so no build leaks into the next.
