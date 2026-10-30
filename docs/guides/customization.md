# Customization

kobako is assembled from parts, and each assembly point is a named interface someone
outside this repository can implement. This document states what each one obliges
its implementer to do, and what kobako promises in return.

| Document | Covers |
|---|---|
| [`variants.md`](variants.md) | the prebuilt artifacts **we ship** |
| this document | the interfaces **a third party replaces** |
| [`assembly-levels.md`](assembly-levels.md) | which interfaces your starting point meets |

A variant is something to download; a customization point is something to
implement. Read the assembly levels first, so you never implement a seam your
starting point already fixed.

## Commitment Grades

Public names declared here are graded commitments, not guidance. A grade says what
kobako owes you, and it is set by what you do with the name.

| Grade | What you do with it | What kobako promises |
|---|---|---|
| **stable** | write it in your own signatures | no change without the owning crate's version increment |
| **append-only** | match, implement, or read a set | the set only gains members |
| **exhaustive** | match a set completely | a new member breaks your match on purpose |
| **replaceable** | write your own in kobako's place | the obligations below are all you owe |

An append-only enum carries `#[non_exhaustive]`, and a new trait method carries a
default unless it keeps a guarantee, as the Filesystem Obligations below do. An exhaustive set has neither, so there is no wildcard to fall into. kobako
swapping its own replaceable implementation is not a break.

A struct with public fields is graded by whether its fields can grow:

| Its fields | Mark | Grade | You build it with |
|---|---|---|---|
| can grow | `#[non_exhaustive]` | append-only | its `new`, or `Default` and then each field |
| fixed by what it models | none | stable · exhaustive | a struct literal |

Source compatibility is the owning crate's semantic version; wire compatibility is
the ABI version. A rename moves no byte, and a wire change need not touch a name.
Both govern only the fixed tier, where the shape of the name *is* the wire.

## Replaceable Points

Each row is one seam a third party can fill, with the endpoint it sits on.

| Point | Interface | Endpoint | Grade |
|---|---|---|---|
| Payload codec | `MrbGuest::Codec: PayloadCodec` | guest | replaceable · methods append-only |
| Payload codec | the byte-level payload surface, `msgpack` feature off | Rust host SDK | stable |
| Capability set | `MrbGuest::init_gems` | guest | replaceable |
| Flags | `MrbGuest::apply_flag` | guest | replaceable |
| Invocation flow | a `Guest` method implemented rather than forwarded | guest | replaceable · methods append-only |
| The whole guest | `impl kobako_core::Guest` + `export_guest!` | guest | replaceable · methods append-only |
| Wasm engine | `impl Runtime`, handed to `Sandbox::with_runtime` | Rust host SDK | replaceable |
| Wasm engine | `impl Runtime` + `DispatchHandler` + `Yielder` | a host frontend of your own | replaceable |
| Filesystem | `impl InodeOperations` + `FileOperations`, behind a Mount | Rust host SDK | replaceable · methods append-only |
| Lock wait | `impl Blocking`, handed to `Filesystem::with_blocking` or `Overlay::with_blocking` | a host frontend of your own | replaceable · methods append-only |

The types those seams carry, by grade:

| Grade | Names |
|---|---|
| **stable** | `export_guest!` · `kobako_core::proxy::dispatch` · `kobako_core::abi::*` · `kobako_mruby::{Kobako, dispatch, MsgpackCodec}` · `kobako_runtime::{Tmpfs, Bind, Inode, Overlay, Mount, MountPathError}` · `kobako_runtime::vfs::{Filesystem, Dentry, File, Identity, Inode, Path, PipeReader, PipeWriter, do_file_open, filename_mkdirat, filename_rmdir, filename_unlinkat, filename_renameat2, vfs_fstatat, do_readlinkat, do_utimes, PATH_MAX, NAME_MAX}` · `kobako_codec::msgpack::{Encode, Decode, Encoder, Decoder, MAX_NESTING_DEPTH}` · `kobako::{Sandbox, Execution, Context, Handles, RunPayload, Yielder}` · `kobako::handles::DetachedHandles` · `kobako::msgpack::IntoReceiver` · `kobako_wasmtime::Driver` |
| **stable · exhaustive** | `kobako_mruby::Arguments` · `kobako_runtime::{Profile, Entry, Completion, Access, Denial}` · `kobako_runtime::vfs::Error` · `kobako_codec::msgpack::{Value, Arguments}` · `kobako::{Backend, Provider, msgpack::RunArg}` |
| **append-only** | `kobako_core::DispatchError` · `kobako_mruby::{CodecError, InstallError}` · `kobako_runtime::{Trap, SetupError, InvokeError, Grant, GrantKind, Policy, FileType, FilesystemError, SeedError, Snapshot, Capture, Usage, OpenOptions, Metadata, DirEntry}` · `kobako_runtime::vfs::OpenHow` · `kobako_codec::msgpack::Error` · `kobako::{Error, Failure, YieldError, Receiver, Extension, Options}` · `kobako::msgpack::ValueReceiver` · `kobako_wasmtime::Config` |
| **append-only, read by an older peer as `Undefined`** | `kobako_transport::envelope::FaultKind`, re-exported as `kobako::FaultKind` |
| **append-only, read by an older peer as `Unknown`** | `kobako_transport::envelope::{Downcall, Upcall}` |
| **append-only, a record an older peer skips or refuses as its sender marked** | `kobako_transport::envelope::Preamble`, re-exported from `kobako_core` |
| **stable · exhaustive, and governed by the ABI version too** | `kobako_transport::abi::*` · every other `kobako_transport::envelope::*`, six re-exported from `kobako_core` |

`kobako::filesystem` re-exports `kobako_runtime::filesystem`, so each name there keeps the grade above.

Two things stay fixed. The **core envelope** and the **ABI surface** are the same for
every assembly, which makes the parts interchangeable at all
(→ [`wire/README.md`](../wire/README.md)). Both live in `kobako-transport`, which depends
on nothing, so an implementer picks up the fixed tier without anyone else's choices.

The **Ruby frontend is fixed to the default codec**. MessagePack is Ruby's native
choice and the gem speaks it directly, so it has no seam to substitute at.

## Payload Codec

A codec owns the payload positions and nothing else. The core envelope routes and
attributes without reading a payload byte, so a new codec leaves the envelope, the
ABI, and the version untouched.

A guest fills the positions its codec serves. A Rust host reaches every position,
because each has a byte-level entry, and the `msgpack` feature adds the bundled
codec's spelling beside it.

| Position | Byte-level entry | `msgpack` spelling |
|---|---|---|
| dispatch arguments and answer | `Receiver` | `ValueReceiver`, `into_receiver` |
| `run` payload | `RunPayload::bytes`, `RunPayload::build` | `RunPayload::values` |
| yield | `Yielder::call_payload` | `Yielder::call_values` |
| invocation result | `Execution::payload` | `Execution::value` |
| object behind a Handle | `Handles::resolve`, `Execution::resolve` | `resolve_as` |

A Handle is resolved by id, since spelling a Handle is the schema's job and reaching
the object is not. Each spelling is a thin wrapper over its entry, so another
schema's overlay is written outside the SDK as extension traits over the same entries.

### Codec Obligations

A codec fills positions, not an encoding. Writing a value is the floor every codec
owes. Each other position it serves or refuses as unserved, so a missing feature
reads apart from a broken guest.

| Position | What serving it obliges | Why |
|---|---|---|
| Yield Reply ok body, Outcome ok body | one value — **the floor** | every invocation answers an Outcome |
| Call payload | positional and keyword arguments, distinguishably | `public_send` does not interchange them |
| Run payload, Yield Call | the arguments alone; neither carries keywords | neither position has keywords |

How an unserved position refuses, and why a Call owes its Reply, is specified in
[codec](../spec/behavior/codec.md). Keep the Call payload's keywords apart from its rest
arguments: a codec folding them together makes `KV.get(key, limit: 9)` lose `limit:`
with nothing raising.

A Handle representation is optional. Without one, Handles ride only the envelope's
`target` field: a guest still reaches a stateful receiver and only forgoes passing
Handles as arguments. A Reply's fault body is no codec position either. A Fault is
kobako's own data and rides the envelope, so a schema neither encodes nor reads one.

### Naming the Codec

A guest shell names its codec once, on the associated type.

```rust
impl MrbGuest for MyGuest {
    type Codec = MyCodec;              // implements PayloadCodec
    fn init_gems(mrb: &Mrb) -> Result<(), beni::Error> { Ok(()) }
}
```

A Rust host names its choice by what it builds against. Every `msgpack` spelling,
including the `Value` they speak, lives in the `msgpack` module rather than the crate
root. Turning the feature off removes a spelling rather than leaving a hole, and a
host with its own schema then implements the byte-level surface. `run` takes
whichever payload it is handed, so the feature governs what a host is offered, never
what it can reach.

A type implementing two schemas' receiver traits has two `into_receiver` in scope.
Name one (`ValueReceiver::into_receiver(kv)`): binding an object is choosing the
schema the guest reaches it through.

### Codec-free Builds

Replaceability is a property of the dependency graph, not a flag. Each tier reaches
a codec only where it asks for one.

| Crate | Codec it reaches |
|---|---|
| `kobako-transport`, `kobako-core` | none |
| `kobako-mruby` | `MsgpackCodec` only with its `msgpack` feature |
| `kobako-wasm` (shipped shell) | MessagePack, by asking for it |
| `kobako` (Rust host SDK) | the `Value` surface, `msgpack` on by default |

A shell naming its own `MrbGuest::Codec` never asks for `msgpack`. The SDK defaults
to a codec because an embedder names it directly; with the feature off it reaches no
payload codec.

`crates:graph` and `wasm:graph` build each tier on every release and check that
no codec appears in the graph. Each build includes the tier's tests, since a library whose
tests still need a codec has not moved it out of the code. A `[dev-dependencies]`
codec is allowed, because no consumer installs it.

## Capability Set

`MrbGuest::init_gems` installs the shell's gems onto the freshly booted VM. Each gem
is a `beni::Gem`; `kobako-io`, `kobako-regexp`, and `kobako-json` are the worked
examples, and none depends on `kobako-mruby`. A shell installing nothing still boots
(→ [mruby](../spec/behavior/mruby.md)).

```rust
struct Greeter;

impl beni::Gem for Greeter {
    fn init(mrb: &Mrb) -> Result<(), beni::Error> {
        let class = mrb.define_class(c"Greeter", mrb.class_object())?;
        class.define_method(mrb, c"hello", beni::method!(hello, 1))?;
        Ok(())
    }
}

// A fixed argument list arrives as typed parameters; a failure goes back
// as `Err`, which beni raises at the guest call site.
fn hello(mrb: &Mrb, _self: Value, name: Value) -> Result<Value, beni::Error> {
    let who = String::try_convert(name, mrb)?;
    Ok(mrb.str_new(format!("hello {who}").as_bytes()).as_value())
}
```

That is the whole of a gem. It ships no Ruby source and needs no `mrbc` pipeline, so
the surface a guest sees is the one the Rust file registers.

### Carrying Rust Data

A class that carries Rust data rather than Ruby state is declared with
`#[beni::wrap]`, and the gem marks its carriers as it installs. The carrier-class
rules themselves are beni's.

```rust
#[beni::wrap(class = "Counter", name = "Greeter::Counter")]
struct Counter {
    hits: u32,
}

// inside the gem's init
mrb.define_class(c"Counter", mrb.class_object())?;
Counter::mark_carriers(mrb)?;
```

### Reaching the Host

A gem that reaches the host names one more tier: `kobako-mruby`, whose `dispatch`
rounds one Call through the host. A method taking a block passes it along, as the
block read off its own frame or `None`; that is the whole obligation. A guest that
is not mruby calls `kobako_core::proxy::dispatch` directly and states the
`block_given` bit itself. A yield names the open Call whose block it runs. So the
guest counts every Call it has open, block or not, from `0` at the outermost.

```text
  gem ──► dispatch(target, method, Option<Proc>, payload) ──► Service
           │                                                    │
           └─ block parks for the call ◄── yield ───────────────┘
              (a yield downcall re-enters while the dispatch
               frame is still on the stack)
```

That tier carries no payload codec, so the gem still names its own schema. It
encodes the payload before calling and decodes the answer after, which keeps a
guest-side raise clear of the parked block.

## Flags

`MrbGuest::apply_flag` receives each flag the host set, before any binding or snippet,
and answers whether the shell recognises it. The default recognises none. An `Err`
means the Guest Binary could not start the run, which the host raises as `SandboxError`.

```rust
fn apply_flag(mrb: &Mrb, flag: &Flag) -> Result<bool, Error> {
    match flag.name.as_str() {
        "strict_mode" => {
            mrb.define_global_const(c"STRICT", true)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
```

The host marks each flag, so the marking follows what a skipped flag would change.

| A skipped flag | Mark it | Unrecognised, the run |
|---|---|---|
| changes nothing a script observes | advisory | goes on without it |
| enables something a script uses | critical | fails with `UnsupportedFlagError` |
| imposes a restriction | critical | fails rather than run unrestricted |

## Guest Replacement

A shell replaces one invocation flow, or the whole guest, and the host cannot tell
the difference.

| Replace | Implement | Obligation |
|---|---|---|
| one invocation flow | that `Guest` method, not forwarded | answer with its Outcome, Yield Reply, or Compile Reply |
| the whole guest | `kobako_core::Guest` + `export_guest!` | the ABI's exports, import, and version |

`MrbGuest` provides `eval`, `run`, `yield_to_block`, and `compile` over mruby. A non-mruby guest
skips `kobako-mruby` entirely. `wasm/kobako-wasm` takes this same path, so the
shipped shell is not privileged. The ABI is in
[`wire/abi.md`](../wire/abi.md).

## Wasm Engine

`crates/kobako-runtime` is the engine-free contract, and `crates/kobako-wasmtime` is
one implementation of it. No frontend names an engine type, so an engine satisfying
the contract carries every frontend above it unchanged.

| Contract part | Names |
|---|---|
| engine seams | `Runtime`, `DispatchHandler`, `Yielder` |
| declared posture | `Profile` |
| per-invocation types | `Snapshot`, `Completion`, `Capture`, `Usage`, `Trap` |

The Rust host SDK takes an engine at `Sandbox::with_runtime` and keeps the whole tier
above it: Catalog, Handles, snippet replay, Extension composition. `Sandbox::new` is
the same path with the bundled wasmtime engine.

`Runtime::compile` is optional. An engine that keeps its default compiles nothing, and
the SDK replays the preloaded source on every invocation instead of its bytecode.

Only the isolation floor and each invocation's Grants cross that seam. The engine's
own caps are configured where the engine is built. The SDK checks the posture it
declares and refuses a Mount it does not support
(→ [runtime](../spec/behavior/runtime.md)).

### Engine-free Builds

As with the codec, the engine's replaceability is a property of the dependency graph.

| `kobako` build | Way in |
|---|---|
| `wasmtime` feature on (default) | `Sandbox::new` or `Sandbox::with_runtime` |
| `wasmtime` feature off | `Sandbox::with_runtime` only; no engine reached |

`crates:graph` builds the SDK that way on every release and checks that no
engine appears in the graph.

The Ruby frontend takes no such seam. `Kobako::Runtime` is pinned to the wasmtime
driver, so choosing an engine there means choosing a different frontend.

## Filesystem

A Mount hands the guest a Filesystem made from any `impl InodeOperations`, and an Overlay
stacks Filesystems as Layers.
The runtime walks each guest path one name at a time and checks it before the
Filesystem acts on it. It follows a link itself, so a Filesystem only answers one
name in a directory it already found.

```
guest path ─▶ runtime: walk name by name, follow links, check the Policy ─▶ Filesystem
                              ▲                                               │
                              └── lookup: the entry at a name ────────────────┘
```

Each entry answers for itself: a `lookup` finds an `Inode`, which holds that entry's
operations. A wrapper wraps each entry its `lookup` finds through `Inode::with_ops`,
so it still sees every call beneath it.
Every call hands the Dentry it is about. A Filesystem answering by path reads it
from `Dentry::path_raw`, so a directory moved through the runtime is asked at its
new path. Its `lookup` finds an `Inode::by_path`. Any other finds an `Inode::new`
with its identity, or a Policy cannot tell a directory from one made in its place.

### Filesystem Obligations

Only `getattr` and the `FileOperations` methods are required. An operation an entry lacks answers
`None`, as a NULL in Linux's `inode_operations`, and the runtime answers for it as Linux's
VFS does. One wrapping another forwards those it keeps.

| Method | Keeps | Without it, the runtime answers |
|---|---|---|
| `lookup` | one dentry and lock per entry, and a Policy checking the stored spelling | ENOTDIR, no directory to walk through |
| `dentry_ops` | a held dentry dropped once its name stands for another entry, through `DentryOperations::revalidate` | valid, as nothing changes it outside the runtime |
| `get_link` | a link followed only within the directory a descriptor opened | EINVAL |
| `create` | a file made under the directory's lock | EACCES |
| `FileOperations::iterate_shared` | a directory listed from what its `open` made | ENOTDIR |
| `FileOperations::fsync` | what was written made durable; answer `Ok` with nothing to flush | EINVAL |
| `mkdir`, `rmdir`, `unlink`, `rename` | a change made under the directory's lock | EPERM |
| `FileOperations::append` | another writer's append from landing over this one | `Unsupported`, which it answers itself |

An Overlay waits for a copy up's lock through `Blocking`. A frontend whose threads share a
lock of their own gives that lock up inside `wait`, as Ruby gives up the GVL.
Otherwise the holder it waits for never finishes.
