# Runtime

What the host checks before a guest runs, which posture it builds, and how invocations share a process.

### Startup checks

The runtime is what stands between a request and a guest that runs. Three checks happen before any guest does:

- The artifact states an ABI version the host implements.
- The requested isolation posture is one the ladder names.
- The posture actually built is not weaker than the one accepted.

### Scheduling modes

The scheduling scenarios pair off deliberately, because releasing the lock changes scheduling and nothing else. Each witness runs one scenario under both modes and compares them: a value, a dispatch, a nested dispatch, a capture. The host-parallel run is the one that shows what the mode is for.

### Isolation and determinism

Ambient denial is read at the WASI layer, not through guest code. The default Guest Binary gives scripts no surface to reach it from.

"Reaches nothing" cannot be witnessed whole, so it is held as what the host grants. No posture preopens a directory, sets a variable, or holds a socket. The guest exports only what the ABI names.

What the determinism buys is witnessed where it shows: two invocations beginning from the same interpreter state.

### Compile cache

Compiling an artifact is expensive enough to keep on disk, and a cache is a second way in. So each of its refusals is witnessed twice over: that construction still succeeds, and what the cache directory holds afterwards. A cache that quietly loaded a planted artifact would pass the first observation alone.

The refusals hold off other users. A Mount or a Service the host grants writes as the host's own user. No refusal tells such a write apart, so the host keeps every writable grant off the cache.

### Invocation results

What the runtime hands an invocation back is read here, not through the Sandbox that usually reads it. A binding that shifted a field's shape would still pass every Sandbox assertion about the derived value. This is also why the two channels are separated twice. One separation is at this seam; the other is a difference the two frontends must agree on.

### Empty artifact

An artifact can satisfy the whole invocation ABI while doing no guest work. That artifact makes the host's own per-invocation cost measurable as a total. That is a claim about the artifact, so it is held to both verbs and to the capture it leaves. These are the ways it could satisfy the loader without satisfying the ABI.

## Includes

- `test/unit/values/test_sandbox_options.rb`
- `test/e2e/test_threading.rb`
- `test/e2e/sandbox/test_sandbox*.rb`
- `test/e2e/sandbox/test_gvl*.rb`
- `test/e2e/runtime/*.rb`
- `test/e2e/sandbox/test_null_guest.rb`
- `test/fuzz/test_dispatch_scheduling_fuzz.rb`
- `test/parity/test_hermetic.rb`
- `test/e2e/test_mount_spelling.rb`
- `test/e2e/test_ruby_filesystem_stat.rb`
- `test/e2e/test_file_stat_ino.rb`
- `crates/kobako-runtime/src/security.rs`
- `crates/kobako-runtime/src/filesystem/pipe.rs`
- `crates/kobako-runtime/src/vfs/namei.rs`
- `crates/kobako-runtime/src/vfs/dentry.rs`
- `crates/kobako-runtime/src/vfs/file.rs`
- `crates/kobako-runtime/src/vfs/utimes.rs`
- `crates/kobako-runtime/src/wait.rs`
- `crates/kobako-runtime/src/net/tcp.rs`
- `crates/kobako-runtime/src/endpoint.rs`
- `crates/kobako-wasmtime/src/ambient.rs`
- `crates/kobako-wasmtime/src/descriptors.rs`
- `crates/kobako-wasmtime/src/preview1/testsuite.rs`
- `crates/kobako-wasmtime/src/cache.rs`
- `crates/kobako-wasmtime/src/abi.rs`
- `crates/kobako-wasmtime/src/wasi.rs`
- `crates/kobako-wasmtime/src/preview1/poll.rs`
- `crates/kobako-wasmtime/src/preview1/sock.rs`
- `crates/kobako-wasmtime/src/preview2/host.rs`
- `crates/kobako-wasmtime/src/preview1/fd.rs`
- `crates/kobako-wasmtime/src/preview1/path.rs`
- `crates/kobako-wasmtime/src/preview1/mount_view.rs`
- `crates/kobako-wasmtime/src/preview1/memory.rs`
- `crates/kobako-wasmtime/src/preview1/differential.rs`
- `crates/kobako-wasmtime/tests/integration/compile_posture.rs`
- `crates/kobako-wasmtime/src/exports.rs`
- `crates/kobako/tests/integration/concurrency.rs`
- `crates/kobako/tests/integration/runtime_injection.rs`
- `crates/kobako-runtime/src/profile.rs`

## `RT-001` Threads holding their own Sandboxes hold their own guest state

| Step | Statement |
| --- | --- |
| Given | two Threads, each with its own Sandbox |
| Given | one of them having set a guest global |
| When | the other Thread reads that global |
| Then | it reads nothing the first Thread set |

## `RT-002` Threads sharing one Sandbox still evaluate on their own identity

| Step | Statement |
| --- | --- |
| Given | several Threads evaluating on one shared Sandbox, each supplying its own identity for its invocation |
| When | each Thread's guest code resolves the identity it was given |
| Then | each resolves only its own |

## `RT-003` The entrypoint verb shares a Sandbox the same way

| Step | Statement |
| --- | --- |
| Given | several Threads running an entrypoint on one shared Sandbox, each supplying its own identity for its invocation |
| When | each Thread's entrypoint resolves the identity it was given |
| Then | each resolves only its own |

## `RT-004` Releasing the lock does not pool the Handles

| Step | Statement |
| --- | --- |
| Given | several Threads invoking their own Sandboxes constructed to release the lock |
| When | each Thread restores the Handles its own invocation minted |
| Then | each restores only its own |

## `RT-005` Releasing the lock does not cross the Handle arguments

| Step | Statement |
| --- | --- |
| Given | several Threads invoking their own Sandboxes constructed to release the lock |
| When | each Thread passes its own Handles back as dispatch arguments |
| Then | each resolves only its own |

## `RT-006` A guest that cannot state its ABI version does not run

| Step | Statement |
| --- | --- |
| Given | a Guest Binary exporting no ABI version |
| When | a Sandbox is constructed over it |
| Then | construction fails |

## `RT-007` A guest stating another ABI version does not run

| Step | Statement |
| --- | --- |
| Given | a Guest Binary reporting an ABI version the host does not implement |
| When | a Sandbox is constructed over it |
| Then | construction fails |

## `RT-008` A refused artifact is never remembered as usable

| Step | Statement |
| --- | --- |
| Given | a Guest Binary already refused once for its ABI version |
| When | a Sandbox is constructed over the same path again |
| Then | construction fails again |

## `RT-009` The strongest posture is the one nobody has to ask for

| Step | Statement |
| --- | --- |
| Given | Sandbox options naming no posture |
| When | the options are read |
| Then | the posture is the strongest rung |

## `RT-010` Both rungs of the ladder may be requested

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying each ladder rung in turn |
| When | the options are read |
| Then | each reports the rung it was given |

## `RT-011` A posture off the ladder is not a posture

| Step | Statement |
| --- | --- |
| Given | Sandbox options naming a posture the ladder does not |
| When | the options are built |
| Then | the posture is refused |

## `RT-012` A runtime that built less than was asked for does not run

| Step | Statement |
| --- | --- |
| Given | options requesting a rung |
| Given | a runtime declaring a rung below it |
| When | the request is enforced as a floor |
| Then | the declaration is refused |

## `RT-013` A posture nobody can place ranks below every request

| Step | Statement |
| --- | --- |
| Given | options requesting any rung |
| Given | a runtime declaring a posture the ladder does not name |
| When | the request is enforced as a floor |
| Then | the declaration is refused |

## `RT-014` A stronger posture satisfies a weaker request

| Step | Statement |
| --- | --- |
| Given | options requesting a rung |
| Given | a runtime declaring that rung or a stronger one |
| When | the request is enforced as a floor |
| Then | the declaration is accepted |

## `RT-015` The bundled runtime builds whichever rung is asked for

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a Sandbox is constructed at each ladder rung in turn |
| Then | each construction reports the rung it requested |

## `RT-016` The request reaches the runtime and comes back

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a runtime is built from that path at each ladder rung in turn |
| Then | each runtime declares the rung it was built for |

## `RT-017` The runtime refuses an unnameable posture on its own

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a runtime is built from that path with a posture the ladder does not name |
| Then | the posture is refused |

## `RT-018` Holding the lock is what happens when nobody chooses

| Step | Statement |
| --- | --- |
| Given | Sandbox options naming no scheduling mode |
| When | the options are read |
| Then | the mode is `:hold` |

## `RT-019` Both scheduling modes may be requested

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying each scheduling mode in turn |
| When | the options are read |
| Then | each reports the mode it was given |

## `RT-020` A mode outside the set is not a mode

| Step | Statement |
| --- | --- |
| Given | Sandbox options naming a scheduling mode the set does not |
| When | the options are built |
| Then | the mode is refused |

## `RT-021` The runtime refuses an unknown mode on its own

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a runtime is built from that path with a scheduling mode the set does not name |
| Then | the mode is refused |

## `RT-022` Releasing the lock changes no value

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode |
| When | each evaluates the same guest source |
| Then | both answer the same value |

## `RT-023` Releasing the lock changes no dispatch result

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode, each with the same Service bound |
| When | each evaluates guest source that dispatches to that Service |
| Then | both answer the same value |

## `RT-024` The lock is reacquired deep enough for a nested dispatch

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode, each with the same yielding Service bound |
| When | each evaluates guest source whose dispatch dispatches again |
| Then | both answer the same value |

## `RT-025` Releasing the lock changes no capture

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode |
| When | each evaluates guest source that writes to standard output |
| Then | both carry the same captured bytes |

## `RT-026` Released Sandboxes on distinct Threads each answer their own

| Step | Statement |
| --- | --- |
| Given | several Threads, each with its own Sandbox constructed to release the lock |
| When | every Thread evaluates guest source computing from its own input |
| Then | each Thread receives the result of its own input |

## `RT-027` A requested posture is honored the same way by either frontend

| Step | Statement |
| --- | --- |
| Given | a scenario requesting the hermetic posture explicitly |
| When | both frontends run it |
| Then | they observe the same result |

## `RT-028` A posture switch resolves the same way on either frontend

| Step | Statement |
| --- | --- |
| Given | a scenario switching to the permissive posture |
| When | both frontends run it |
| Then | they resolve it the same way |

## `RT-029` Randomly generated dispatch programs answer the same under either mode

| Step | Statement |
| --- | --- |
| Given | generated dispatch programs run on Sandboxes under each scheduling mode |
| When | each program runs under both |
| Then | the two modes answer identically |

## `RT-030` Generated programs keep each Thread's references to itself

| Step | Statement |
| --- | --- |
| Given | generated dispatch programs run concurrently on distinct released Sandboxes |
| When | each Thread resolves the references it minted |
| Then | each resolves only its own |

## `RT-031` Generated programs keep each invocation's identity to itself on a shared Sandbox

| Step | Statement |
| --- | --- |
| Given | generated dispatch programs run concurrently on one shared released Sandbox, each supplying its own identity |
| When | each Thread resolves the identity it was given |
| Then | each resolves only its own |

## `RT-032` An option value the runtime cannot build with

| Step | Statement |
| --- | --- |
| Given | a runtime path and a timeout that is not positive |
| When | a runtime is built from that path |
| Then | the refusal names the deadline's constraint |

## `RT-033` A posture the ladder does not name

| Step | Statement |
| --- | --- |
| Given | a Sandbox construction requesting a profile off the ladder |
| When | a Sandbox is constructed |
| Then | the option is refused |

## `RT-034` A keyword the options do not take

| Step | Statement |
| --- | --- |
| Given | a Sandbox construction carrying an unknown keyword |
| When | a Sandbox is constructed |
| Then | the option is refused |

## `RT-035` A corrupt cache entry does not stop a runtime being built

| Step | Statement |
| --- | --- |
| Given | a compiled-artifact cache entry holding bytes that are not an artifact |
| When | a runtime is built over the artifact that entry names |
| Then | it is built |

## `RT-036` The compile it falls back to replaces that entry

| Step | Statement |
| --- | --- |
| Given | a compiled-artifact cache entry holding bytes that are not an artifact |
| When | a runtime is built over the artifact that entry names |
| Then | the entry no longer holds those bytes |

## `RT-037` A cache directory others may write is not read from

| Step | Statement |
| --- | --- |
| Given | a cache entry in a directory writable beyond its owner |
| When | a runtime is built over the artifact that entry names |
| Then | it is built by compiling |

## `RT-038` A cache directory others may write is not written back into

| Step | Statement |
| --- | --- |
| Given | a cache entry in a directory writable beyond its owner |
| When | a runtime is built over the artifact that entry names |
| Then | the entry still holds what it held |

## `RT-039` Storing an artifact prunes what has gone unused

| Step | Statement |
| --- | --- |
| Given | a cache entry untouched for longer than the retention window |
| When | a runtime is built over an artifact the cache does not hold |
| Then | the untouched entry is gone |

## `RT-040` A completed invocation reports both channels, both marks, and its usage

| Step | Statement |
| --- | --- |
| Given | a runtime driven without a Sandbox around it |
| When | an invocation completes |
| Then | it answers two captures, two truncation marks, an elapsed time and a memory peak |

## `RT-041` A successful outcome carries a payload and no attribution record

| Step | Statement |
| --- | --- |
| Given | a runtime driven without a Sandbox around it |
| When | an invocation completes |
| Then | the outcome carries payload bytes and no record |

## `RT-042` The two channels are already apart at this seam

| Step | Statement |
| --- | --- |
| Given | a runtime driven without a Sandbox around it |
| When | an invocation writes different content to each channel |
| Then | each capture carries only what was written to its own |

## `RT-043` An invocation that wrote nothing carries empty captures

| Step | Statement |
| --- | --- |
| Given | a runtime driven without a Sandbox around it |
| When | an invocation writes to neither channel |
| Then | both captures are empty and neither mark is set |

## `RT-044` A guest that does no work still answers an evaluation

| Step | Statement |
| --- | --- |
| Given | a Sandbox over an artifact that satisfies the ABI and does no guest work |
| When | source is evaluated against it |
| Then | the invocation completes with no value |

## `RT-045` A guest that does no work still answers an entrypoint run carrying arguments

| Step | Statement |
| --- | --- |
| Given | a Sandbox over an artifact that satisfies the ABI and does no guest work |
| When | an entrypoint is run against it with positional and keyword arguments |
| Then | the invocation completes with no value |

## `RT-046` Doing no work leaves nothing captured

| Step | Statement |
| --- | --- |
| Given | a Sandbox over an artifact that satisfies the ABI and does no guest work |
| When | source is evaluated against it |
| Then | the capture is empty |

## `RT-047` The strongest posture reads no host time

| Step | Statement |
| --- | --- |
| Given | a guest running at the strongest posture |
| When | it reads the clock |
| Then | it reads the epoch rather than the host's time |

## `RT-048` The strongest posture reads no host entropy

| Step | Statement |
| --- | --- |
| Given | a guest running at the strongest posture |
| When | it reads entropy |
| Then | it reads a constant stream rather than the host's entropy |

## `RT-049` The weaker posture reads live host time

| Step | Statement |
| --- | --- |
| Given | a guest running at the weaker posture |
| When | it reads the clock |
| Then | it reads the host's time |

## `RT-050` The weaker posture reads host entropy

| Step | Statement |
| --- | --- |
| Given | a guest running at the weaker posture |
| When | it reads entropy |
| Then | it reads the host's entropy |

## `RT-051` The clock the strongest posture offers never advances

| Step | Statement |
| --- | --- |
| Given | the clock a guest at the strongest posture measures elapsed time with |
| When | it is read |
| Then | it stands still |

## `RT-052` An engine behind the contract drives the invocation

| Step | Statement |
| --- | --- |
| Given | a Sandbox built over an engine the Host App supplied |
| When | an invocation runs |
| Then | that engine is driven once for it |

## `RT-053` An engine behind the contract is handed the registrations the Sandbox sealed

| Step | Statement |
| --- | --- |
| Given | a Sandbox over a supplied engine that has bound a path |
| When | an invocation runs |
| Then | the engine is handed the sealed bindings |

## `RT-055` Threads sharing one Sandbox all reach the Services it bound

| Step | Statement |
| --- | --- |
| Given | several Threads invoking one shared Sandbox |
| Given | a Service bound on it before its first invocation |
| When | each Thread's guest code calls that Service at once |
| Then | every call answers |

## `RT-056` A guest stating the host's ABI version is accepted

| Step | Statement |
| --- | --- |
| Given | a Guest Binary other than the bundled one, reporting the ABI version the host implements |
| When | a Sandbox is constructed over it |
| Then | construction completes |

## `RT-057` Threads sharing one Sandbox each receive their own result

| Step | Statement |
| --- | --- |
| Given | several Threads evaluating on one shared Sandbox, each with its own input |
| When | every Thread evaluates guest source computing from its input |
| Then | each Thread receives the result of its own input |

## `RT-058` A cap that is not a positive quantity of its kind is refused

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying, for each cap in turn, zero, a negative, or a value of the wrong kind |
| When | the options are built |
| Then | the cap is refused |

## `RT-059` An artifact that is not there does not run

| Step | Statement |
| --- | --- |
| Given | no Guest Binary at the path a Sandbox names |
| When | a Sandbox is constructed over it |
| Then | it fails as a construction failure saying the artifact has not been built |

## `RT-060` An artifact that is not a Wasm module is a construction failure

| Step | Statement |
| --- | --- |
| Given | a file at the path whose bytes are not a Wasm module |
| When | a runtime is built from that path |
| Then | construction fails, and not as the artifact that has not been built |

## `RT-061` A deadline that never arrives is refused

| Step | Statement |
| --- | --- |
| Given | a deadline that is not finite |
| When | Sandbox options are built with it |
| Then | they are refused |

## `RT-062` Releasing the lock changes no failure

| Step | Statement |
| --- | --- |
| Given | guest code that raises |
| When | it is evaluated once under each lock mode |
| Then | both fail with the same class, guest class and message |

## `RT-063` An artifact that cannot become a runtime fails construction

| Step | Statement |
| --- | --- |
| Given | an artifact that cannot be read, one the engine cannot link, and one it cannot instantiate |
| When | a Sandbox is built over each |
| Then | each fails as a construction failure at the step it cannot pass |

## `RT-064` The host links nothing beyond the import the ABI names

| Step | Statement |
| --- | --- |
| Given | an artifact importing one host function besides the one the ABI names |
| When | a Sandbox is built over it at either posture |
| Then | the import is unknown and construction fails |

## `RT-065` Invocations at once each capture only their own output

| Step | Statement |
| --- | --- |
| Given | several Threads each writing a marker of its own to both channels |
| When | they invoke at once, on one shared Sandbox or on one each |
| Then | each capture holds only its own invocation's marker |

## `RT-066` A shared Service is not serialized

| Step | Statement |
| --- | --- |
| Given | a Sandbox shared across Threads with one Service bound |
| When | two invocations call it at once |
| Then | both are inside the Service at the same time |

## `RT-067` No posture grants an ambient filesystem, environment, or socket

| Attribute | Value |
| --- | --- |
| relates | `RT-047`, `RT-049` |

| Step | Statement |
| --- | --- |
| Given | a guest running at either posture |
| When | it asks for a preopened directory, its environment, or a connection |
| Then | it finds no directory, no variable, and no socket |

## `RT-068` The Guest Binary exports the ABI's functions and nothing else

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | its exports are read |
| Then | it exports exactly the functions the ABI names |

## `RT-069` Released Sandboxes on distinct Threads run in parallel

| Attribute | Value |
| --- | --- |
| unverifiable | parallelism shows only as wall-clock time, which a shared CI host does not hold steady |

| Step | Statement |
| --- | --- |
| Given | several Threads, each with its own Sandbox constructed to release the lock |
| When | every Thread evaluates guest source at once |
| Then | the guest code runs in parallel rather than one Thread at a time |

## `RT-070` A Sandbox holding the lock runs no other Thread meanwhile

| Attribute | Value |
| --- | --- |
| unverifiable | a Service boundary switches Threads, so the other Thread's progress cannot be told apart from the guest's |

| Step | Statement |
| --- | --- |
| Given | a Sandbox constructed to hold the lock |
| Given | another Thread ready to run |
| When | the Sandbox evaluates guest source |
| Then | the other Thread makes no progress until the guest code finishes, except while the guest waits on the host |

## `RT-071` A reference from an earlier invocation resolves to nothing under either mode

| Attribute | Value |
| --- | --- |
| unverifiable | every invocation begins in a fresh guest, so no guest code can present an earlier reference |

| Step | Statement |
| --- | --- |
| Given | a Sandbox under each scheduling mode |
| Given | a reference an earlier invocation of that Sandbox issued |
| When | guest code in a later invocation presents it |
| Then | it resolves to no object |

## `RT-072` Compiling the Snippets runs at the strongest posture whatever the request

| Attribute | Value |
| --- | --- |
| relates | `RT-047`, `RT-048`, `RT-067`, `S-189` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox requesting the weaker posture |
| When | the guest compiles its Snippets |
| Then | it reads the epoch and a constant stream rather than the host's time and entropy |

## `RT-073` A wait on the strongest posture's clock ends at once

| Attribute | Value |
| --- | --- |
| relates | `RT-051` |

| Step | Statement |
| --- | --- |
| Given | a guest running at the strongest posture |
| When | it waits ten seconds on a clock |
| Then | the wait returns without the host waiting |

## `RT-074` A wait on the weaker posture's clock ends at the deadline

| Attribute | Value |
| --- | --- |
| relates | `RT-049`, `S-010` |

| Step | Statement |
| --- | --- |
| Given | a guest running at the weaker posture under a deadline |
| When | it waits on a clock past that deadline |
| Then | the invocation ends as a deadline reached, at the deadline |

## `RT-075` A Mount appears to the guest as a directory at its path

| Attribute | Value |
| --- | --- |
| relates | `RT-067` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount at `/data` |
| When | the guest asks its first preopened directory for its name |
| Then | it reads `/data` |

## `RT-076` A guest reads a file beneath a Mount

| Attribute | Value |
| --- | --- |
| relates | `RT-075`, `FS-001` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a Tmpfs holding a file |
| When | the guest opens and reads that file beneath the Mount's path |
| Then | it reads the file's content |

## `RT-077` A guest writes a file beneath a read-write Mount

| Attribute | Value |
| --- | --- |
| relates | `RT-075`, `FS-002` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a Tmpfs |
| When | the guest creates and writes a file beneath the Mount's path |
| Then | the Tmpfs holds that file with what was written |

## `RT-078` A change beneath a read-only Mount is refused as a read-only filesystem

| Attribute | Value |
| --- | --- |
| relates | `RT-077` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-only Mount |
| When | the guest opens a file beneath it for writing |
| Then | the open answers that the filesystem is read-only |

## `RT-079` A path climbing above a Mount's root is refused

| Attribute | Value |
| --- | --- |
| relates | `RT-075`, `FS-007` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the guest opens a path that climbs above the Mount's root |
| Then | the open answers that the guest lacks the capability |

## `RT-080` A Policy refusing an open answers that access is denied

| Attribute | Value |
| --- | --- |
| relates | `RT-076` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount whose Policy refuses a path |
| When | the guest opens that path |
| Then | the open answers that access is denied |

## `RT-081` A Policy cannot widen a Mount's access

| Attribute | Value |
| --- | --- |
| refines | `RT-078` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-only Mount whose Policy allows writing |
| When | the guest creates and writes a file beneath it |
| Then | the call answers EROFS, and the Filesystem holds nothing |

## `RT-082` Every invocation reaches the Filesystem its Mount holds

| Attribute | Value |
| --- | --- |
| relates | `MT-009` |

| Step | Statement |
| --- | --- |
| Given | a Mount over a Tmpfs an earlier invocation wrote a file to |
| When | the next invocation's guest reads that file |
| Then | it reads what the earlier invocation wrote |

## `RT-083` Removing a Mount's root is refused as invalid

| Attribute | Value |
| --- | --- |
| relates | `RT-079` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the guest removes the directory `.` beneath it |
| Then | the removal is refused as invalid, and the Filesystem is not asked |

## `RT-084` Renaming a Mount's root is refused as busy

| Attribute | Value |
| --- | --- |
| relates | `RT-083` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the guest renames `.` beneath it, or renames a file onto `.` |
| Then | each rename is refused as busy, and the Filesystem is not asked |

## `RT-085` Unlinking a Mount's root answers that it is a directory

| Attribute | Value |
| --- | --- |
| relates | `RT-083` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the guest unlinks the file `.` beneath it |
| Then | the unlink answers that `.` is a directory, and the Filesystem is not asked |

## `RT-086` An empty path names nothing beneath a Mount

| Attribute | Value |
| --- | --- |
| relates | `RT-079` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the guest opens the empty path beneath it |
| Then | the open answers that nothing is there |

## `RT-087` A `..` after a missing name names nothing

| Attribute | Value |
| --- | --- |
| relates | `RT-079` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding `file.txt` |
| When | the guest opens `missing/../file.txt` beneath it |
| Then | the open answers that nothing is there |

## `RT-088` A `..` after a file is not a directory

| Attribute | Value |
| --- | --- |
| relates | `RT-087` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding `file.txt` |
| When | the guest opens `file.txt/..` beneath it |
| Then | the open answers that `file.txt` is not a directory |

## `RT-089` A path ending in a slash names only a directory

| Attribute | Value |
| --- | --- |
| relates | `RT-088` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding `file.txt` |
| When | the guest opens `file.txt/` beneath it |
| Then | the open answers that `file.txt` is not a directory |

## `RT-090` A file created at a path ending in a slash is refused as a directory

| Attribute | Value |
| --- | --- |
| refines | `RT-089` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the guest creates a file at `new.txt/` beneath it |
| Then | the creation answers that the path is a directory, and no file is made |

## `RT-091` An open may ask for synchronized writes

| Attribute | Value |
| --- | --- |
| relates | `RT-076` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding `file.txt` |
| When | the guest opens `file.txt` asking each write to be synchronized |
| Then | the open succeeds, and each write landing bytes syncs the file once |

## `RT-092` An open of a directory answers its other flags as Linux does

| Attribute | Value |
| --- | --- |
| relates | `RT-091` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the guest opens a directory while asking to create, or opens a file as one |
| Then | the first answers EINVAL, and the second ENOTDIR, and nothing is made |

## `RT-093` A rename of a file onto a path ending in a slash is not a directory

| Attribute | Value |
| --- | --- |
| refines | `RT-089` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding `file.txt` |
| When | the guest renames `file.txt` onto `file.txt/` |
| Then | the rename answers that `file.txt` is not a directory |

## `RT-094` A directory beneath a Mount with a Policy does not move

| Attribute | Value |
| --- | --- |
| refines | `RT-080` |

| Step | Statement |
| --- | --- |
| Given | a Mount with a Policy, holding a directory with a file in it |
| When | the guest renames that directory |
| Then | the rename answers that it crosses devices, and nothing moves |

## `RT-095` An append lands after another writer's append

| Attribute | Value |
| --- | --- |
| refines | `FS-036` |

| Step | Statement |
| --- | --- |
| Given | a file another writer appends to as the guest writes |
| When | the guest writes to that file opened to append |
| Then | the guest's bytes land after the other writer's |

## `RT-096` A call past its hostcall fuel answers NOMEM

| Attribute | Value |
| --- | --- |
| relates | `RT-076` |

| Step | Statement |
| --- | --- |
| Given | an invocation calling preview1 with an iovec array or a buffer past 128 MiB |
| When | the host would copy it |
| Then | the call answers NOMEM before anything is copied |

## `RT-097` A stat following no link reports the link itself

| Attribute | Value |
| --- | --- |
| refines | `FS-048` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount whose host directory holds a link |
| When | the guest stats the link, following it and then following none |
| Then | the first answers a regular file, and the second a symbolic link |

## `RT-098` A stat of a link named with a trailing slash reports its directory

| Attribute | Value |
| --- | --- |
| refines | `RT-097` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a link to a directory |
| When | the guest stats the link with a trailing slash, following no link |
| Then | the stat answers a directory |

## `RT-099` An allocation grows a file to where its room ends and shortens none

| Attribute | Value |
| --- | --- |
| relates | `RT-077` |

| Step | Statement |
| --- | --- |
| Given | a read-write Mount of a Tmpfs, or of a Bind on Linux, holding a file |
| When | the guest opens the file to write and asks to allocate room in it |
| Then | a shorter file grows with zeros to where the room ends, and a longer one stays as it was |

## `RT-100` An open following no link refuses a link to a directory

| Attribute | Value |
| --- | --- |
| refines | `FS-047` |
| relates | `RT-098` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a link to a directory |
| When | the guest opens the link following no link, then again with a trailing slash |
| Then | the first answers ELOOP, and the second opens the directory |

## `RT-101` A Policy judges a path through a link where the link leads

| Attribute | Value |
| --- | --- |
| refines | `RT-081` |
| relates | `RT-094`, `RT-100`, `FS-047` |

| Step | Statement |
| --- | --- |
| Given | a Bind Mount with a Policy, holding a link to a directory |
| When | the guest opens or removes a path through the link |
| Then | the Policy judges the path in that directory, and a refused path answers EACCES and changes nothing |

## `RT-102` A path ending in a dot is neither removed nor moved

| Attribute | Value |
| --- | --- |
| relates | `RT-083` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding the directory `dir` and the file `file.txt` |
| When | the guest removes and renames `dir/.`, `dir/..`, and `file.txt/..` |
| Then | removing `dir/.` answers EINVAL and `dir/..` ENOTEMPTY, the renames EBUSY, except ENOTDIR through the file, and `dir` stays |

## `RT-103` A create of what already stands changes nothing

| Attribute | Value |
| --- | --- |
| refines | `RT-078` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-only Mount holding a file and a directory |
| When | the guest opens each with `O_CREAT` and no write |
| Then | the file opens, and the directory answers EISDIR |

## `RT-104` A seek past the largest offset is invalid

| Attribute | Value |
| --- | --- |
| relates | `RT-076` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the guest seeks the open file past the largest signed 64-bit offset |
| Then | the seek answers EINVAL, as Linux does |

## `RT-105` A file moved beneath itself is no directory

| Attribute | Value |
| --- | --- |
| relates | `FS-061` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding `file.txt` |
| When | the guest renames `file.txt` to `file.txt/x` |
| Then | the rename answers ENOTDIR, and `file.txt` stays |

## `RT-106` An open following no link finds a dangling link as itself

| Attribute | Value |
| --- | --- |
| refines | `RT-100` |
| relates | `FS-052` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a link to nothing |
| When | the guest opens the link following no link, then again with `O_CREAT` and `O_EXCL` |
| Then | the first answers ELOOP, the second EEXIST, and nothing is made where it points |

## `RT-107` A listing goes on over the entries it began with

| Attribute | Value |
| --- | --- |
| relates | `RT-075` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount whose directory a guest began to list, and a file then made in it |
| When | the guest lists on from where it stopped, then lists again from the start |
| Then | only the listing from the start holds the new file |

## `RT-108` A cached artifact is read only by the engine that could have compiled it

| Attribute | Value |
| --- | --- |
| relates | `RT-035` |

| Step | Statement |
| --- | --- |
| Given | an artifact compiled and cached by an engine of another release or another configuration |
| When | a runtime is built over the same Guest Binary |
| Then | it compiles the Guest Binary again rather than reading that artifact |

## `RT-109` A path ending in a dot makes nothing where no directory stands

| Attribute | Value |
| --- | --- |
| relates | `RT-102` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount with nothing named `missing` |
| When | the guest makes the directory `missing/.` and opens `missing/.` with `O_CREAT` |
| Then | both answer ENOENT and nothing named `missing` is made |

## `RT-110` A cache directory reached through a link is neither read nor written

| Attribute | Value |
| --- | --- |
| relates | `RT-037`, `RT-038` |

| Step | Statement |
| --- | --- |
| Given | a cache entry in a private directory the cache directory only links to |
| When | a runtime is built over the artifact that entry names |
| Then | it is built by compiling and the entry still holds what it held |

## `RT-111` A guest that never finishes being set up ends at the deadline

| Attribute | Value |
| --- | --- |
| relates | `OC-032`, `S-128` |

| Step | Statement |
| --- | --- |
| Given | a Guest Binary whose allocator never returns |
| When | it runs an invocation under a deadline |
| Then | the invocation ends as the deadline's own trap |

## `RT-112` A Guest Binary that never finishes instantiating fails construction

| Attribute | Value |
| --- | --- |
| refines | `RT-063` |

| Step | Statement |
| --- | --- |
| Given | a Guest Binary whose start function never returns |
| When | a Sandbox is built over it under a setup deadline |
| Then | it fails as a construction failure once that deadline passes |

## `RT-113` Two stores of one artifact never write the same file

| Attribute | Value |
| --- | --- |
| relates | `RT-035`, `RT-039` |

| Step | Statement |
| --- | --- |
| Given | two writers in one process storing the same compiled artifact |
| When | each takes a temporary file to write into |
| Then | each holds a file of its own |

## `RT-114` Moving a descriptor onto an open file replaces that file

| Attribute | Value |
| --- | --- |
| relates | `FL-010` |

| Step | Statement |
| --- | --- |
| Given | a guest holding two open files, the second on a Filesystem that refuses to close it |
| When | the guest renumbers the first onto the second |
| Then | the call succeeds, the first now standing at the second's number, as Linux's `dup2` |

## `RT-115` A path climbing above the directory it is opened beneath is refused

| Attribute | Value |
| --- | --- |
| refines | `RT-079` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount's root |
| When | it opens a path through that directory that climbs above it |
| Then | the open answers ENOTCAPABLE |

## `RT-116` An offset past the largest signed offset is invalid

| Attribute | Value |
| --- | --- |
| relates | `RT-104` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the guest reads, writes, or sizes the file past the largest signed 64-bit offset |
| Then | the call answers EINVAL, whichever Filesystem holds the file |

## `RT-117` A directory made in a removed directory's place is not reached through it

| Attribute | Value |
| --- | --- |
| relates | `RT-115` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount |
| When | the directory is removed and another is made at its path |
| Then | a lookup through the descriptor answers ENOENT |

## `RT-118` A directory moved back to its path is reached through its descriptor again

| Attribute | Value |
| --- | --- |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount |
| When | the directory moves away and back to its path |
| Then | a call through the descriptor reaches the directory |

## `RT-119` A step out of a link lands where it leads

| Attribute | Value |
| --- | --- |
| refines | `RT-087` |
| relates | `RT-101`, `RT-109` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a link to a directory |
| When | the guest opens or stats a path with `..` or a final `.` after it |
| Then | the call answers what the link's target or the directory above it holds there |

## `RT-120` A Policy judges a path as its Filesystem stores the names

| Attribute | Value |
| --- | --- |
| refines | `RT-081` |
| relates | `FS-081` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Policy refuses one spelling, over a Filesystem storing that spelling |
| When | the guest opens the path in another spelling the Filesystem reaches |
| Then | the open answers EACCES |

## `RT-121` A directory descriptor holds its directory while open

| Attribute | Value |
| --- | --- |
| refines | `RT-150` |
| relates | `FS-086` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Policy, on a Filesystem reusing inode numbers |
| When | the directory is removed and another made at its path |
| Then | a lookup through the descriptor answers ENOENT |

## `RT-122` A path holding a NUL is invalid

| Attribute | Value |
| --- | --- |
| relates | `RT-086` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding `file.txt` |
| When | the guest opens `file.txt` followed by a NUL beneath it |
| Then | the open answers EINVAL |

## `RT-123` A link named with a trailing slash is neither removed nor moved

| Attribute | Value |
| --- | --- |
| refines | `RT-089` |
| relates | `RT-098`, `RT-101` |

| Step | Statement |
| --- | --- |
| Given | a read-write Bind Mount, with or without a Policy, holding a link to a directory |
| When | the guest unlinks or renames the link, named with a trailing slash |
| Then | each answers ENOTDIR, and the link stays |

## `RT-125` A Policy leaves what a link out of a Bind or into a loop answers

| Attribute | Value |
| --- | --- |
| refines | `RT-101` |

| Step | Statement |
| --- | --- |
| Given | a Bind Mount with a Policy, holding links out of it and into a loop |
| When | the guest opens a path through each link, then unlinks each link |
| Then | an open out of the Bind answers NOTCAPABLE, one into a loop ELOOP, and each unlink removes the link |

## `RT-126` A lookup that fails fails the call beneath a Policy

| Attribute | Value |
| --- | --- |
| refines | `RT-101` |

| Step | Statement |
| --- | --- |
| Given | a Mount with a Policy whose Filesystem fails to look up a directory |
| When | the guest opens a file beneath that directory |
| Then | the open answers that failure, and no file opens |

## `RT-127` A guest sees a FIFO as of an unknown type and cannot open it

| Attribute | Value |
| --- | --- |
| refines | `FS-090` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a FIFO |
| When | the guest stats the FIFO and opens it to read |
| Then | the stat answers an unknown filetype named once, and the open answers EACCES |

## `RT-128` A directory a guest opens carries no right to write

| Attribute | Value |
| --- | --- |
| relates | `RT-092` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding a directory |
| When | the guest opens the directory and reads its descriptor's rights |
| Then | the rights hold no `fd_write`, as wasmtime-wasi answers |

## `RT-129` A descriptor may ask not to block

| Attribute | Value |
| --- | --- |
| relates | `RT-091` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding `file.txt` |
| When | the guest opens `file.txt` not to block, or sets an open one not to |
| Then | each call succeeds and the descriptor reports it, as Linux takes it on a file |

## `RT-130` An advice reaching past the largest signed offset is invalid

| Attribute | Value |
| --- | --- |
| refines | `RT-116` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest advises on a length past the largest offset, or from past it |
| Then | each advice answers EINVAL, as Linux does |

## `RT-131` A lone wait on a CPU-time clock is invalid

| Attribute | Value |
| --- | --- |
| relates | `RT-073` |

| Step | Statement |
| --- | --- |
| Given | an invocation |
| When | the guest waits on the process or thread CPU-time clock alone |
| Then | each wait answers EINVAL, as wasmtime-wasi answers it |

## `RT-132` A cached artifact is on disk before it takes its name

| Attribute | Value |
| --- | --- |
| relates | `RT-035`, `RT-039` |
| unverifiable | only a host losing power between the rename and its writeback shows it |

| Step | Statement |
| --- | --- |
| Given | a runtime built over an artifact the cache does not hold |
| When | the host stops before the cache directory is written back |
| Then | the cache holds the whole artifact under its name, or none |

## `RT-133` A write ending past the largest signed offset is invalid

| Attribute | Value |
| --- | --- |
| refines | `RT-116` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest writes bytes that would end past the largest signed 64-bit offset |
| Then | the write answers EINVAL as Linux does, and the file stays as it was |

## `RT-134` An unlink with a trailing slash answers for the name itself

| Attribute | Value |
| --- | --- |
| refines | `RT-089` |
| relates | `RT-123` |

| Step | Statement |
| --- | --- |
| Given | a read-write Bind Mount holding a directory, a file, and a link to nothing |
| When | the guest unlinks each, and a missing name, named with a trailing slash |
| Then | the directory answers EISDIR, the missing name ENOENT, the rest ENOTDIR, and all stay |

## `RT-135` A wait to write a file is ready whatever its Mount permits

| Attribute | Value |
| --- | --- |
| relates | `RT-073` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding files open only to read beneath a read-write and a read-only Mount |
| When | the guest waits to write to each |
| Then | each is ready, as Linux answers for a file with no poll of its own |

## `RT-136` A read whose end passes the largest signed offset is invalid

| Attribute | Value |
| --- | --- |
| refines | `RT-116` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest reads past the largest signed 64-bit offset, by offset and by its position |
| Then | each read answers EINVAL as Linux does, and a read ending at it answers the end |

## `RT-137` Sizing a file opened only to read answers by its Mount, then as Linux does

| Attribute | Value |
| --- | --- |
| relates | `RT-135` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding files open only to read beneath a read-only and a read-write Mount |
| When | the guest sets the size of each |
| Then | each answers EINVAL, as Linux ftruncate does |

## `RT-138` An argument preview1 cannot hold is refused in wasmtime-wasi's order

| Attribute | Value |
| --- | --- |
| relates | `RT-096` |

| Step | Statement |
| --- | --- |
| Given | an invocation calling preview1 on a descriptor it does not hold |
| When | it passes an undefined flag, a buffer outside its memory, or a path past the call's fuel |
| Then | the flag traps, the buffer answers EBADF, and the path ENOMEM, as wasmtime-wasi answers each |

## `RT-139` A Mount whose Policy decides may be written

| Attribute | Value |
| --- | --- |
| refines | `RT-135` |
| relates | `RT-137`, `MT-025` |

| Step | Statement |
| --- | --- |
| Given | a read-write Mount whose Policy allows every change |
| When | it waits to write a file open to read, and sizes one open to write |
| Then | the wait is ready and the file is sized, the Policy unasked by either |

## `RT-140` A link leading out of an opened directory escapes it

| Attribute | Value |
| --- | --- |
| refines | `RT-115` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount without a Policy |
| When | it opens or stats a path through that directory whose link leads out of it |
| Then | the call answers ENOTCAPABLE, as `RESOLVE_BENEATH` answers a link and a `..` alike |

## `RT-141` A link staying within an opened directory is followed

| Attribute | Value |
| --- | --- |
| relates | `RT-115`, `RT-140` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount without a Policy |
| When | it opens a path through that directory whose links stay within it |
| Then | it reaches the file the links lead to |

## `RT-142` A create through a dangling link beneath an opened directory makes its target

| Attribute | Value |
| --- | --- |
| refines | `RT-140` |
| relates | `RT-106` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened, which holds a link pointing at nothing |
| When | it opens the link's path to create a file, following the link |
| Then | the file stands where the link points |

## `RT-143` A link leading out of a Mount escapes it

| Attribute | Value |
| --- | --- |
| relates | `FS-007`, `RT-140` |

| Step | Statement |
| --- | --- |
| Given | a Bind Mount holding a link to a file outside its host directory |
| When | the guest opens the link's path from the Mount's root |
| Then | the open answers ENOTCAPABLE, as an absolute path does |

## `RT-144` A file's access is checked before a buffer outside memory is bounded

| Attribute | Value |
| --- | --- |
| refines | `RT-138` |

| Step | Statement |
| --- | --- |
| Given | a file opened without the right to read, and one without the right to write |
| When | the guest reads or writes each with a buffer outside its memory |
| Then | each call answers EBADF, as Linux reads the file's mode before the buffer |

## `RT-145` A link swapped in after a Policy judges a path is not followed

| Attribute | Value |
| --- | --- |
| refines | `RT-101` |
| relates | `FS-099` |

| Step | Statement |
| --- | --- |
| Given | a Bind Mount whose Policy allows one directory and refuses another |
| When | another writer swaps the allowed directory for a link as the Policy judges |
| Then | the read reaches the directory the walk held, and nothing the Policy refused |

## `RT-146` An open that may create asks to make the file before it finds what stands

| Attribute | Value |
| --- | --- |
| refines | `MT-025` |
| relates | `MT-007` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Policy records each access it is asked |
| When | the guest opens a missing path and an existing file to create, without the right to write |
| Then | the Policy is asked a change for each, then a read for the file, as Linux's `lookup_open` asks `may_o_create` first |

## `RT-147` An open of a missing path that makes nothing answers ENOENT

| Attribute | Value |
| --- | --- |
| refines | `RT-078` |
| relates | `RT-146` |

| Step | Statement |
| --- | --- |
| Given | a read-only Mount, and a Mount whose Policy records each access it is asked |
| When | the guest opens a missing path to write, without asking to create it |
| Then | each open answers ENOENT, and the Policy was asked nothing |

## `RT-148` What stands at a path answers before a read-only Mount refuses

| Attribute | Value |
| --- | --- |
| refines | `RT-078` |
| relates | `RT-147` |

| Step | Statement |
| --- | --- |
| Given | a read-only Mount holding a file and a directory |
| When | the guest opens the file with `CREAT` and `EXCL`, and the directory to write |
| Then | the first open answers EEXIST, and the second EISDIR |

## `RT-149` A directory moved while opened is reached through its descriptor

| Attribute | Value |
| --- | --- |
| relates | `RT-117`, `FS-105` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount without a Policy |
| When | the directory moves away and another is made at its path |
| Then | a lookup through the descriptor reaches the moved directory |

## `RT-150` A directory moved while opened beneath a Policy walks no path

| Attribute | Value |
| --- | --- |
| refines | `RT-149` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount with a Policy |
| When | the directory moves away outside the runtime, and another is made at its path |
| Then | a lookup through the descriptor answers ENOENT |

## `RT-151` A directory without an identity is not reached again beneath a Policy

| Attribute | Value |
| --- | --- |
| refines | `RT-150` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount with a Policy |
| When | the Filesystem reports no identity for the directory, and a lookup goes through it |
| Then | the lookup answers ENOENT |

## `RT-152` An Overlay directory takes one descriptor however many Layers it spans

| Attribute | Value |
| --- | --- |
| relates | `S-201` |

| Step | Statement |
| --- | --- |
| Given | an invocation with room for two descriptors, handed an Overlay whose two Layers hold a directory |
| When | the guest opens the directory, then two files |
| Then | the directory and the first file open, and the second answers EMFILE |

## `RT-154` A link loop answers ELOOP

| Attribute | Value |
| --- | --- |
| relates | `RT-141` |

| Step | Statement |
| --- | --- |
| Given | a directory opened beneath a Mount, holding a link pointing at itself |
| When | the guest opens the link, following it |
| Then | the open answers ELOOP |

## `RT-155` A directory renamed onto its own path beneath a Policy stays

| Attribute | Value |
| --- | --- |
| relates | `RT-094` |

| Step | Statement |
| --- | --- |
| Given | a Bind Mount with a Policy, holding a directory beneath its root |
| When | the guest renames that directory onto its own path |
| Then | the rename succeeds and changes nothing |

## `RT-156` The weaker posture sees the times a Filesystem keeps

| Attribute | Value |
| --- | --- |
| relates | `RT-049` |

| Step | Statement |
| --- | --- |
| Given | an invocation on the weaker posture, with a Mount over a Tmpfs |
| When | the guest writes a file and reads what the path names |
| Then | its modification time is no earlier than the write |

## `RT-157` The strongest posture sees every time at the Unix epoch

| Attribute | Value |
| --- | --- |
| refines | `RT-047` |

| Step | Statement |
| --- | --- |
| Given | an invocation on the strongest posture, with a Mount over a Tmpfs |
| When | the guest writes a file and reads what the path and the open file name |
| Then | each time reads the Unix epoch |

## `RT-158` A time another posture wrote reaches the strongest posture as the epoch

| Attribute | Value |
| --- | --- |
| refines | `RT-157` |

| Step | Statement |
| --- | --- |
| Given | one Tmpfs mounted for an invocation on each posture |
| When | the weaker writes a file and the strongest reads what the path names |
| Then | each time the strongest reads is the Unix epoch |

## `RT-159` A directory removed through the runtime makes nothing more, whatever its Filesystem

| Attribute | Value |
| --- | --- |
| refines | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | a directory opened beneath a Mount, over a Filesystem answering each call by its path |
| When | it is removed, another made in its place, and a make goes through it |
| Then | the make and a listing through it answer not found, and nothing is made |

## `RT-160` A directory removed beneath one Mount is removed beneath another of its instance

| Attribute | Value |
| --- | --- |
| refines | `RT-159` |

| Step | Statement |
| --- | --- |
| Given | one Filesystem instance mounted twice, a directory opened beneath the first |
| When | the directory is removed beneath the second, then made into through the first |
| Then | the make answers not found |

## `RT-161` A path ending in a dot through a link is neither removed nor moved

| Attribute | Value |
| --- | --- |
| refines | `RT-102` |

| Step | Statement |
| --- | --- |
| Given | a read-write Bind Mount holding a directory and a link to it |
| When | the guest removes and renames the link's path followed by `.` and by `..` |
| Then | removing with `.` answers EINVAL and with `..` ENOTEMPTY, the renames EBUSY, and the directory stays |

## `RT-162` A create through a trailing slash answers EISDIR before following a link

| Attribute | Value |
| --- | --- |
| refines | `RT-103` |

| Step | Statement |
| --- | --- |
| Given | a read-write Bind Mount holding a link to nothing |
| When | the guest opens the link's name with a trailing slash and `O_CREAT` |
| Then | the open answers EISDIR, and nothing is made |

## `RT-163` A directory made while another is removed in its place stays alive

| Attribute | Value |
| --- | --- |
| refines | `RT-159` |

| Step | Statement |
| --- | --- |
| Given | a directory being removed through the runtime by one thread |
| When | another thread makes one in its place, opens it, and makes into it |
| Then | the make succeeds in the directory made in its place |

## `RT-164` A directory moved beneath itself is refused whatever its Filesystem

| Attribute | Value |
| --- | --- |
| relates | `FS-107` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem that checks nothing about where a directory moves |
| When | a directory is moved through the runtime into a directory beneath it |
| Then | the move answers EINVAL, and the Filesystem is not asked |

## `RT-165` A Filesystem changing the directory its own change holds answers a deadlock

| Attribute | Value |
| --- | --- |
| relates | `RT-163` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose make calls the runtime to make in the same directory |
| When | a directory is made through the runtime |
| Then | the inner make answers EDEADLK |

## `RT-166` A call waiting for a directory's lock waits through the registered Blocking

| Attribute | Value |
| --- | --- |
| relates | `FS-039` |

| Step | Statement |
| --- | --- |
| Given | a Blocking registered, one thread changing a directory |
| When | another thread makes a call on the same directory |
| Then | the call waits through that Blocking, then answers |

## `RT-167` A create opens the file it finds, or makes one

| Attribute | Value |
| --- | --- |
| refines | `RT-103` |

| Step | Statement |
| --- | --- |
| Given | a file the guest opens with `O_CREAT` |
| When | another thread removes the file while the open is judged |
| Then | the open succeeds |

## `RT-168` A file renamed onto itself through a link stays

| Attribute | Value |
| --- | --- |
| refines | `RT-155` |

| Step | Statement |
| --- | --- |
| Given | a Bind Mount holding a file, and a link to the directory holding it |
| When | the guest renames the file through the link onto its own path |
| Then | the rename succeeds and changes nothing |

## `RT-169` Every spelling of a name holds its one dentry

| Attribute | Value |
| --- | --- |
| refines | `RT-120` |
| relates | `RT-094` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem storing `DIR/F`, and reaching each name by any case |
| When | `DIR/F` is removed while a move of `dir/f` is under way |
| Then | the removal waits for the move, then answers not found |

## `RT-170` A directory moved beneath another Mount of its instance is reached beneath a Policy

| Attribute | Value |
| --- | --- |
| refines | `RT-150` |
| relates | `RT-160`, `RT-149` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount with a Policy |
| When | the directory moves beneath another Mount of the same Filesystem instance |
| Then | a lookup through the descriptor reaches it, as the Policy allows the path it moved to |

## `RT-171` A host reporting no directory owner caches no artifact on disk

| Attribute | Value |
| --- | --- |
| relates | `RT-037`, `RT-038` |

| Step | Statement |
| --- | --- |
| Given | a host that reports no directory's owner, as Windows |
| When | a runtime is built over a Guest Binary |
| Then | it is built by compiling, and no compiled artifact is written to disk |

## `RT-172` A path asked while directories above it move names one that stood

| Attribute | Value |
| --- | --- |
| refines | `RT-149` |
| relates | `RF-024` |

| Step | Statement |
| --- | --- |
| Given | a directory whose ancestors move again and again through the runtime |
| When | its path is asked at the same time |
| Then | each path answered is one the directory stood at |

## `RT-173` A link reports no size to the guest

| Attribute | Value |
| --- | --- |
| relates | `FS-048` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Filesystem reports a link with the size of where it points |
| When | the guest looks the link up without following it |
| Then | the link reports a size of zero |

## `RT-174` The runtime opens only a file or a directory, whatever its Filesystem

| Attribute | Value |
| --- | --- |
| relates | `FS-090` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Filesystem holds an entry that is neither a file, a directory, nor a link |
| When | the guest opens the entry |
| Then | the open answers EACCES |

## `RT-175` The runtime answers for an operation a Filesystem lacks

| Attribute | Value |
| --- | --- |
| relates | `RT-174` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Filesystem has no operation to make, remove, or move a name |
| When | the guest makes a directory, makes a file, removes a directory, removes a file, and renames a file |
| Then | the calls answer EPERM, EACCES, EPERM, EPERM, and EPERM |

## `RT-176` The runtime refuses a sync of a file without one

| Attribute | Value |
| --- | --- |
| relates | `RT-175` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a file whose Filesystem has no operation to sync it |
| When | the guest syncs the file |
| Then | the call answers EINVAL |

## `RT-177` A standard stream answers as a pipe does

| Attribute | Value |
| --- | --- |
| relates | `RT-176` |

| Step | Statement |
| --- | --- |
| Given | a guest holding its standard streams |
| When | the guest syncs one, sizes one, and calls about the position of each |
| Then | the first two answer EINVAL, and the calls about a position answer ESPIPE |

## `RT-178` A standard stream takes no offset

| Attribute | Value |
| --- | --- |
| relates | `RT-177` |

| Step | Statement |
| --- | --- |
| Given | a guest holding its standard input and output |
| When | the guest reads at an offset from each, and writes at an offset to each |
| Then | each call answers ESPIPE |

## `RT-179` A directory a move replaces makes nothing more

| Attribute | Value |
| --- | --- |
| refines | `RT-159` |

| Step | Statement |
| --- | --- |
| Given | a guest holding an empty directory, one thread moving another directory onto it |
| When | another thread makes an entry through the held directory |
| Then | the make waits for the move and answers ENOENT, and the move succeeds |

## `RT-180` An open file answers for itself whichever Wasm Driver asks

| Attribute | Value |
| --- | --- |
| refines | `RT-116` |
| relates | `RT-133` |

| Step | Statement |
| --- | --- |
| Given | a file opened beneath a Mount, with no Wasm Driver in front of it |
| When | a caller reads or writes it against its mode, or past the largest offset |
| Then | the first answers EBADF and the second EINVAL, and the Filesystem is not asked |

## `RT-181` An allocation refuses in Linux's order

| Attribute | Value |
| --- | --- |
| relates | `RT-180` |

| Step | Statement |
| --- | --- |
| Given | a file opened to read, a file opened to write, and a pipe's write end |
| When | a caller allocates no bytes on the first, then some on each |
| Then | the calls answer EINVAL, EBADF, ENOTSUP, and ESPIPE |
## `RT-182` A listing hands `.` and `..` first and goes on where it stopped

| Attribute | Value |
| --- | --- |
| relates | `RT-107`, `RT-180` |

| Step | Statement |
| --- | --- |
| Given | a directory opened through the runtime, with no Wasm Driver in front of it |
| When | a caller lists it, stops early, and lists it again from where it stopped |
| Then | `.` and `..` come first, and each entry carries its own position |

## `RT-183` A guest tells entries apart by inode number where its posture allows

| Attribute | Value |
| --- | --- |
| relates | `RT-182` |

| Step | Statement |
| --- | --- |
| Given | a permissive guest and a hermetic guest, each holding a Mount of a file |
| When | each stats the file and lists its directory |
| Then | the permissive guest reads one hash in both, and the hermetic guest reads 0 |

## `RT-184` A directory answers as one open to read does

| Attribute | Value |
| --- | --- |
| relates | `RT-180` |

| Step | Statement |
| --- | --- |
| Given | a directory opened beneath a Mount |
| When | a caller reads, writes, sizes, allocates, syncs, advises on it, or asks its position |
| Then | it answers EISDIR, EBADF, EINVAL, EBADF, success, success, and EBADF |

## `RT-185` A call starting from anything but a directory is not a directory

| Attribute | Value |
| --- | --- |
| relates | `RT-184` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest makes a path call starting from that descriptor |
| Then | the call answers ENOTDIR, as Linux refuses the walk |

## `RT-186` Setting times asks the Mount, then the Policy, then the entry

| Attribute | Value |
| --- | --- |
| relates | `RT-175` |

| Step | Statement |
| --- | --- |
| Given | an entry beneath a Mount, through a path or a descriptor alike |
| When | a caller sets its times |
| Then | a read-only Mount answers EROFS, a Policy EACCES, and an entry lacking `setattr` ENOTSUP |

## `RT-187` A call asking no time succeeds

| Attribute | Value |
| --- | --- |
| refines | `RT-186` |

| Step | Statement |
| --- | --- |
| Given | an entry beneath a read-only Mount |
| When | a caller sets its times, naming none |
| Then | the call succeeds, as `utimensat` returns before it looks at anything |

## `RT-188` A link no Filesystem makes is answered by what stands

| Attribute | Value |
| --- | --- |
| relates | `RT-175` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Filesystem makes no links |
| When | a guest links a missing name, or links to a taken or a free name |
| Then | the calls answer ENOENT, EEXIST, and EPERM, and nothing is made |

## `RT-189` A new descriptor takes the lowest closed number

| Attribute | Value |
| --- | --- |
| relates | `RT-075` |

| Step | Statement |
| --- | --- |
| Given | a guest that closed two descriptors, and holds a higher one |
| When | it opens three files |
| Then | the first two take the closed numbers, lowest first, and the third a fresh one |

## `RT-190` A full descriptor table answers EMFILE whatever the path names

| Attribute | Value |
| --- | --- |
| relates | `S-200` |

| Step | Statement |
| --- | --- |
| Given | a guest holding as many descriptors as it may |
| When | it opens a path that names nothing, or one no call can take |
| Then | the open answers EMFILE, as Linux reserves a descriptor first |

## `RT-191` The host ABI endpoint needs nothing of an Engine

| Attribute | Value |
| --- | --- |
| relates | `RT-180` |

| Step | Statement |
| --- | --- |
| Given | a guest instance made of a plain buffer, with no Engine behind it |
| When | the endpoint writes a request and answers an upcall through it |
| Then | the request lands in its memory, and the Reply is written back there |

## `RT-192` Only what kobako refuses fails wasi-testsuite

| Attribute | Value |
| --- | --- |
| relates | `RT-186`, `RT-188` |

| Step | Statement |
| --- | --- |
| Given | wasi-testsuite's prebuilt preview1 tests, each over a Bind of its own directory |
| When | each runs through the preview1 imports |
| Then | only the tests needing a link made fail |

## `RT-193` An entry whose Filesystem keeps no count of names is named once

| Attribute | Value |
| --- | --- |
| relates | `RT-127` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem reporting no count of names for an entry |
| When | a guest on either posture reads what the entry reports |
| Then | it reads one name |

## `RT-194` A permissive guest's inode number names no number its host keeps

| Attribute | Value |
| --- | --- |
| refines | `RT-183` |

| Step | Statement |
| --- | --- |
| Given | a permissive guest and an entry whose Filesystem names its device and inode numbers |
| When | the guest reads the entry's inode number twice |
| Then | it reads one number, and that number is no hash computed without the process's key |

## `RT-195` A guest sees a special file's kind as preview1 names it

| Attribute | Value |
| --- | --- |
| relates | `RT-127`, `FS-144` |

| Step | Statement |
| --- | --- |
| Given | an entry of each kind a Filesystem reports |
| When | a preview1 Wasm Driver encodes each kind |
| Then | a socket reads as a stream socket, each device as itself, and a FIFO as unknown |

## `RT-196` A socket a guest sees cannot be opened

| Attribute | Value |
| --- | --- |
| refines | `FS-090` |
| relates | `RT-127` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a socket |
| When | the guest stats the socket and opens it to read |
| Then | the stat answers a stream socket, and the open answers EACCES |

## `RT-197` Only a file or a directory opens, whatever a Filesystem allows

| Attribute | Value |
| --- | --- |
| refines | `FS-090` |

| Step | Statement |
| --- | --- |
| Given | the rules every invocation holds to |
| When | they judge an open of each kind a Filesystem reports |
| Then | a file and a directory pass, and each other kind answers permission denied |

## `RT-198` A pipe of the runtime's own reports a FIFO

| Attribute | Value |
| --- | --- |
| relates | `RT-195` |

| Step | Statement |
| --- | --- |
| Given | a pipe the runtime made |
| When | a caller asks what its end reports |
| Then | it reports a FIFO |

## `RT-199` A dentry goes however deep it stands

| Attribute | Value |
| --- | --- |
| relates | `FS-145` |

| Step | Statement |
| --- | --- |
| Given | a dentry held far beneath the root, as moves can stack the directories above it |
| When | nothing holds it any more, on a thread with a small stack |
| Then | it goes, and the thread still runs |

## `RT-200` An open refuses a link before its Filesystem is asked

| Attribute | Value |
| --- | --- |
| refines | `RT-100` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose link would open if asked |
| When | the guest opens the link to read, following no link |
| Then | the open answers ELOOP, and the Filesystem is not asked |

## `RT-201` A time set through a descriptor is judged where it stands

| Attribute | Value |
| --- | --- |
| refines | `RT-186` |

| Step | Statement |
| --- | --- |
| Given | a guest beneath a Policy, holding a file whose name is gone |
| When | it sets a time on the file through its descriptor |
| Then | the call answers ENOENT, and the Policy is not asked about the old path |

## `RT-202` A link to an absolute path is not made

| Attribute | Value |
| --- | --- |
| relates | `RT-143` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem that makes any link it is asked to |
| When | the guest makes one to an absolute path, then to a relative one |
| Then | the first answers EPERM as wasi-testsuite requires, and the second is made |

## `RT-203` A path past the limit is made where no Policy needs it

| Attribute | Value |
| --- | --- |
| relates | `RT-199` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory that moved past `PATH_MAX` from the root, beneath no Policy |
| When | it makes an entry there |
| Then | the entry is made, as Linux sets no limit on where a directory stands |

## `RT-204` A path a Policy cannot be handed is too long

| Attribute | Value |
| --- | --- |
| relates | `RT-203` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory that moved past `PATH_MAX` from the root, beneath a Policy |
| When | it makes an entry there |
| Then | the call answers ENAMETOOLONG, and the Policy is not asked about that path |

## `RT-205` A read-only Mount answers what stands in Linux's order

| Attribute | Value |
| --- | --- |
| refines | `RT-078` |

| Step | Statement |
| --- | --- |
| Given | a read-only Mount |
| When | the guest makes what stands, or removes what does not |
| Then | the first answers EEXIST, and the second EROFS |

## `RT-206` A Policy is asked only once a call holds what stands

| Attribute | Value |
| --- | --- |
| relates | `RT-205` |

| Step | Statement |
| --- | --- |
| Given | a read-write Mount whose Policy refuses everything |
| When | the guest makes what stands, or removes what does not |
| Then | the first answers EEXIST, and the second ENOENT |

## `RT-207` A move across Mounts answers ahead of a dot at its end

| Attribute | Value |
| --- | --- |
| relates | `RT-205` |

| Step | Statement |
| --- | --- |
| Given | two Mounts |
| When | the guest moves `.` of one to a name in the other |
| Then | the move answers EXDEV, and not EBUSY |

## `RT-208` A readlink asks its hook only of a link

| Attribute | Value |
| --- | --- |
| relates | `RT-205` |

| Step | Statement |
| --- | --- |
| Given | a Mount holding a file and a link |
| When | the guest reads the link of what is missing, of the file, and of the link |
| Then | the calls answer ENOENT, EINVAL, and ENOTSUP |

## `RT-209` A move between two names of one entry changes nothing

| Attribute | Value |
| --- | --- |
| relates | `RT-205` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem holding one entry under two names |
| When | the guest moves one name onto the other |
| Then | the move succeeds, and the Filesystem is not asked |

## `RT-210` A directory opened with flags reports them

| Attribute | Value |
| --- | --- |
| relates | `RT-184` |

| Step | Statement |
| --- | --- |
| Given | a directory |
| When | the guest opens it asking not to wait and to sync |
| Then | the open file reports both flags |

## `RT-211` A move acts on what stands at its names once it holds them

| Attribute | Value |
| --- | --- |
| relates | `RT-078`, `RT-120` |

| Step | Statement |
| --- | --- |
| Given | a Mount with a Policy, and a move whose source another move takes away after the walk |
| When | the first move holds both directories |
| Then | it answers ENOENT, and nothing else moves |

## `RT-212` A listing refuses a name no directory can hold

| Attribute | Value |
| --- | --- |
| relates | `RT-182` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem listing a name that is empty or holds a `/` |
| When | the guest lists the directory |
| Then | the call answers EIO, and hands over no such name |

## `RT-213` A path no call can take is refused before a Filesystem is asked

| Attribute | Value |
| --- | --- |
| refines | `RT-122` |

| Step | Statement |
| --- | --- |
| Given | a call on a path or a link target holding a NUL, empty, or past `PATH_MAX` |
| When | it reaches the VFS, whichever Wasm Driver brought it |
| Then | it answers EINVAL, ENOENT, or ENAMETOOLONG, and the Filesystem is not asked |

## `RT-215` An open empties what stood through `setattr` once it is open

| Attribute | Value |
| --- | --- |
| relates | `RT-200` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem holding a file, and a name nothing stands at |
| When | the guest opens each with `O_CREAT` and `O_TRUNC` |
| Then | the first is opened, then asked `setattr` marked as from an open; the second is made and asked neither |

## `RT-216` A name found standing stays for the next walk unless its Filesystem lets it go

| Attribute | Value |
| --- | --- |
| relates | `RT-145` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose names stand, with nothing holding one between two calls |
| When | both calls name it |
| Then | it is looked up once, and twice where `DentryOperations::delete` lets it go |

## `RT-217` A read of a pipe into no room reads nothing

| Attribute | Value |
| --- | --- |
| refines | `RT-177` |

| Step | Statement |
| --- | --- |
| Given | a pipe still holding bytes |
| When | a read gives it no room |
| Then | it reads nothing and leaves the bytes for the next read |

## `RT-218` An entry a Filesystem cannot open opens with no operations

| Attribute | Value |
| --- | --- |
| refines | `RT-175` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Filesystem has no `open` for a file it holds |
| When | the guest opens the file and reads it |
| Then | the open succeeds and the read answers EINVAL, as Linux's opens with no `f_op->open` |

## `RT-219` A change naming a dot through a link answers as Linux's

| Attribute | Value |
| --- | --- |
| relates | `RT-119`, `RT-161` |

| Step | Statement |
| --- | --- |
| Given | a read-write Bind Mount holding a directory and a link to it |
| When | the guest makes a directory and removes a file at the link's path followed by `.` |
| Then | making answers EEXIST and removing EISDIR, and the directory stays |

## `RT-220` An exclusive create follows no link at the end

| Attribute | Value |
| --- | --- |
| relates | `RT-162` |

| Step | Statement |
| --- | --- |
| Given | a Mount holding a link to nothing |
| When | the guest opens the link's path to create it exclusively, asking to follow links |
| Then | the open answers EEXIST and nothing is made |

## `RT-221` A trailing slash on a link to a directory reads no link

| Attribute | Value |
| --- | --- |
| relates | `RT-208` |

| Step | Statement |
| --- | --- |
| Given | a Mount holding a link to a directory |
| When | the link is read at its path followed by `/` |
| Then | the call answers EINVAL, as Linux's follows the link to the directory first |

## `RT-222` A write of nothing leaves a file where it stands

| Attribute | Value |
| --- | --- |
| relates | `RT-180` |

| Step | Statement |
| --- | --- |
| Given | a file opened to append, holding bytes, positioned at its start |
| When | a call writes nothing to it |
| Then | its position stays at its start |

## `RT-223` A change asks a Filesystem once for the name it makes

| Attribute | Value |
| --- | --- |
| relates | `RT-216` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem counting its lookups, holding no entry at a name |
| When | a call makes a directory or file there, or renames onto it |
| Then | the Filesystem is asked about that name once |

## `RT-224` A link to what has no name left answers ENOENT

| Attribute | Value |
| --- | --- |
| relates | `RT-188` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem reporting no names for an entry a walk just found |
| When | a call gives the entry another name |
| Then | the call answers ENOENT and the Filesystem is asked to make nothing |

## `RT-225` An open no flags can mean answers before where it starts

| Attribute | Value |
| --- | --- |
| refines | `RT-185` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest opens from it asking to create a directory |
| Then | the call answers EINVAL |

## `RT-226` A descriptor moved beneath a Policy still reports itself

| Attribute | Value |
| --- | --- |
| relates | `RT-150` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount with a Policy |
| When | the directory moves away outside the runtime, and the guest stats the descriptor |
| Then | the call succeeds, as a Policy judges no stat |

## `RT-227` A descriptor moved beneath a Policy still lists itself

| Attribute | Value |
| --- | --- |
| relates | `RT-150` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount with a Policy |
| When | the directory moves away outside the runtime, and the guest lists the descriptor |
| Then | the call succeeds, as a Policy judges no listing |

## `RT-228` A directory moving through the runtime is never taken for removed

| Attribute | Value |
| --- | --- |
| refines | `RT-170` |

| Step | Statement |
| --- | --- |
| Given | a directory another thread keeps moving through the runtime |
| When | the runtime asks whether it was removed |
| Then | the answer is no |

## `RT-229` A walk to a name no lookup answered holds the directory to find it

| Attribute | Value |
| --- | --- |
| relates | `RT-165` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose make calls the runtime to walk through the same directory to a name no lookup answered |
| When | a directory is made through the runtime |
| Then | the inner walk answers EDEADLK |

## `RT-230` An empty path answers before where it starts

| Attribute | Value |
| --- | --- |
| refines | `RT-185` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest makes a call on an empty path starting from it |
| Then | the call answers ENOENT |

## `RT-231` An overlong path answers before where it starts

| Attribute | Value |
| --- | --- |
| refines | `RT-185` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest makes a call on a path past `PATH_MAX` starting from it |
| Then | the call answers ENAMETOOLONG |

## `RT-232` A path call setting no time checks no path

| Attribute | Value |
| --- | --- |
| relates | `RT-185` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest sets no time on a path starting from it |
| Then | the call succeeds, as `utimensat` returns first |

## `RT-233` A directory without an identity still reports itself beneath a Policy

| Attribute | Value |
| --- | --- |
| relates | `RT-151`, `RT-226` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount with a Policy |
| When | the Filesystem reports no identity for it, and the guest stats the descriptor |
| Then | the call succeeds |

## `RT-234` A directory moved while a call starts from it stays reachable beneath a Policy

| Attribute | Value |
| --- | --- |
| refines | `RT-150` |

| Step | Statement |
| --- | --- |
| Given | a guest beneath a Policy, holding a directory another Mount of its Filesystem moves |
| When | a call starts from the directory as the move lands |
| Then | that call and each later one reach the directory where it went |

## `RT-235` A directory opened for neither reading nor writing refuses a read

| Attribute | Value |
| --- | --- |
| relates | `RT-210` |

| Step | Statement |
| --- | --- |
| Given | a directory the guest opened without the right to read or write |
| When | the guest reads the descriptor |
| Then | the call answers EBADF |

## `RT-236` A change to an entry holds its lock

| Attribute | Value |
| --- | --- |
| relates | `RT-165`, `RT-186` |

| Step | Statement |
| --- | --- |
| Given | a thread holding the lock of an entry |
| When | the thread sets the entry's times |
| Then | the call answers EDEADLK |

## `RT-237` A listing that cannot be held within a guest's memory limit answers ENOMEM

| Attribute | Value |
| --- | --- |
| refines | `RT-107` |

| Step | Statement |
| --- | --- |
| Given | a guest with a memory limit, its descriptors on a Bind holding listings up to that limit |
| When | the guest begins another listing, to read it whole or part way |
| Then | the call answers ENOMEM before it hands any entry, and closing a descriptor gives back what it held |

## `RT-238` An allocation past a Tmpfs's size answers ENOSPC

| Attribute | Value |
| --- | --- |
| refines | `RT-099` |

| Step | Statement |
| --- | --- |
| Given | a read-write Mount of a Tmpfs |
| When | the guest asks to allocate more room than that Tmpfs may hold |
| Then | the allocation answers ENOSPC |

## `RT-239` A Bind answers ENOTSUP to an allocation where its host cannot grow in one step

| Attribute | Value |
| --- | --- |
| refines | `RT-099` |

| Step | Statement |
| --- | --- |
| Given | a read-write Bind Mount on a host other than Linux, holding a file |
| When | the guest opens the file to write and asks to allocate room in it |
| Then | the allocation answers ENOTSUP, and the file holds what it held |

## `RT-240` A `..` a move landed before answers EAGAIN

| Attribute | Value |
| --- | --- |
| refines | `RT-115` |

| Step | Statement |
| --- | --- |
| Given | a walk standing in a directory beneath a Mount |
| When | a move lands during the walk, and the path goes on with `..` |
| Then | the call answers EAGAIN, and made again reaches where the directory stands now |

## `RT-241` A walk a move carried out from under its start answers ENOTCAPABLE

| Attribute | Value |
| --- | --- |
| refines | `RT-115` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount's root |
| When | a move carries the directory a walk through it stands in out from under it |
| Then | the call answers ENOTCAPABLE |

## `RT-242` Each call asks a Filesystem's permission where Linux's VFS does

| Step | Statement |
| --- | --- |
| Given | a Filesystem answering `permission` |
| When | a call walks a path, makes or removes a name, or opens an entry |
| Then | it asks of each directory walked, of the directory changed, and of the entry opened |

## `RT-243` A change a Filesystem's permission refuses changes nothing

| Attribute | Value |
| --- | --- |
| refines | `RT-242` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `permission` refuses a change to a directory |
| When | a call makes or removes a name in that directory |
| Then | the call answers EACCES, and the directory holds what it held |

## `RT-244` A Filesystem with `create` alone opens what it made

| Attribute | Value |
| --- | --- |
| relates | `RT-175` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem with `create` and no `atomic_open` |
| When | a guest opens a name where nothing stands, asking to create it |
| Then | the Filesystem makes the file, and the open reaches what it recorded |

## `RT-245` A name nothing stands at stays for the next walk unless its Filesystem lets it go

| Attribute | Value |
| --- | --- |
| refines | `RT-216` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem holding nothing at a name, with nothing holding the name between two calls |
| When | both calls name it |
| Then | it is looked up once and asked whether it holds, and looked up twice where `DentryOperations::delete` lets it go |

## `RT-246` Two names a Filesystem hands one inode for share it

| Step | Statement |
| --- | --- |
| Given | a Filesystem handing the same file inode for two names |
| When | a call opens each name |
| Then | both dentries record that one inode |

## `RT-247` A Filesystem's root stands for what `fill_super` made

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `fill_super` makes a root with an identity, and one making a root that names none |
| When | each is made into an instance |
| Then | the first root records that identity, and the second none |

## `RT-248` A file answering `llseek` itself decides where its seek lands

| Attribute | Value |
| --- | --- |
| relates | `RT-116` |

| Step | Statement |
| --- | --- |
| Given | a file whose Filesystem answers `FileOperations::llseek` |
| When | a call seeks it |
| Then | the call answers where that seek lands, and the file stands there |

## `RT-249` Each call reaches the security hook Linux names for it

| Step | Statement |
| --- | --- |
| Given | a security module recording the hooks it is asked |
| When | calls make, open, truncate, move, and remove names |
| Then | each call reaches the hook Linux names for what it does, a truncate through an open file included |

## `RT-250` What a security module refuses of a walk refuses the walk

| Attribute | Value |
| --- | --- |
| relates | `RT-242` |

| Step | Statement |
| --- | --- |
| Given | a security module refusing to pass through a directory |
| When | a call walks through that directory |
| Then | the call answers EACCES |

## `RT-251` A write syncs only as far as its file was opened to

| Step | Statement |
| --- | --- |
| Given | files opened with `O_DSYNC`, with `O_SYNC`, and with neither |
| When | each is written, and a caller asks to sync data alone, then everything |
| Then | `O_DSYNC` and the data sync reach the file as data alone, and `O_SYNC` and the full sync as everything |

## `RT-252` One call takes at most the most one read or write takes

| Step | Statement |
| --- | --- |
| Given | a file opened to read and write, and a buffer past 2 GiB less a page |
| When | a call reads or writes that buffer |
| Then | the file is handed 2 GiB less a page, as a short count the caller reads on from |

## `RT-253` A writer waiting for a directory goes ahead of readers arriving after it

| Attribute | Value |
| --- | --- |
| relates | `RT-166` |

| Step | Statement |
| --- | --- |
| Given | a thread holding a directory shared, and a writer waiting for it |
| When | another thread asks for it shared, and the first asks for it shared again |
| Then | the other waits until the writer finished, and the first takes it at once, never hanging on itself |

## `RT-254` An instance `fill_super` cannot set up answers why

| Attribute | Value |
| --- | --- |
| relates | `RT-247` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `fill_super` answers an error, and one that makes no root |
| When | each is made into an instance |
| Then | no instance is made, the first answers its error, and the second answers EINVAL |

## `RT-255` A security module is asked what a Filesystem's permission allowed

| Attribute | Value |
| --- | --- |
| refines | `RT-242` |

| Step | Statement |
| --- | --- |
| Given | a security module recording the changes it is asked about, over a Filesystem with `permission` |
| When | a call makes a name the Filesystem allows, and one it refuses |
| Then | the module is asked about the first alone, and the second answers EACCES |

## `RT-256` A guest's path call a move landed in is made again a few times

| Attribute | Value |
| --- | --- |
| refines | `RT-240` |

| Step | Statement |
| --- | --- |
| Given | a guest's path call whose walk answers EAGAIN |
| When | the Wasm Driver handles the call |
| Then | it makes the call again, four times in all at most, and answers EAGAIN only once they are used up |

## `RT-257` An open no flags can mean answers before the descriptor it names

| Attribute | Value |
| --- | --- |
| refines | `RT-225` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding nothing at a descriptor number |
| When | the guest opens from that number asking to create a directory |
| Then | the call answers EINVAL |

## `RT-258` An empty path answers before the descriptor it names

| Attribute | Value |
| --- | --- |
| refines | `RT-230` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding nothing at a descriptor number |
| When | the guest makes a non-readlink call on an empty path from that number |
| Then | the call answers ENOENT |

## `RT-259` A full descriptor table answers after the open's flags

| Attribute | Value |
| --- | --- |
| refines | `RT-190` |

| Step | Statement |
| --- | --- |
| Given | a guest holding as many descriptors as it may |
| When | it opens asking to create a directory |
| Then | the open answers EINVAL, as Linux reads the flags before it reserves a descriptor |

## `RT-260` A special file opened to write beneath a read-only Mount answers as its security module does

| Attribute | Value |
| --- | --- |
| refines | `RT-196` |

| Step | Statement |
| --- | --- |
| Given | a read-only Mount whose Filesystem holds a FIFO |
| When | the guest opens the FIFO to write |
| Then | the open answers EACCES, as Linux takes no write access to a mount for a special file |

## `RT-261` A truncating open beneath a read-only Mount answers EROFS first

| Attribute | Value |
| --- | --- |
| refines | `RT-215` |

| Step | Statement |
| --- | --- |
| Given | a read-only Mount whose Filesystem refuses every write to a file |
| When | the guest opens the file to read with `O_TRUNC` |
| Then | the open answers EROFS, as Linux asks the mount before `may_open` |

## `RT-262` An open asks to open what it just made

| Attribute | Value |
| --- | --- |
| refines | `RT-244` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem refusing to open what it makes |
| When | the guest opens a missing path to create it |
| Then | the open answers EACCES and the file stands, as Linux's `do_open` asks `may_open` of a file it made |

## `RT-263` A directory moving to another parent is asked to change

| Attribute | Value |
| --- | --- |
| refines | `RT-242` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem answering `permission` |
| When | a call moves a directory into another directory |
| Then | the directory itself is asked whether it may change, as Linux's `vfs_rename` asks of its `..` |

## `RT-264` Walks reaching one name at once ask its Filesystem once

| Attribute | Value |
| --- | --- |
| relates | `RT-229` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose lookup of a name is under way for one walk |
| When | another walk reaches the same name |
| Then | it waits for that lookup's answer, and the Filesystem is asked once, as Linux's `d_alloc_parallel` has it wait |

## `RT-265` A walk begun during a move begins once the move lands

| Attribute | Value |
| --- | --- |
| refines | `RT-240` |

| Step | Statement |
| --- | --- |
| Given | a move under way through the runtime |
| When | a walk begins |
| Then | it begins once the move lands, so no `..` it takes answers EAGAIN for that move |

## `RT-266` A start moved while it is asked beneath a Policy answers EAGAIN

| Attribute | Value |
| --- | --- |
| refines | `RT-234` |
| relates | `RT-256` |

| Step | Statement |
| --- | --- |
| Given | a directory a guest holds beneath a Mount with a Policy |
| Given | another Mount moving it while the runtime asks whether it stands |
| When | the runtime asks whether it stands |
| Then | it answers EAGAIN for the Wasm Driver to make the call again |

## `RT-267` A wait ends once its run is to stop

| Attribute | Value |
| --- | --- |
| relates | `RT-166` |

| Step | Statement |
| --- | --- |
| Given | a call waiting on a directory or a name another thread holds |
| When | its run's deadline passes, or its thread is interrupted |
| Then | the call answers EINTR while the other still holds, as Linux's killable waits do |

## `RT-268` A link pointing nowhere leaves the walk where it stands

| Attribute | Value |
| --- | --- |
| refines | `RT-119` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem holding a link whose target is empty |
| When | a call follows the link, at the end of a path or in its middle |
| Then | the walk goes on from the directory holding the link, as Linux's `pick_link` takes it as a pure jump |

## `RT-269` A listing hands over what came before a name no directory can hold

| Attribute | Value |
| --- | --- |
| refines | `RT-212` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem listing a plain name and then one that holds a `/` |
| When | the guest lists the directory from its start |
| Then | the call hands over the names before it, and only a call going on from there answers EIO |

## `RT-270` An allocation or advice on a FIFO answers ESPIPE

| Attribute | Value |
| --- | --- |
| refines | `RT-099` |

| Step | Statement |
| --- | --- |
| Given | a FIFO open to write, whose Filesystem opens it like any file |
| When | a caller allocates for it or advises on it |
| Then | each call answers ESPIPE, as Linux's `vfs_fallocate` and `generic_fadvise` read what the file is |

## `RT-271` A call asking no time succeeds whatever descriptor it names

| Attribute | Value |
| --- | --- |
| refines | `RT-187` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding nothing at a descriptor number |
| When | the guest sets no time through that number, or on a path starting from it |
| Then | each call succeeds, as `utimensat` returns before it takes the descriptor |

## `RT-272` A truncate follows what the Policy allowed as the file opened

| Attribute | Value |
| --- | --- |
| refines | `RT-139` |

| Step | Statement |
| --- | --- |
| Given | a file opened to write beneath a Mount whose Policy allowed it |
| Given | the file moved since to where the Policy refuses changes |
| When | the guest truncates the file through its descriptor |
| Then | the truncate lands, as Landlock keeps on the file what it allowed at the open |

## `RT-273` A file its Filesystem opens as a stream takes no position

| Attribute | Value |
| --- | --- |
| relates | `RT-178` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose open marks the file it opens as a stream |
| When | the guest seeks the file, and reads it at an offset |
| Then | each call answers ESPIPE, as Linux's `stream_open` takes the position away |

## `RT-274` An allocation on a device answers ENODEV

| Attribute | Value |
| --- | --- |
| refines | `RT-099` |
| relates | `RT-270` |

| Step | Statement |
| --- | --- |
| Given | a character device open to write, whose Filesystem opens it like any file |
| When | a caller allocates for it |
| Then | the call answers ENODEV, as Linux's `vfs_fallocate` allocates only for a regular file or a block device |

## `RT-275` A stream reports what it was opened and set for

| Attribute | Value |
| --- | --- |
| refines | `RT-129` |
| relates | `RT-177` |

| Step | Statement |
| --- | --- |
| Given | a stream open to read and write |
| When | the guest sets it to append and not to block, then asks what it reports |
| Then | it reports both flags and the rights to read and to write, as Linux's `F_GETFL` reports a pipe's |

## `RT-276` A descriptor reports the kind its file is

| Attribute | Value |
| --- | --- |
| refines | `RT-195` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose entry is a character device |
| When | the guest opens the entry for its path alone and asks what its descriptor reports |
| Then | the descriptor reports a character device, as a stat of the same entry does |

## `RT-277` A readlink into no room is invalid

| Attribute | Value |
| --- | --- |
| refines | `RT-208` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a Mount |
| When | the guest reads a link into a buffer of no bytes, naming a path no walk can take |
| Then | the call answers EINVAL before the path, as Linux's `do_readlinkat` refuses a size of zero first |

## `RT-278` A walk ending where nothing stands answers ENOENT before where it stands

| Attribute | Value |
| --- | --- |
| refines | `RT-241` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened beneath a Mount's root |
| When | a move carries a walk through it out from under it, to a last name nothing stands at |
| Then | the call answers ENOENT, as Linux's `step_into` refuses that name before `complete_walk` |

## `RT-279` A read-only Filesystem refuses a write before its permission is asked

| Attribute | Value |
| --- | --- |
| refines | `RT-242` |
| relates | `RT-205` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem made read-only, whose `permission` refuses every write |
| When | a caller opens one of its files to write |
| Then | the call answers EROFS, as Linux's `sb_permission` answers before `->permission` |

## `RT-280` A file without `llseek` takes no seek

| Attribute | Value |
| --- | --- |
| refines | `RT-248` |

| Step | Statement |
| --- | --- |
| Given | a file whose Filesystem answers no `FileOperations::llseek` |
| When | a call seeks it |
| Then | the call answers ESPIPE, as Linux clears `FMODE_LSEEK` for a file without `->llseek` |

## `RT-281` A truncate asks `setattr` to move the modification time too

| Attribute | Value |
| --- | --- |
| refines | `RT-215` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem holding a file |
| When | a caller truncates the open file, and opens it with `O_TRUNC` |
| Then | each `setattr` names the new size and a modification time of now, as Linux's `do_truncate` names `ATTR_MTIME` |

## `RT-282` A Filesystem asking about the name its lookup answers is asked within it

| Attribute | Value |
| --- | --- |
| refines | `RT-264` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose lookup of a name asks the runtime about that same name |
| When | a walk reaches the name |
| Then | the Filesystem is asked again within its own lookup, and the walk answers rather than waiting on itself |

## `RT-283` A guest sees one device for every entry

| Attribute | Value |
| --- | --- |
| relates | `RT-194` |

| Step | Statement |
| --- | --- |
| Given | an entry whose Filesystem reports a device and an inode number |
| When | a guest on either posture asks what it is |
| Then | it sees the device as 0, as the inode number it sees already tells entries apart |

## `RT-284` A guest's read or write moves its first buffer with room

| Attribute | Value |
| --- | --- |
| relates | `RT-096` |

| Step | Statement |
| --- | --- |
| Given | a preview1 read or write handed several buffers, the first of them empty |
| When | the Wasm Driver takes the buffers |
| Then | it moves only the first buffer with room, as wasmtime-wasi does, leaving the rest to the guest's next call |

## `RT-285` A wait on what a descriptor never does answers EBADF

| Attribute | Value |
| --- | --- |
| relates | `RT-131` |

| Step | Statement |
| --- | --- |
| Given | a guest holding its standard output, a pipe's write end |
| When | it waits for that descriptor to be ready to read, and to write |
| Then | the first answers EBADF rather than a wait no event ends, and the second is ready |

## `RT-286` A call naming two paths answers for its first before its second

| Attribute | Value |
| --- | --- |
| refines | `RT-258` |
| relates | `RT-207` |

| Step | Statement |
| --- | --- |
| Given | a guest holding its standard output, which is no directory |
| When | it moves a name from that descriptor to a path past `PATH_MAX`, or to a descriptor number naming nothing |
| Then | each call answers ENOTDIR for the first path, as Linux's `filename_renameat2` walks it before taking the second |

## `RT-287` An absolute path answers before the descriptor it names

| Attribute | Value |
| --- | --- |
| refines | `RT-258` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding nothing at a descriptor number |
| When | the guest makes a directory at an absolute path from that number |
| Then | the call answers ENOTCAPABLE, as Linux's `path_init` looks at no descriptor for an absolute path |

## `RT-288` A file made by `create` alone opens only once the security modules allow it

| Attribute | Value |
| --- | --- |
| refines | `RT-244` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem with `create` and no `atomic_open`, and a security module refusing the open |
| When | a guest opens a name where nothing stands, asking to create it |
| Then | the Filesystem makes the file, and its `open` is never asked |

## `RT-289` A readlink of an empty path answers for the descriptor it names

| Attribute | Value |
| --- | --- |
| refines | `RT-258` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file at one descriptor number and nothing at another |
| When | the guest reads a link at an empty path from each number |
| Then | the number naming nothing answers EBADF, and the file answers ENOENT |

## `RT-290` A Mount's root opens for the guest before its Policy applies

| Attribute | Value |
| --- | --- |
| refines | `RT-080` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Policy refuses its root and allows a file beneath it |
| When | the guest reads that file |
| Then | the read succeeds, and the Policy is asked only about the file |

## `RT-291` Opening a Mount's root asks the security modules

| Attribute | Value |
| --- | --- |
| relates | `RT-290` |

| Step | Statement |
| --- | --- |
| Given | a security module refusing to open a directory |
| When | a Mount's root is opened for an Invocation |
| Then | the open answers as the module refuses, as Linux's `dentry_open` asks it |

## `RT-292` An open that appends asks to append

| Attribute | Value |
| --- | --- |
| refines | `RT-242` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem answering `permission` |
| When | a call opens a file to append to it |
| Then | the open asks to append as well as to write, as Linux's `MAY_APPEND` does |

## `RT-293` A file an open makes opens once its directory is unlocked

| Attribute | Value |
| --- | --- |
| refines | `RT-244` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `open` reaches the directory it made a file in |
| When | a call opens a name where nothing stands, asking to create it |
| Then | the open succeeds, as Linux's `lookup_open` unlocks the directory before `do_open` |

## `RT-294` A file `atomic_open` finds standing opens as one that stood

| Attribute | Value |
| --- | --- |
| refines | `RT-244` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `atomic_open` finds a file at the name and makes nothing |
| When | a call opens that name, asking to create it alone |
| Then | the open answers EEXIST, as Linux's `do_open` refuses `O_EXCL` without `FMODE_CREATED` |

## `RT-295` A wait on a descriptor not open reports it in its event

| Attribute | Value |
| --- | --- |
| relates | `RT-285` |

| Step | Statement |
| --- | --- |
| Given | a guest holding no descriptor at some number |
| When | it waits for that number to be ready |
| Then | the wait succeeds, and the event answers EBADF, as Linux's `poll` marks the descriptor `POLLNVAL` |

## `RT-296` A size no file can take answers before the descriptor is found

| Step | Statement |
| --- | --- |
| Given | a guest holding no descriptor at some number |
| When | it sets the size at that number to one past the largest offset |
| Then | the call answers EINVAL, as Linux's `ftruncate` checks the length before it finds the descriptor |

## `RT-297` A descriptor opened for neither reading nor writing refuses each call through it

| Attribute | Value |
| --- | --- |
| refines | `RT-235` |

| Step | Statement |
| --- | --- |
| Given | a directory the guest opened without the right to read, list, or write |
| When | the guest lists, syncs, advises, sizes, or waits on the descriptor |
| Then | each answers EBADF, a wait in its event, as `fdget` refuses `O_PATH` |

## `RT-298` A directory a guest opens hands down the right to write

| Attribute | Value |
| --- | --- |
| refines | `RT-128` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding a directory |
| When | the guest opens the directory and reads the rights its descriptor hands down |
| Then | those rights hold `fd_write`, as Linux takes an open's mode from its flags alone |

## `RT-299` A descriptor standing only for its path keeps its times and flags

| Attribute | Value |
| --- | --- |
| refines | `RT-297` |

| Step | Statement |
| --- | --- |
| Given | a directory the guest opened without the right to read, list, or write |
| When | the guest sets the times or the flags of the descriptor |
| Then | the call answers EBADF, as Linux's `fdget` refuses `O_PATH` |

## `RT-300` A descriptor standing only for its path still names where it stands

| Attribute | Value |
| --- | --- |
| refines | `RT-297` |

| Step | Statement |
| --- | --- |
| Given | a directory the guest opened without the right to read, list, or write |
| When | the guest stats the descriptor, or a name beneath it |
| Then | each call succeeds, as Linux's `fdget_raw` takes `O_PATH` |

## `RT-301` A listing failing once it has handed entries answers those entries

| Attribute | Value |
| --- | --- |
| relates | `RT-182`, `RT-237` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose listing fails after it hands some entries |
| When | the guest lists the directory, then lists on from where it stopped |
| Then | the first call answers the entries, and the next the error, as `getdents64` does |

## `RT-302` An open standing only for its path opens nothing

| Attribute | Value |
| --- | --- |
| refines | `RT-297` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem, beneath a Policy, holding a file and a link |
| When | the guest opens either without the right to read, list, or write, following no link |
| Then | each open stands for the entry, asking neither the Policy nor the Filesystem |

## `RT-303` An open making or emptying a file without a right opens it to read

| Attribute | Value |
| --- | --- |
| relates | `RT-302` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the guest opens a file without any right, to make it or to empty it |
| Then | the open makes or empties the file, and the descriptor reads it |

## `RT-304` A listing of a directory its Filesystem cannot stat hands nothing

| Attribute | Value |
| --- | --- |
| relates | `RT-182`, `RT-237` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem reading a directory whole, unable to stat it or the directory above it |
| When | the guest lists the directory |
| Then | the listing answers that failure and hands no entry, as overlayfs does |

## `RT-305` A listing names its dots as a stat of them does

| Attribute | Value |
| --- | --- |
| refines | `RT-182` |

| Step | Statement |
| --- | --- |
| Given | a directory of any Filesystem |
| When | a caller lists the directory |
| Then | `.` and `..` carry the inode numbers a stat of each reports |

## `RT-306` A socket call refuses a descriptor standing only for its path

| Attribute | Value |
| --- | --- |
| refines | `RT-297` |

| Step | Statement |
| --- | --- |
| Given | a directory the guest opened without the right to read, list, or write |
| When | the guest makes a socket call through the descriptor |
| Then | the call answers EBADF, as Linux's socket calls take theirs through `fdget` |

## `RT-307` Sizing anything but a regular file answers EINVAL

| Attribute | Value |
| --- | --- |
| relates | `RT-137`, `RT-274` |

| Step | Statement |
| --- | --- |
| Given | a character device open to write, whose Filesystem sizes it like any file |
| When | a caller sets its size |
| Then | the call answers EINVAL, as Linux's `do_ftruncate` sizes only a regular file |

## `RT-308` A file `atomic_open` makes and opens answers as the security modules do

| Attribute | Value |
| --- | --- |
| relates | `RT-288` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `atomic_open` makes and opens the file, and a security module refusing the open |
| When | a guest opens a name where nothing stands, asking to create it |
| Then | the Filesystem makes the file and the open answers EACCES, as Linux's `finish_open` reaches `security_file_open` |

## `RT-309` Moving a descriptor onto a number nothing stands at answers EBADF

| Attribute | Value |
| --- | --- |
| relates | `RT-114` |

| Step | Statement |
| --- | --- |
| Given | a guest holding one open file and no descriptor at the next number |
| When | the guest renumbers the file onto that number |
| Then | the call answers EBADF and the file stays at its own number, as preview1 replaces only an open descriptor |

## `RT-310` A name stored under another spelling asks its Filesystem once

| Attribute | Value |
| --- | --- |
| refines | `RT-169` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem storing `F`, and reaching it by any case |
| When | a walk reaches `f` for the first time |
| Then | the walk asks `lookup` once and holds the dentry of `F`, as Linux's `d_add_ci` records the entry found |

## `RT-311` An open of a name no walk looked up asks `atomic_open`

| Attribute | Value |
| --- | --- |
| relates | `RT-244`, `RT-294` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem with `atomic_open`, holding a file no walk looked up |
| When | a guest opens that file, asking to create nothing |
| Then | the runtime asks `atomic_open` in place of `lookup`, and the open reaches the file, as Linux's `lookup_open` does |

## `RT-312` A name `atomic_open` finds under another spelling opens as that spelling

| Attribute | Value |
| --- | --- |
| refines | `RT-311` |
| relates | `RT-310` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `atomic_open` finds the name stored as `F` |
| When | a guest opens `f`, asking to create nothing |
| Then | the open holds the dentry of `F`, as `finish_no_open` hands Linux's VFS what `d_add_ci` returned |

## `RT-313` A file a read-only Mount refuses to make is made nowhere

| Attribute | Value |
| --- | --- |
| refines | `RT-311` |
| relates | `RT-148` |

| Step | Statement |
| --- | --- |
| Given | a read-only Mount, and a name a walk already found missing |
| When | a guest opens that name, asking to create it |
| Then | the open answers EROFS and the Filesystem makes nothing, as `create_error` clears `O_CREAT` |

## `RT-314` A name with a trailing slash asks `atomic_open` for a directory

| Attribute | Value |
| --- | --- |
| refines | `RT-311` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `atomic_open` opens a file no walk looked up |
| When | a guest opens that file's name with a trailing slash, asking to create nothing |
| Then | `atomic_open` is asked for a directory and the open answers ENOTDIR, as Linux's `lookup_open` adds `O_DIRECTORY` |

## `RT-315` A directory found under a second name moves its one dentry there

| Attribute | Value |
| --- | --- |
| relates | `RT-246` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem handing the same directory inode for two names |
| When | a walk reaches one name, then the other |
| Then | the dentry the first walk held now stands at the second name, as Linux's `d_splice_alias` moves a directory's alias |

## `RT-316` A directory found beneath itself answers ELOOP

| Attribute | Value |
| --- | --- |
| refines | `RT-315` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem handing a directory's own inode for a name inside it |
| When | a walk reaches that name |
| Then | the walk answers ELOOP, as `d_splice_alias` refuses an alias that is an ancestor |

## `RT-317` A directory whose move another call holds up answers ESTALE

| Attribute | Value |
| --- | --- |
| refines | `RT-315` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem handing one directory inode for names in two directories, and a call holding the rename lock |
| When | a walk reaches that name |
| Then | the walk answers ESTALE without waiting, as `__d_unalias` only tries the locks |

## `RT-318` A file `atomic_open` opens under another spelling opens as that spelling

| Attribute | Value |
| --- | --- |
| refines | `RT-312` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose `atomic_open` finds the name stored as `F` and opens it |
| When | a guest opens `f`, asking to create nothing |
| Then | the open holds the dentry of `F`, as Linux's `atomic_open` takes the dentry `finish_open` was handed |

## `RT-319` Two walks finding one directory under two names at once leave it one dentry

| Attribute | Value |
| --- | --- |
| refines | `RT-315` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem handing the same directory inode for names in two directories |
| When | two walks reach the two names at the same time |
| Then | both walks hold one dentry, as `d_splice_alias` finds the alias and adds the dentry under one `i_lock` |

## `RT-320` A walk waiting on a name a directory moved onto asks no lookup

| Attribute | Value |
| --- | --- |
| refines | `RT-315` |

| Step | Statement |
| --- | --- |
| Given | a walk looking a name up, and a second walk waiting for that lookup |
| When | the lookup finds a directory standing at another dentry, and that dentry moves to the name |
| Then | the second walk holds the moved dentry without asking the Filesystem, as `d_alloc_parallel` retries a dentry `__d_move` unhashed |

## `RT-321` A walk answered ESTALE walks its path once more

| Attribute | Value |
| --- | --- |
| relates | `RT-317` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem answering ESTALE the first time it is asked about a name |
| When | a call's walk reaches that name |
| Then | the walk is made again and the call answers as that walk does, as `filename_lookup` walks again with `LOOKUP_REVAL` |

## `RT-322` A walk answered ESTALE twice answers ESTALE

| Attribute | Value |
| --- | --- |
| refines | `RT-321` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem answering ESTALE every time it is asked about a name |
| When | a call's walk reaches that name |
| Then | the call answers ESTALE after asking the Filesystem twice, as no caller makes a walk's error again |

## `RT-323` An open asking to truncate where the mount takes no write hands `atomic_open` no truncation

| Attribute | Value |
| --- | --- |
| relates | `RT-311` |

| Step | Statement |
| --- | --- |
| Given | a read-only mount of a Filesystem opening in one step |
| When | an open asks to truncate a name no walk looked up |
| Then | its `atomic_open` is asked for no truncation, as Linux's `lookup_open` clears `O_TRUNC` without `got_write` |

## `RT-324` An exclusive create hands `atomic_open` no truncation

| Attribute | Value |
| --- | --- |
| relates | `RT-311` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem opening in one step |
| When | an open asks to make a name exclusively and to truncate it |
| Then | its `atomic_open` is asked for no truncation, as Linux's `lookup_open` clears `O_TRUNC` for `O_EXCL` |

## `RT-325` A name found under another spelling takes that spelling's dentry as it stands

| Attribute | Value |
| --- | --- |
| refines | `RT-310` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem storing `f` as `F`, and a walk that already holds the dentry of `F` |
| When | a walk reaches `f` |
| Then | it holds the dentry of `F` without asking `revalidate` about it, as Linux's `d_add_ci` finds it through `d_hash_and_lookup` |

## `RT-326` A walk to a name a lookup answered waits on no directory

| Attribute | Value |
| --- | --- |
| relates | `RT-229` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem whose make calls the runtime to walk to a name a held dentry stands at |
| When | a directory is made through the runtime in the same directory |
| Then | the inner walk answers without waiting, as Linux's `lookup_fast` takes the dcache's dentry |

## `RT-327` A walk made again for ESTALE counts the links the first one followed

| Attribute | Value |
| --- | --- |
| refines | `RT-321` |

| Step | Statement |
| --- | --- |
| Given | a path through thirty links to a name whose first lookup answers ESTALE |
| When | a call walks the path, following its last link |
| Then | the walk made again counts both walks' links toward one budget of forty, and answers ELOOP |

## `RT-328` A stream read keeps no position

| Attribute | Value |
| --- | --- |
| refines | `RT-273` |

| Step | Statement |
| --- | --- |
| Given | a file its Filesystem opens as a stream, whose read moves the position it is handed |
| When | the file is read twice |
| Then | each read is handed position 0, and the file's position stays 0, as Linux's `file_ppos` answers none for a stream |

## `RT-329` A security module the Wasm Driver is built with judges every guest call

| Attribute | Value |
| --- | --- |
| relates | `MT-007` |

| Step | Statement |
| --- | --- |
| Given | a Wasm Driver built with a security module refusing every directory made, and a Mount allowing every change |
| When | a guest makes a directory beneath the Mount |
| Then | the call answers EACCES and nothing is made, as an LSM stacked at boot refuses `security_path_mkdir` |

## `RT-330` A truncate made through an open hands the Filesystem that open

| Attribute | Value |
| --- | --- |
| refines | `RT-281` |

| Step | Statement |
| --- | --- |
| Given | a file opened to write, and an open asking to truncate it |
| When | each truncates the file |
| Then | the Filesystem's `setattr` is handed the open file each truncate is made through, as Linux's `do_truncate` sets `ATTR_FILE` |

## `RT-331` A name kept while the names beneath its directory go waits on neither

| Attribute | Value |
| --- | --- |
| relates | `RT-172` |

| Step | Statement |
| --- | --- |
| Given | a name beneath a directory, kept for the next walk over and over |
| When | the names kept beneath that directory are let go of over and over at once |
| Then | both go on and finish, neither waiting on what the other holds |

## `RT-332` A call answered ESTALE after its walk is made once more

| Attribute | Value |
| --- | --- |
| relates | `RT-321` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem answering ESTALE the first time it is asked about a name |
| When | a removal or a rename reaches that name once its walk ended |
| Then | the call is made again, walk and all, and answers as that call does, as `retry_estale` allows once |

## `RT-333` A name a call makes answered ESTALE answers ESTALE

| Attribute | Value |
| --- | --- |
| refines | `RT-332` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem answering ESTALE the first time it is asked about a name |
| When | a mkdir, a symlink, or a link looks up that name to make it |
| Then | the call answers ESTALE after asking once, as `filename_create`'s error returns before `retry_estale` |

## `RT-334` A security module reads a name where the entry stands as it asks

| Attribute | Value |
| --- | --- |
| refines | `RT-249` |

| Step | Statement |
| --- | --- |
| Given | a file moved after the open found it and before the open asks `file_open` |
| When | the security module reads the name `file_open` is handed |
| Then | the name is where the file stands after the move, as `struct path` gives it |

## `RT-335` A wait looks again once the queue it stands on wakes

| Attribute | Value |
| --- | --- |
| relates | `RT-267` |

| Step | Statement |
| --- | --- |
| Given | a wait registered on what another thread produces |
| When | that thread wakes the queue once its event arrives, or the wait's timeout passes |
| Then | the wait looks again and answers what it then finds, or nothing ready |

## `RT-336` A host-side wait waits through the registered Blocking

| Attribute | Value |
| --- | --- |
| refines | `RT-166` |

| Step | Statement |
| --- | --- |
| Given | a Blocking registered |
| When | a guest's wait finds nothing ready and sleeps on the host |
| Then | it sleeps through that Blocking, as a call waiting for a lock does |

## `RT-337` A clock wait ends with EINTR once its thread is interrupted

| Attribute | Value |
| --- | --- |
| refines | `RT-267` |

| Step | Statement |
| --- | --- |
| Given | a guest at the weaker posture waiting ten seconds on a clock |
| When | its thread is interrupted |
| Then | the wait answers EINTR at once, as a signal ends Linux's `poll` |

## `RT-338` A preview2 clock wait on the strongest posture ends at once

| Attribute | Value |
| --- | --- |
| refines | `RT-073` |

| Step | Statement |
| --- | --- |
| Given | a guest running at the strongest posture |
| When | it subscribes to ten seconds of `wasi:clocks` and blocks on the pollable |
| Then | the wait returns without the host waiting |

## `RT-339` A preview2 poll on a clock waits until the clock is due

| Attribute | Value |
| --- | --- |
| relates | `RT-074` |

| Step | Statement |
| --- | --- |
| Given | a guest at the weaker posture subscribed to fifty milliseconds of `wasi:clocks` |
| When | it polls the pollable |
| Then | `poll` answers its index once the fifty milliseconds have passed |

## `RT-340` A preview2 poll of no pollable traps

| Step | Statement |
| --- | --- |
| Given | a guest |
| When | it polls an empty list |
| Then | the call traps, as WASI says it does |

## `RT-341` A preview2 view takes no file of the open file limit

| Attribute | Value |
| --- | --- |
| relates | `RT-189` |

| Step | Statement |
| --- | --- |
| Given | a guest whose open file limit its standard streams already fill |
| When | it subscribes to a clock through `wasi:clocks`, and keeps subscribing past a million handles |
| Then | each subscription hands out a pollable, since a view holds no file. Past a million handles the call traps, as wasmtime's resource table traps past its capacity |

## `RT-342` A socket connects without blocking and its poll wakes once connected

| Attribute | Value |
| --- | --- |
| relates | `RT-335` |

| Step | Statement |
| --- | --- |
| Given | a socket connecting to a listening peer |
| When | a poll waits on its socket file for writing |
| Then | the poll wakes once the connect lands, and the connect answers success |

## `RT-343` A socket reads what its peer sent, then the end

| Step | Statement |
| --- | --- |
| Given | a connected socket whose peer sends bytes and then closes |
| When | the socket file is read until it answers zero |
| Then | it answers EAGAIN while nothing arrived, then the bytes in order, then the end |

## `RT-344` A socket sends what it was handed, then its FIN

| Step | Statement |
| --- | --- |
| Given | a connected socket handed bytes and then shut down for writing |
| When | the peer reads until the end |
| Then | it reads every byte in order, then the end, as the FIN follows the send buffer |

## `RT-345` A connect the peer refuses answers ECONNREFUSED

| Step | Statement |
| --- | --- |
| Given | an address no peer listens on |
| When | a socket connects to it |
| Then | the connect answers ECONNREFUSED once it settles |

## `RT-346` Dropping a socket file releases the host socket

| Attribute | Value |
| --- | --- |
| relates | `RT-342` |

| Step | Statement |
| --- | --- |
| Given | a connected socket whose peer is reading |
| When | the last view of its socket file is dropped |
| Then | the peer reads the end, as `sock_close` shuts the socket down |

## `RT-347` A preview2 handle and a preview1 descriptor never share a number

| Attribute | Value |
| --- | --- |
| refines | `RT-189` |

| Step | Statement |
| --- | --- |
| Given | a guest holding its three standard streams |
| When | it takes a preview2 handle, then frees it, then takes another |
| Then | the first takes 3, the lowest number neither encoding holds. The next takes 3 again, as Linux's one fdtable numbers every file |

## `RT-348` A preview2 drop of a handle as another kind traps

| Attribute | Value |
| --- | --- |
| relates | `RT-341` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a preview2 handle of one kind |
| When | it drops the handle through another kind's drop, or hands it where another kind is taken |
| Then | the call traps and the handle stays, as the Canonical ABI traps on a handle of the wrong type |

## `RT-349` A preview1 read or write of a socket's number reaches its socket file

| Attribute | Value |
| --- | --- |
| refines | `RT-347` |

| Step | Statement |
| --- | --- |
| Given | a guest whose preview2 socket connected to a peer |
| When | it writes and then reads through preview1 on the socket's number, and asks its `fdstat` |
| Then | the peer reads what was written and the guest reads the peer's reply. The `fdstat` reports a non-blocking socket stream, as one fdtable holds a socket's file. A `sock_*` call on the number answers ENOTSUP, since the rest of a socket's calls go through preview2 |

## `RT-350` A preview1 poll of a socket's number waits for its peer

| Attribute | Value |
| --- | --- |
| refines | `RT-349` |

| Step | Statement |
| --- | --- |
| Given | a guest whose preview2 socket connected to a peer that has sent nothing |
| When | it polls the socket's number to read, and the peer then writes |
| Then | the poll answers the subscription once the bytes arrive, not before |

## `RT-351` A reset reads as an error the socket's poll reports

| Attribute | Value |
| --- | --- |
| refines | `RT-343` |

| Step | Statement |
| --- | --- |
| Given | a connected socket whose peer closes with bytes it never read |
| When | the peer's reset arrives |
| Then | the socket file's poll finds ERR, as `tcp_poll` reads `sk_err`, and its read answers ECONNRESET |

## `RT-352` A socket's number answers preview1 before it connects

| Attribute | Value |
| --- | --- |
| refines | `RT-349` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a new preview2 TCP socket and a new UDP socket |
| When | it polls, reads, and writes each socket's number through preview1 before any connect |
| Then | the TCP socket polls writable and hung up. Its read answers ENOTCONN and its write EPIPE, as a fresh `tcp_sock` does. The UDP socket polls writable. Its read answers EAGAIN and its write EDESTADDRREQ, as `udp_sendmsg` answers with no peer |

## `RT-353` A preview2 list or string past the guest's memory traps

| Attribute | Value |
| --- | --- |
| relates | `RT-348` |

| Step | Statement |
| --- | --- |
| Given | a guest calling a preview2 import that takes a list or a string |
| When | it names one whose length runs past its memory, or whose length is negative as a signed number |
| Then | the call traps before the host reads or keeps anything. The Canonical ABI traps on such a list too |

## `RT-354` A preview2 call answered after the host gave up traps

| Attribute | Value |
| --- | --- |
| relates | `T-282` |

| Step | Statement |
| --- | --- |
| Given | an invocation whose host gave it up while a callback ran |
| When | the guest makes any preview2 call |
| Then | the call traps once it answered, as each preview1 call does, so the guest cannot go on as if answered |

## `RT-355` A socket whose connect failed polls ready to read

| Attribute | Value |
| --- | --- |
| refines | `RT-350` |

| Step | Statement |
| --- | --- |
| Given | a guest whose preview2 socket's connect was refused |
| When | it polls the socket's number to read |
| Then | the poll answers the subscription at once, as Linux's `poll` reports an error whatever was asked |

## `RT-356` A connected socket asked to connect again answers EISCONN

| Attribute | Value |
| --- | --- |
| refines | `RT-342` |

| Step | Statement |
| --- | --- |
| Given | a socket that connected |
| When | a Wasm Driver asks it to connect again |
| Then | it answers EISCONN and dials nothing, as Linux's `__inet_stream_connect` answers |

## `RT-357` A reset socket answers its error once, then as closed

| Attribute | Value |
| --- | --- |
| refines | `RT-351` |

| Step | Statement |
| --- | --- |
| Given | a connected socket whose peer reset it |
| When | the guest reads twice, then writes |
| Then | the first read answers ECONNRESET and the second zero, as `sock_error` clears `sk_err`. The write answers EPIPE, as `sk_stream_error` does |

## `RT-358` A socket a reset or a refused connect closed polls hung up

| Attribute | Value |
| --- | --- |
| refines | `RT-351` |

| Step | Statement |
| --- | --- |
| Given | a socket its peer reset, and a socket whose connect was refused |
| When | each socket's file is polled |
| Then | each finds IN, OUT, ERR, and HUP, as `tcp_poll` answers a socket `tcp_done` left in `TCP_CLOSE` |

## `RT-359` A socket's shut-down reading side reads what arrived

| Attribute | Value |
| --- | --- |
| refines | `RT-343` |

| Step | Statement |
| --- | --- |
| Given | a connected socket whose peer sent bytes the guest has not read |
| When | the guest shuts the socket's reading side down, then reads |
| Then | it reads the bytes, then zero, as `RCV_SHUTDOWN` stops nothing queued |

## `RT-360` A read of no bytes from a socket answers zero

| Attribute | Value |
| --- | --- |
| refines | `RT-349` |

| Step | Statement |
| --- | --- |
| Given | a connected TCP socket with nothing to read |
| When | the guest reads zero bytes from the socket's file |
| Then | the read answers zero, as `sock_read_iter` does, not EAGAIN |

## `RT-363` A socket's number answers a file's calls as Linux answers them on a socket

| Attribute | Value |
| --- | --- |
| refines | `RT-349` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a new preview2 TCP socket and a new UDP socket |
| When | it seeks, tells, reads or writes at an offset, syncs, sizes, advises, and lists each number through preview1. It then asks each fdstat and filestat |
| Then | a position or an offset answers ESPIPE, a sync or a size EINVAL, and a listing ENOTDIR. Advice succeeds, as Linux answers each on a socket. Both stats report a TCP socket as a stream and a UDP socket as datagrams |
