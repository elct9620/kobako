# ABI

The ABI is the set of Wasm functions that carry both [wire](README.md) layers between host and guest, and the version that pins them.

| Section | Holds |
|---------|-------|
| ABI Signatures | the symbols and the layers they form |
| Downcall and Upcall | how a request and its Reply cross |
| Invocation inputs | what each Invocation reads, and from where |
| ABI Version | what the version pins, and how it moves |

## ABI Signatures

The surface is four functions and a memory, named as the Component Model's WIT world `kobako:abi` would name them. Signatures here use Wasm's own types; a Rust side spells the same values as `u32`.

| Symbol | Linkage | Wasm signature | Layer |
|---|---|---|---|
| `kobako:abi/guest@0.6.0#downcall` | export | `(ptr: i32, len: i32) -> i32` | message |
| `upcall` from `kobako:abi/host@0.6.0` | import | `(ptr: i32, len: i32, ret: i32)` | message |
| `preamble` from `kobako:abi/host@0.6.0` | import | `(ret: i32)` | setup |
| `cabi_realloc` | export | `(old_ptr: i32, old_len: i32, align: i32, new_len: i32) -> i32` | memory |
| `memory` | export | linear memory | memory |

Each name follows the Canonical ABI's legacy core naming, so the same core module wraps into a component unchanged. The WIT these lower from is:

```wit
package kobako:abi@0.6.0;

interface host {
  upcall: func(request: list<u8>) -> list<u8>;
  preamble: func() -> list<u8>;
}

interface guest {
  downcall: func(request: list<u8>) -> list<u8>;
}
```

`cabi_realloc` gives the host the guest memory every list lands in: a request, a Reply, the Preamble, and each list a WASI 0.2 import answers. With no room left it traps, so the host never writes at 0. Adding, removing, or renaming a symbol is an ABI version increment (→ [§ ABI Version](#abi-version)).

### Downcall and Upcall

Each direction has one symbol, named for the way it crosses. An operation is a tag in the message rather than a symbol (→ [`envelope.md`](envelope.md) § Message Layer).

| Symbol | Caller | Carries |
|---|---|---|
| `downcall` | host | eval, run, yield, compile |
| `upcall` | guest | call |

1. The caller writes a message at `[ptr, ptr + len)` in guest memory; the host takes that room from `cabi_realloc`.
2. The callee reads it, handles the operation its tag names, and encodes the Reply.
3. The callee writes the Reply into guest memory and returns its place in a return area.

A Call the host cannot answer is answered on the fault arm, naming why. When not even that fits in guest memory, the Invocation ends in a Trap naming the reason.

### Call Nesting

Initiators alternate by depth, and every Reply is the return value of the call at its depth. Correlation comes from the Wasm call stack, so no answer carries an id. A Yield alone names a Call: the open Call whose Block it runs.

```
downcall{run}                  <- host, depth 0
+- upcall{call}                <- guest, depth 1
   +- downcall{yield}          <- host, depth 2 (a)
      +- upcall{call}          <- guest, depth 3
```

a. Sent from inside the Service's `yield`.

A Yield nests inside the upcall the host is still answering. The Yield Call and its answer are in [`envelope.md`](envelope.md) § Yield Messages.

### Return Area

Every answer is a `list<u8>`, named by two u32 values in guest memory, as the Canonical ABI lays a list out. Every pointer refers to guest linear memory.

```
 ret          ret + 4
 +-------------+-------------+
 |     ptr     |     len     |
 +-------------+-------------+
  u32, little-endian, 4-byte aligned
```

| Symbol | Who writes the pair | Where |
|---|---|---|
| `downcall` | guest | the address it returns |
| `upcall`, `preamble` | host | the `ret` the guest passed |

A pair off its alignment or outside guest memory is refused, and so is a length past the message cap. An empty answer frames no message, so the reader refuses it rather than decoding it.

The reader copies the answer before the call returns and keeps no reference to it. An answer lives only as long as the instance that holds it. Every Invocation entry gets a fresh instance, so no `cabi_post_*` export frees one.

---

## Invocation inputs

An Invocation takes its entry message from the downcall and its Preamble from the `preamble` import. WASI stdin carries none of it.

```
Host                                     Guest
  |  downcall: eval | run | compile        |
  |--------------------------------------->|
  |                              preamble  |
  |<---------------------------------------|
  |  Preamble bytes                        |
  |--------------------------------------->|  decode, entry
```

| Operation | Downcall | Preamble |
|---|---|---|
| eval | `eval` with the user source | taken first |
| run | `run` with the Run envelope | taken first |
| compile | `compile`, with no body | taken first |
| yield | `yield` with the frame and arguments | not taken |

A compile takes the same Preamble an Invocation would, so its Bytecode matches what that Invocation replays, and runs none of it. Within one Invocation, `preamble` answers the same bytes each time it is asked, as `wasi:cli/environment` answers its values. The Preamble is a message of its own under the same cap as every other; what it holds is in [`envelope.md`](envelope.md) § Preamble.

---

## ABI Version

The ABI version rides in the names: `0.6.0` in `kobako:abi/guest@0.6.0` and `kobako:abi/host@0.6.0`. It is independent of every package version, and only a change no reader survives moves it.

| Inside the version | Outside the version |
|--------------------|---------------------|
| the symbol set, names, signatures | a new request tag or Preamble record kind in the registry |
| the return area convention | a payload Codec swap |
| the message framing, a redefined byte value, a new answer arm | the other growth a predating reader answers (→ [`README.md`](README.md) § Growth) |

The version follows WIT's semver as WASI's own 0.x packages do: ABI version `N` is `0.N.0`, and a break moves the minor. The host reads the version from the downcall's export name, before any entry runs, and accepts a Guest Binary only when it equals its own. A Guest Binary exporting no `kobako:abi/guest` downcall, or one under another version, does not run (→ [`spec/behavior/runtime.md`](../spec/behavior/runtime.md)).

### Lockstep Evolution

A host implements exactly one ABI version, so an increment is the cost of a change no reader survives.

| Rule | Consequence |
|------|-------------|
| No in-band version field | alignment checked once, not per message |
| No negotiation | each side implements one wire shape |
| Unsupported answer | a peer answers an unknown request, so a request tag joins without a bump |

A Sandbox is short-lived, so no connection or stored payload outlasts a version. An independently built Guest Binary conforms by rebuilding against the new one. Release notes list wire-affecting changes under Breaking Changes.

### Version 6

Version `6` takes the Canonical ABI's core shape, so the Guest Binary imports WASI 0.2 as a core module and wraps into a component unchanged.

| Decision | Consequence |
|----------|-------------|
| names in the legacy core form | the version is read from a name, never from a call |
| one allocator, `cabi_realloc`, that traps | the host never writes at an address it was not given |
| answers through a return area | no packed i64 crosses the boundary |
| the Preamble from an import, not stdin | descriptor 0 is free for the script's own input |
| ctors run once, at the first entry to an export other than `cabi_realloc` | an unbaked Guest Binary still runs them |

### Version 5

Version `5` carries the operation in the message, so an added operation is a registry entry rather than a new symbol. A Yield Call names the open Call whose block it runs.

| Decision | Consequence |
|----------|-------------|
| a Yield Call names its open Call | a stored block runs as itself from inside a later Call |
| a break carries nothing, on a Reply as on a Yield Reply | a block exit lands past any number of Calls, as through a plain method |
| one message symbol per direction | the symbol set stays small |
| one tag space for every message | an answer's tag never collides across operations |
| every Reply is a return value | a request and its Reply are one round trip |
| field placement follows whose data it is | a Fault rides the Envelope, readable without a Codec |
| setup is a counted list of records, each marked critical or advisory | a new kind of setup joins without a version |
| a fresh instance per Invocation entry | the guest may exit with dirty interpreter state |
| boot state may live in data segments | the guest may arrive pre-initialized |
