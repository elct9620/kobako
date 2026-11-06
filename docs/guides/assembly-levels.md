# Assembly Levels

kobako is assembled from parts, and you choose how many of them are yours.
This document maps four levels. Each widens what is yours and costs more to stand on.
Find the one that varies what you need varied, and stop there.

Read this before [`customization.md`](customization.md).
This one tells you which interfaces you will meet; that one names each and what kobako promises in return.
What implementing a trait obliges is in that trait's rustdoc.

| Document | Answers |
|---|---|
| [`variants.md`](variants.md) | what we ship |
| this one | which level to stand on |
| [`customization.md`](customization.md) | which interfaces that level exposes, and what kobako promises for each |

## The Ladder

Each level names what you depend on, and all four stand on the same fixed pillar.

```
 what you name

 L1  gem "kobako" + data/kobako.wasm ------+
 L2  gem "kobako" + your guest ------------+
 L3  the kobako crate ---------------------+
 L4  kobako-transport + kobako-runtime ----+
                                           |
                                           v
                                +--------------------+
                                |  kobako-transport  | (a)
                                |  the Envelope      |
                                |  the ABI's values  |
                                +--------------------+
```

- (a) The fixed pillar depends on nothing, and every level depends on it. It is the same in every assembly, which is what makes the parts interchangeable at all.

## Level Ownership

These two tables show what is yours at each level, guest side first.

| | capability gems | invocation flows | guest language |
|---|---|---|---|
| L1 | `kobako-io` | the `MrbGuest` flows | mruby |
| L2 | yours | yours | yours |
| L3 | yours | yours | yours |
| L4 | yours | yours | yours |

| | Codec | Engine | Frontend |
|---|---|---|---|
| L1 | MessagePack | wasmtime | the Ruby gem |
| L2 | MessagePack ⚠ | wasmtime | the Ruby gem |
| L3 | yours | yours | the Rust SDK's ⚠ |
| L4 | yours | yours | yours |

The ⚠ cells are the traps: the freedom looks available from where you stand, and is not.
The other fixed cells surprise nobody: you picked the Ruby gem, so you get its Engine.

## L1 Bundled Assembly

L1 is `gem "kobako"` and the `data/kobako.wasm` it ships, with every part chosen for you but one.
The Posture is yours to lower, and it defaults to hermetic.

| Part | Choice |
|---|---|
| Codec | MessagePack |
| capability | `kobako-io` only |
| Posture | hermetic by default; `profile: :permissive` brings no live clock, since the bundled Guest Binary imports none |
| Engine | wasmtime |
| invocation flows | the provided `MrbGuest` flows |

A downloadable variant adds Regexp or JSON ([`variants.md`](variants.md)) and keeps you here.
A variant is a different artifact, not a different level.

## L2 Custom Guest

At L2 you build a Guest Binary of your own, and the Ruby gem drives it unchanged.

| Option | What you build |
|---|---|
| mruby | a leaf shell over `kobako-mruby` naming the `beni::Gem` set scripts may reach |
| mruby, other flows | the same shell, replacing an `MrbGuest` flow you do not want |
| another interpreter | any artifact that satisfies the ABI |

The gem drives whatever artifact you hand it, since the guest still speaks what the gem speaks.
Build the shell as a `cdylib` for `wasm32-wasip1`, then bake its boot state with `kobako-baker input.wasm output.wasm`.
The shipped shell, `wasm/kobako-wasm/src/guest.rs`, is a working one to start from.

The Codec is not yours here ⚠.
A guest shell names its Codec on `MrbGuest::Codec`, so the freedom looks available.
The Ruby Frontend has no matching seam: it speaks MessagePack directly.
A guest answering in anything else has nothing to answer to, so your own Codec means L3.

## L3 Rust Host

At L3 the `kobako` crate, the Rust SDK, is a second Frontend over the same Wasm Driver, and every part below it is yours.

| Part | How you pick it |
|---|---|
| Codec | build without the `msgpack` feature; each payload position is bytes you own |
| guest | any artifact satisfying the ABI, in any language |
| Engine | anything satisfying the `kobako-runtime` contract, via `Sandbox::with_runtime` |

The host model is the Rust SDK's ⚠.
Services bind at constant paths, and Handles are minted per Invocation.
Extensions compose over preload and bind, and registration seals at the first Invocation.
That shape is what the Rust SDK is, the same one the Ruby gem has.
Both Frontends are held to the same behavior ([`parity.md`](../process/parity.md)), so a different host model means L4.
[`plugin-rs`](../../examples/plugin-rs) is a host on the SDK, and [`fixed-schema-rs`](../../examples/fixed-schema-rs) replaces the payload schema with protobuf.

## L4 Custom Frontend

At L4 you compose `kobako-transport` and `kobako-runtime` directly and write the host model you want.

| You inherit | You owe |
|---|---|
| the Envelope and the ABI | everything above them |

This is not an exotic path: kobako's own Ruby gem is an L4 assembly, as is any host not written in Rust.
The gem stands on the same Wasm Driver as the Rust SDK without going through it.
[`wire-rs`](../../examples/wire-rs) assembles a host by hand on `kobako-wasmtime`, `kobako-runtime`, `kobako-transport`, and `kobako-codec`.

## Further Reading

Pick the next document by where this one left you.

| You are heading for | Read |
|---|---|
| L1, with a capability the default lacks | [`variants.md`](variants.md) |
| L2 or beyond | [`customization.md`](customization.md), the seams and what kobako promises for each |
| how the parts fit around the fixed pillar | [`architecture.md`](../architecture.md) |
| the wire itself | [`wire/`](../wire/README.md), starting from its overview |
| what the sandbox does and does not defend | [`security.md`](security.md) |
