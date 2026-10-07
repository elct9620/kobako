# Assembly Levels

kobako is assembled from parts, and you choose how many of them are yours.
This document maps four levels. Each widens what is yours and costs more to stand on.
Find the one that varies what you need varied, and stop there.

Read this before [`customization.md`](customization.md).
This one tells you which interfaces you will meet; that one states what each obliges its implementer to do.

| Document | Answers |
|---|---|
| [`variants.md`](variants.md) | what we ship |
| this one | which level to stand on |
| [`customization.md`](customization.md) | what the interfaces at that level oblige you to |

## The Ladder

Each level names what you depend on, and all four stand on the same fixed pillar.

```
     what you name                        ┌──────────────────────────┐
                                          │      the fixed pillar     │
 L1  gem "kobako"                         │                          │
     └ the bundled data/kobako.wasm       │   kobako-transport       │
                                          │   ├ the core envelope    │
 L2  gem "kobako" + a guest you build     │   └ the ABI's values     │
     └ kobako-mruby, or your own guest    │                          │
                                          │   depends on nothing;    │
 L3  the kobako crate (Rust host SDK)     │   every level depends    │
     └ any guest, any engine              │   on it                  │
                                          │                          │
 L4  kobako-transport + kobako-runtime    │   the same in every      │
     └ a frontend you write               │   assembly, which is     │
       (kobako's own Ruby gem is one)     │   what makes the parts   │
                                          │   interchangeable at all │
                                          └──────────────────────────┘
```

## Level Ownership

These two tables show what is yours at each level, guest side first.

| | capability gems | invocation flows | guest language |
|---|---|---|---|
| L1 | `kobako-io` | the harness's | mruby |
| L2 | yours | yours | yours |
| L3 | yours | yours | yours |
| L4 | yours | yours | yours |

| | payload schema | wasm engine | host frontend |
|---|---|---|---|
| L1 | MessagePack | wasmtime | the Ruby gem |
| L2 | MessagePack ⚠ | wasmtime | the Ruby gem |
| L3 | yours | yours | the SDK's ⚠ |
| L4 | yours | yours | yours |

The ⚠ cells are the traps: the freedom looks available from where you stand, and is not.
The other fixed cells surprise nobody: you picked the Ruby gem, so you get its engine.

## L1 Bundled Assembly

L1 is `gem "kobako"` and the `data/kobako.wasm` it ships, with every choice made for you.

| Part | Choice |
|---|---|
| wire | MessagePack |
| capability | `kobako-io` only |
| isolation floor | hermetic |
| engine | wasmtime |
| invocation flows | the mruby harness's own |

A downloadable variant adds Regexp or JSON ([`variants.md`](variants.md)) and keeps you here.
A variant is a different artifact, not a different level.

## L2 Custom Guest

At L2 you build a Guest Binary of your own, and the Ruby gem drives it unchanged.

| Option | What you build |
|---|---|
| mruby | a leaf shell over `kobako-mruby` naming the `beni::Gem` set scripts may reach |
| mruby, other flows | the same shell, replacing a harness flow you do not want |
| another interpreter | any artifact that satisfies the ABI |

The gem drives whatever artifact you hand it, since the guest still speaks what the gem speaks.

The schema is not yours here ⚠.
A guest shell names its codec on `MrbGuest::Codec`, so the freedom looks available.
The Ruby frontend has no matching seam: it speaks MessagePack directly.
A guest answering in anything else has nothing to answer to, so your own schema means L3.

## L3 Rust Host

At L3 the `kobako` crate is a second frontend over the same driver, and every part below it is yours.

| Part | How you pick it |
|---|---|
| payload schema | build without the `msgpack` feature; each payload position is bytes you own |
| guest | any artifact satisfying the ABI, in any language |
| engine | anything satisfying the `kobako-runtime` contract, via `Sandbox::with_runtime` |

The host model is the SDK's ⚠.
Services bind at constant paths, and Handles are minted per invocation.
Extensions compose over preload and bind, and registration seals at the first invocation.
That shape is what the SDK is, the same one the Ruby gem has.
The differential parity harness holds the two to it, so a different host model means L4.

## L4 Custom Frontend

At L4 you compose `kobako-transport` and `kobako-runtime` directly and write the host model you want.

| You inherit | You owe |
|---|---|
| the wire and the ABI | everything above them |

This is not an exotic path: kobako's own Ruby gem is an L4 assembly, as is any host not written in Rust.
The gem reaches the driver through a magnus shim rather than the Rust SDK.

## Further Reading

Pick the next document by where this one left you.

| You are heading for | Read |
|---|---|
| L1, with a capability the default lacks | [`variants.md`](variants.md) |
| L2 or beyond | [`customization.md`](customization.md), the obligations each seam carries |
| how the parts fit around the fixed pillar | [`architecture.md`](../architecture.md) |
| the wire itself | [`wire-contract.md`](../wire-contract.md), then [`wire-codec.md`](../wire-codec.md) |
| what the sandbox does and does not defend | [`security.md`](security.md) |
