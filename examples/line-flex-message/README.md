# line-flex-message — a real DSL gem, driven from inside the sandbox

A self-contained script that runs the
[line-message-builder](https://github.com/elct9620/line-message-builder) Flex
DSL **inside a kobako sandbox**. The guest writes an idiomatic, receiver-less
`bubble do ... end`; the host owns the real builder. Every guest call forwards
onto the actual `Line::Message::Builder::Flex` nodes across the wasm boundary,
so the JSON printed is genuine LINE Flex output — paste it into LINE's
[Flex Message Simulator](https://developers.line.biz/flex-simulator/) to see it
render.

It is the [`Kobako::Extension`](../../docs/extensions.md) companion to the
[dsl](../dsl/README.md) demo: that one teaches the host-owned-DSL mechanism with
a toy builder; this one proves the same mechanism against a real gem whose
output a third party can validate.

## The shape

Two small generic pieces bridge the gem to the sandbox — neither reimplements
any builder logic:

```
Build     (guest)  a receiver-less wrapper: instance_eval descends into each
                   returned child Handle, so `body do text "..." end` reads
                   naturally while every verb resolves on the host.
Buildable (host)   adapts a line-message-builder node so a caller without a
                   block can descend: the gem's box children return the mutated
                   `contents` Array, so the adapter hands back the new child.
```

`Studio` is bound at the guest constant `Flex`; `Flex.bubble` / `Flex.carousel`
mint a fresh root node that crosses to the guest as a Handle. The DSL vocabulary
is exactly what the gem's nodes define — a verb no node defines is refused
host-side, so the guest can never widen it.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first
run — no `Gemfile` is required in the working directory.

```bash
ruby examples/line-flex-message/app.rb --example default
ruby examples/line-flex-message/app.rb --example cards
ruby examples/line-flex-message/app.rb --example receipt
```

From a clone of the kobako repository, prefix with `bundle exec` so the local
checkout is used instead of the released gem.

## What to observe

`--example default` prints a single Flex bubble — a café card with a hero
image, a bold title, baseline info rows, and footer buttons. `--example cards`
prints a carousel the guest builds by looping over a menu, one bubble per item.
`--example receipt` shows dynamic content: the host injects an order through
`#run` and the guest template loops its line items into rows, so the same
template renders a different card for different data. Its banner URL comes from
an `Assets` helper Service bound on the host — the sandbox-side stand-in for a
view-context helper.

The banner prints to stderr and the JSON to stdout, so the output pipes cleanly:

```bash
ruby examples/line-flex-message/app.rb --example default | pbcopy
```

Paste it into the Flex Simulator: it renders as a real LINE message, built
entirely by the gem, driven entirely from guest source running in the sandbox.
