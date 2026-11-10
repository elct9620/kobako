# Customization

kobako is assembled from parts, and each assembly point is a named interface someone
outside this repository can implement. This document names each one and what kobako
promises in return. What implementing a trait obliges is in that trait's rustdoc.

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
| **replaceable** | write your own in kobako's place | the documented obligations are all you owe |

An append-only enum carries `#[non_exhaustive]`, and a new trait method carries a
default unless it keeps a guarantee no default can. An exhaustive set has neither, so
there is no wildcard to fall into. kobako swapping its own replaceable implementation
is not a break.

Source compatibility is the owning crate's semantic version; wire compatibility is
the ABI version. A rename moves no byte, and a wire change need not touch a name.
Both govern only the fixed pillar, where the shape of the name *is* the wire.

### Graded Structs

A struct is graded by whether its fields can grow:

| Its public fields | Mark | Grade | You build it with |
|---|---|---|---|
| can grow | `#[non_exhaustive]` | append-only | a constructor it offers, then each field |
| fixed by what it models | none | stable · exhaustive | a struct literal |
| none | none | stable | nothing; you pass on what kobako made |

`Iattr::size` and `Kiocb::at` are such constructors. `DirEmit` and `Failure` offer
none, so you only read what kobako hands you. `OpenFlags` you only pass from
`build_open_flags` to `do_file_open`, so what it holds may change beneath you.

## Replaceable Points

Each row is one seam a third party can fill, with the endpoint it sits on.

| Point | Interface | Endpoint | Grade |
|---|---|---|---|
| Payload Codec | `MrbGuest::Codec: PayloadCodec` | guest | replaceable · methods append-only |
| Payload Codec | the byte-level payload surface, `msgpack` feature off | Rust SDK | stable |
| Capability set | `MrbGuest::init_gems` | guest | replaceable |
| Flags | `MrbGuest::apply_flag` | guest | replaceable |
| Invocation flow | a `Guest` method implemented rather than forwarded | guest | replaceable · methods append-only |
| The whole guest | `impl kobako_core::Guest` + `export_guest!` | guest | replaceable · methods append-only |
| Wasm engine | `impl Runtime` + `GuestInstance`, handed to `Sandbox::with_runtime` | Rust SDK | replaceable |
| Wasm engine | `impl Runtime` + `DispatchHandler` + `Yielder` | a Frontend of your own | replaceable |
| Filesystem | `impl FileSystemType` + `InodeOperations` + `FileOperations` + `DentryOperations`, behind a Mount | Rust SDK | replaceable · methods append-only |
| Host wait, for a lock or for I/O | `impl Blocking`, handed once to `sched::register_blocking` | a Frontend of your own | replaceable · methods append-only |
| Network policy | `impl Policy<Name>`, `Policy<Connection>`, or `Policy<Request>`, handed to a `network::Builder` | Rust SDK | replaceable |
| Credential | `impl CredentialSource`, handed to `Credential::new` | Rust SDK | replaceable |
| Protocol Family | `impl ProtocolFamily`, handed to `Namespace::new` | a Wasm Driver of your own | replaceable · methods append-only |
| Outbound HTTP | `impl Outbound`, handed to `Namespace::new` | a Wasm Driver of your own | replaceable · methods append-only |
| security module | `impl SecurityOperations`, handed to `kobako_wasmtime::Config::security` | Rust SDK | replaceable · methods append-only |

### Graded Names

The types those seams carry, by grade:

| Grade | Names |
|---|---|
| **stable** | `export_guest!` · `kobako_core::proxy::dispatch` · `kobako_core::abi::*` · `kobako_mruby::{Kobako, dispatch, MsgpackCodec}` · `kobako_codec::msgpack::{Encode, Decode, Encoder, Decoder}` · `kobako_codec::msgpack::codec::MAX_NESTING_DEPTH` · `kobako::{Sandbox, Execution, Context, Handles, RunPayload, Yielder}` · `kobako::extension::ProviderFn` · `kobako::handles::DetachedHandles` · `kobako::msgpack::IntoReceiver` · `kobako_wasmtime::Driver` |
| **stable** | `kobako_runtime::{Tmpfs, Bind, Overlay, Mount, MountPathError}` · `kobako_runtime::filesystem::Filesystem` · `kobako_runtime::filesystem::{PipeInodeInfo, Rdir, create_pipe_files}` · `kobako_runtime::vfs::{Cred, Dentry, DirContext, File, FileSecurity, FMode, Identity, Inode, Mask, OpenFlags, Path, PollMask, Start, SuperBlock, VfsMount, build_open_flags, do_file_open, filename_mkdirat, filename_rmdir, filename_unlinkat, filename_renameat2, filename_linkat, filename_symlinkat, dir_emit_dots, generic_file_llseek, generic_read_dir, generic_write_sync, vfs_fstatat, do_readlinkat, do_utimes_path, do_utimes_fd, verify_dirent_name, PATH_MAX, NAME_MAX}` · `kobako_runtime::security::{WalkGuard, prepare_creds_within, restrict_self}` · `kobako_runtime::endpoint::{Calls, MemoryCap, MemoryLimitExceeded, ABANDONED, RUNTIME_INCOMPATIBLE, clip_capture, compile_request, compiled_bytecodes, handle, pipe_capacity, read_compile_reply, read_outcome, request_of, stdin_content, verify_abi_version, write_request}` · `kobako_runtime::sched::{Task, run_task}` · `kobako_runtime::wait::{WaitQueueHead, PollTable, poll_wait, do_poll}` |
| **stable** | `kobako_runtime::network::{Network, Secret, EntryError, Builder, Ruleset, address_set_of}` · `kobako_runtime::net::{HostInet, OpenConnections, process_connections, RequestBody, Connector, Transport, Exchange, HyperOutbound, ResponseBody, ResponseFuture, NameLookup, Namespace, TcpSock, UdpSock, Fields, sock_alloc_file, new_platform_tls, MAX_SOCKET_BUFFER, MAX_DATAGRAM, MAX_FIELD_SIZE}` |
| **stable · exhaustive** | `kobako_mruby::Arguments` · `kobako_runtime::{Profile, Entry, Completion, Access}` · `kobako_runtime::vfs::{Lookup, SecurityPath, SetTime, Whence}` · `kobako_runtime::endpoint::UpcallEnd` · `kobako_codec::msgpack::{Value, Arguments}` · `kobako::{Backend, Provider, msgpack::RunArg}` · `kobako_runtime::network::AddressSet` · `kobako_runtime::net::{ResponseHead, Response}` |
| **append-only** | `kobako_core::DispatchError` · `kobako_mruby::{CodecError, InstallError}` · `kobako_runtime::{Trap, SetupError, InvokeError, Grant, GrantKind, Policy, MountRequest, FileType, FilesystemError, SeedError, Snapshot, Capture, Usage, Kstat, DirEntry}` · `kobako_runtime::filesystem::{RenameFlag, Finish}` · `kobako_runtime::vfs::{OpenHow, DirEmit, Iattr, Kiocb}` · `kobako_codec::msgpack::Error` · `kobako::{Error, Failure, YieldError, Receiver, Extension, Options}` · `kobako::msgpack::ValueReceiver` · `kobako_wasmtime::Config` · `kobako_runtime::network::{Entry, CredentialSource, Placement, Credential, Policies, Request, Name, Connection, Endpoint, HttpRule, Protocol, Addresses}` · `kobako_runtime::net::{ProtocolFamily, Outbound, ProtoOps, SockOpt, Scheme, OutgoingRequest, HttpError, LookupError, FieldError}` |
| **append-only, read by an older peer as `Undefined`** | `kobako_transport::envelope::FaultKind` |
| **append-only, read by an older peer as `Unknown`** | `kobako_transport::envelope::{Downcall, Upcall}` |
| **append-only, a record an older peer skips or refuses as its sender marked** | `kobako_transport::envelope::Preamble`, re-exported from `kobako_core` |
| **stable · exhaustive, and governed by the ABI version too** | `kobako_transport::abi::*` · every other `kobako_transport::envelope::*`, six re-exported from `kobako_core` |

A re-export keeps the grade of what it re-exports. The `kobako` root re-exports
`Usage`, `Fault`, `FaultKind`, `Access`, `Policy`, `MountRequest`, `Profile`, and `Filesystem`, and
`kobako::msgpack::Value` re-exports `kobako_codec::msgpack::Value`.
`kobako::filesystem` re-exports `kobako_runtime::filesystem`, which names every type
a Filesystem's operations take, so each name there keeps the grade above.
`kobako::network` re-exports `kobako_runtime::network`, which re-exports
`OpenConnections` and `process_connections` from `net`.

### Fixed Pillar

Two things stay fixed, and the Ruby Frontend takes no Codec seam.

| Fixed | Why |
|---|---|
| the **Envelope** and the **ABI surface** | the same for every assembly, which makes the parts interchangeable at all |
| the Ruby Frontend's Codec, MessagePack | Ruby's native choice, which the gem speaks directly |

Both wire parts live in `kobako-transport`, which depends on nothing, so an
implementer picks up the fixed pillar without anyone else's choices
(→ [`wire/README.md`](../wire/README.md)).

## Payload Codec

A Codec owns the payload positions and nothing else. The Envelope routes and
attributes without reading a payload byte, so a new Codec leaves the Envelope, the
ABI, and the version untouched.

A guest fills the positions its Codec serves. A Rust host reaches every position,
because each has a byte-level entry, and the `msgpack` feature adds the bundled
Codec's spelling beside it.

| Position | Byte-level entry | `msgpack` spelling |
|---|---|---|
| dispatch arguments and answer | `Receiver` | `ValueReceiver`, `into_receiver` |
| `run` payload | `RunPayload::bytes`, `RunPayload::build` | `RunPayload::values` |
| yield | `Yielder::call_payload` | `Yielder::call_values` |
| invocation result | `Execution::payload` | `Execution::value` |
| object behind a Handle | `Handles::resolve`, `Execution::resolve` | `resolve_as` |

A Handle is resolved by id, since spelling a Handle is the Codec's job and reaching
the object is not. Each spelling is a thin wrapper over its entry, so another
Codec's overlay is written outside the SDK as extension traits over the same entries.

### Codec Obligations

A Codec fills positions, not an encoding. Writing a value is the floor every Codec
owes. Each other position it serves or refuses as unserved, so a missing feature
reads apart from a broken guest.

| Position | What serving it obliges | Why |
|---|---|---|
| Yield Reply ok body, Outcome ok body | one value — **the floor** | every Invocation answers an Outcome |
| Call payload, Run payload | positional and keyword arguments, distinguishably | `public_send` and an entrypoint do not interchange them |
| Yield Call | the arguments alone, as a plain list | a yield carries no keywords |

How an unserved position refuses, and why a Call owes its Reply, is specified in
[codec](../spec/behavior/codec.md). Keep keywords apart from the rest arguments: a
Codec folding them together makes `KV.get(key, limit: 9)` lose `limit:` with nothing
raising.

A Handle representation is optional. Without one, Handles ride only the Envelope's
`target` field: a guest still reaches a stateful receiver and only forgoes passing
Handles as arguments. A Reply's fault body is no Codec position either. A Fault is
kobako's own data and rides the Envelope, so a Codec neither encodes nor reads one.

### Naming the Codec

A guest shell names its Codec once, on the associated type.

```rust
impl MrbGuest for MyGuest {
    type Codec = MyCodec;              // implements PayloadCodec
    fn init_gems(mrb: &Mrb) -> Result<(), beni::Error> { Ok(()) }
}
```

A Rust host names its choice by what it builds against. Every `msgpack` spelling,
including the `Value` they speak, lives in the `msgpack` module rather than the crate
root. Turning the feature off removes a spelling rather than leaving a hole, and a
host with its own Codec then implements the byte-level surface. `run` takes
whichever payload it is handed, so the feature governs what a host is offered, never
what it can reach.

A type implementing two Codecs' receiver traits has two `into_receiver` in scope.
Name one (`ValueReceiver::into_receiver(kv)`): binding an object is choosing the
Codec the guest reaches it through.

### Codec-free Builds

Replaceability is a property of the dependency graph, not a flag. Each tier reaches
a Codec only where it asks for one.

| Crate | Codec it reaches |
|---|---|
| `kobako-transport`, `kobako-core` | none |
| `kobako-mruby` | `MsgpackCodec` only with its `msgpack` feature |
| `kobako-wasm` (shipped shell) | MessagePack, by asking for it |
| `kobako` (Rust SDK) | the `Value` surface, `msgpack` on by default |

A shell naming its own `MrbGuest::Codec` never asks for `msgpack`. The SDK defaults
to a Codec because an embedder names it directly; with the feature off it reaches no
payload Codec.

`crates:graph` and `wasm:graph` hold each tier to that table in the default gate, on
every push and pull request. A tier's tests are held to it too, since a library whose
tests still need a Codec has not moved it out of the code. A `[dev-dependencies]`
Codec is allowed, because no consumer installs it.

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
gem ---> dispatch (a) ---> Service
            ^                 |
            +--- yield (b) ---+
```

- (a) `dispatch(target, method, Option<Proc>, payload)`; the block parks for the Call.
- (b) A yield downcall re-enters while the `dispatch` frame is still on the stack.

That tier carries no payload Codec, so the gem still names its own. It encodes the
payload before calling and decodes the answer after, so a guest-side raise stays
clear of the parked block.

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

The Host App sets each flag for every Invocation, critical unless marked advisory.

```ruby
sandbox.flag(:strict_mode)                         # critical
sandbox.flag(:compile_cache, "on", advisory: true) # skipped where unrecognised
```

The Rust SDK spells these `flag(name, value)` and `flag_advisory(name, value)`;
an empty value is a plain switch.

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

`crates/kobako-runtime` is the Engine-free contract, and `crates/kobako-wasmtime` is
one Wasm Driver implementing it. The contract carries a Frontend through
`Sandbox::with_runtime`, so any Wasm Driver satisfying it carries the Rust SDK above it
unchanged.

| Contract part | Names |
|---|---|
| Engine seams | `Runtime`, `DispatchHandler`, `Yielder` |
| host ABI endpoint | `endpoint`, over a `GuestInstance` the Wasm Driver implements |
| files | `vfs`, with `File` answering what a guest may do with an open file |
| declared Posture | `Profile`, with `guest_kstat` for an entry and `clock_due` for a wait |
| per-invocation types | `Snapshot`, `Completion`, `Capture`, `Usage`, `Trap` |

The Rust SDK takes a Wasm Driver at `Sandbox::with_runtime` and keeps the whole tier
above it: Catalog, Handles, snippet replay, Extension composition. `Sandbox::new` is
the same path with the bundled `kobako_wasmtime::Driver`.

### Engine Encoding

A Wasm Driver encodes each ABI onto its Engine, and the runtime beneath it decides
every answer (→ [`isolation/README.md`](../isolation/README.md)).

| Imports a Guest Binary links | The Driver answers them through |
|---|---|
| preview1's descriptor calls, `fd_*`, `path_*`, `poll_oneoff` | `vfs` and a descriptor table |
| `wasi:sockets`, `wasi:http`, and the `wasi:io` they stand on | `net::Namespace` and a resource table |
| the kobako ABI's `__kobako_upcall` | `endpoint` |

### Engine Options

Only the Posture and each Invocation's Grants cross that seam. The Engine's own caps
are configured where the Wasm Driver is built
(→ [runtime](../spec/behavior/runtime.md)).

| Option | Left at its default |
|---|---|
| `Runtime::compile` | compiles nothing; the SDK replays the preloaded source |
| `Runtime::supports` | supports no Grant; the SDK refuses a Mount |
| `Runtime::profile` | required; the SDK checks the Posture it declares |

### Engine-free Builds

As with the Codec, the Engine's replaceability is a property of the dependency graph.

| `kobako` build | Way in |
|---|---|
| `wasmtime` feature on (default) | `Sandbox::new` or `Sandbox::with_runtime` |
| `wasmtime` feature off | `Sandbox::with_runtime` only; no Engine reached |

`crates:graph` holds the SDK to that table in the default gate, on every push and
pull request.

The Ruby Frontend takes no such seam. `Kobako::Runtime` is pinned to
`kobako_wasmtime::Driver`, so choosing an Engine there means choosing a different
Frontend.

## Filesystem

A Filesystem made from any `impl FileSystemType` plugs in behind a Mount, and an
Overlay stacks Filesystems as Layers. The Mount decides what a guest may do, so the
Filesystem behind it need not.

```text
FileSystemType --> Filesystem --> Mount (a) --> guest
                       |
                       +--> Layer (b) --> Overlay --> Mount
```

- (a) Placed at a guest path; it allows reading, or reading and writing.
- (b) The Overlay stacks Filesystems; it is mounted as one Filesystem.

What each method owes is in the rustdoc of `FileSystemType`, `InodeOperations`,
`FileOperations`, and `DentryOperations`, which an editor shows as you implement them.
