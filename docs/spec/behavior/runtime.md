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
- `crates/kobako-runtime/src/endpoint.rs`
- `crates/kobako-wasmtime/src/ambient.rs`
- `crates/kobako-wasmtime/src/descriptors.rs`
- `crates/kobako-wasmtime/src/preview1/testsuite.rs`
- `crates/kobako-wasmtime/src/cache.rs`
- `crates/kobako-wasmtime/src/abi.rs`
- `crates/kobako-wasmtime/src/wasi.rs`
- `crates/kobako-wasmtime/src/preview1/poll.rs`
- `crates/kobako-wasmtime/src/preview1/fd.rs`
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
| Given | a Guest Binary reporting an ABI version the Host Gem does not implement |
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
| Then | the other Thread makes no progress until the guest code finishes |

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

## `RT-099` An allocation for a file beneath a Mount is unsupported

| Attribute | Value |
| --- | --- |
| relates | `RT-077` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding a file |
| When | the guest opens the file to write and asks to allocate room in it |
| Then | the allocation answers NOTSUP, and the file holds what it held |

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

## `RT-101` A path through a link beneath a Mount with a Policy answers ELOOP

| Attribute | Value |
| --- | --- |
| refines | `RT-081` |
| relates | `RT-094`, `RT-100`, `FS-047` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount with a Policy, holding a link to a directory |
| When | the guest opens or removes a path through the link, or opens the link following it |
| Then | each call answers ELOOP and changes nothing |

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
| Given | an invocation handed a Mount whose directory a guest began to list, and a file then made in it |
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

## `RT-114` Moving a descriptor onto an open file closes that file first

| Attribute | Value |
| --- | --- |
| relates | `FL-010` |

| Step | Statement |
| --- | --- |
| Given | a guest holding two open files, the second on a Filesystem that refuses to close it |
| When | the guest renumbers the first onto the second |
| Then | the call answers that refusal and the second descriptor is closed |

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

## `RT-119` A step out of a link answers ELOOP

| Attribute | Value |
| --- | --- |
| refines | `RT-087` |
| relates | `RT-101`, `RT-109` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a link to a directory |
| When | the guest names a path taking `..` or a final `.` right after the link |
| Then | the call answers ELOOP |

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

## `RT-125` A link out of a Bind or into a loop answers ELOOP beneath a Policy

| Attribute | Value |
| --- | --- |
| refines | `RT-101` |

| Step | Statement |
| --- | --- |
| Given | a Bind Mount with a Policy, holding links out of it and into a loop |
| When | the guest opens a path through each link, then unlinks each link |
| Then | each open answers ELOOP, and each unlink removes the link |

## `RT-126` A link check that fails fails the call beneath a Policy

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
| When | the guest stats the FIFO and opens it |
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

## `RT-130` An advice for a length past the largest signed offset is invalid

| Attribute | Value |
| --- | --- |
| refines | `RT-116` |

| Step | Statement |
| --- | --- |
| Given | an invocation holding a file open beneath a Mount |
| When | the guest advises on a length past the largest offset, or from past it |
| Then | the first advice answers EINVAL, and the second succeeds, as Linux does |

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

## `RT-142` A create through a dangling link beneath an opened directory is refused

| Attribute | Value |
| --- | --- |
| refines | `RT-140` |
| relates | `RT-106` |

| Step | Statement |
| --- | --- |
| Given | a guest holding a directory it opened, which holds a link pointing at nothing |
| When | it opens the link's path to create a file, following the link |
| Then | the open answers ELOOP and creates nothing |

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

## `RT-146` A Policy is asked once for an open that may create, as what it does

| Attribute | Value |
| --- | --- |
| refines | `MT-025` |
| relates | `MT-007` |

| Step | Statement |
| --- | --- |
| Given | a Mount whose Policy records each access it is asked |
| When | the guest opens a missing path and an existing file to create, without the right to write |
| Then | the Policy was asked once per open: as a change for the missing path, as a read for the file |

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

## `RT-150` A directory moved while opened beneath a Policy is not reached

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

## `RT-153` A step out of where a link led answers ELOOP

| Attribute | Value |
| --- | --- |
| refines | `RT-119` |

| Step | Statement |
| --- | --- |
| Given | a directory opened beneath a Mount, holding a link to a directory within it |
| When | the guest names a path stepping out with `..` after passing through the link |
| Then | the call answers ELOOP |

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

## `RT-180` An open file answers for itself whichever Driver asks

| Attribute | Value |
| --- | --- |
| refines | `RT-116` |
| relates | `RT-133` |

| Step | Statement |
| --- | --- |
| Given | a file opened beneath a Mount, with no Driver in front of it |
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
| Given | a directory opened through the runtime, with no Driver in front of it |
| When | a caller lists it, stops early, and lists it again from where it stopped |
| Then | `.` and `..` come first, and each entry carries the position after it |

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
| When | it opens a path that names nothing |
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
| Then | only the tests needing a link made or a time set fail |

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
| When | a preview1 Driver encodes each kind |
| Then | a socket reads as a stream socket, each device as itself, and a FIFO as unknown |

## `RT-196` A socket a guest sees cannot be opened

| Attribute | Value |
| --- | --- |
| refines | `FS-090` |
| relates | `RT-127` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a socket |
| When | the guest stats the socket and opens it |
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
| When | the guest opens the link following no link |
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
| When | it reaches the VFS, whichever Driver brought it |
| Then | it answers EINVAL, ENOENT, or ENAMETOOLONG, and the Filesystem is not asked |

## `RT-214` A link answers as Linux's where no security module refuses

| Attribute | Value |
| --- | --- |
| relates | `RT-119`, `RT-142` |

| Step | Statement |
| --- | --- |
| Given | a Bind holding a link to a directory and a link to nothing, both beneath it |
| When | a call steps out of the first, and creates through the second, under no security module |
| Then | each resolves as Linux's, and under kobako's own module each answers ELOOP |

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
