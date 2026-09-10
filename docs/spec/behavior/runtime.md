# Runtime

What the host checks before a guest runs, which posture it builds, and how invocations share a process.

## Includes

- `test/unit/values/test_sandbox_options.rb`
- `test/e2e/test_threading.rb`
- `test/e2e/sandbox/test_sandbox.rb`
- `test/e2e/sandbox/test_sandbox_profile.rb`
- `test/e2e/sandbox/test_gvl_scheduling.rb`
- `test/e2e/sandbox/test_gvl_handle_isolation.rb`
- `test/e2e/runtime/test_runtime.rb`
- `test/e2e/runtime/test_artifact_cache.rb`
- `test/e2e/runtime/test_snapshot.rb`
- `test/e2e/sandbox/test_null_guest.rb`
- `test/fuzz/test_dispatch_scheduling_fuzz.rb`
- `test/parity/test_hermetic.rb`
- `crates/kobako-wasmtime/src/ambient.rs`
- `crates/kobako-wasmtime/src/frames.rs`
- `crates/kobako/tests/concurrency.rs`
- `crates/kobako/tests/runtime_injection.rs`
- `crates/kobako-runtime/src/profile.rs`

### Why these scenarios

The runtime is what stands between a request and a guest that runs. Three checks happen before any guest does: the artifact states an ABI version the host implements, the requested isolation posture is one the ladder names, and the posture actually built is not weaker than the one accepted.

The scheduling scenarios pair off deliberately. Releasing the lock is a scheduling change and nothing else, so each witness runs one scenario under both modes and compares — a value, a dispatch, a nested dispatch, a capture. The host-parallel run is the one that shows what the mode is for.

The ambient denial the hermetic posture rests on has no scenario of its own: the default Guest Binary exposes no time or entropy surface for guest code to read, which is the posture itself, so there is nothing to observe from inside. What the determinism buys is witnessed where it shows — two invocations beginning from the same interpreter state — and what is checked here is the seam the request travels along.

Compiling an artifact is expensive enough to keep on disk, and a cache is a second way in. So each of its refusals is witnessed twice over: that construction still succeeds, and what the cache directory holds afterwards. A cache that quietly loaded a planted artifact would pass the first observation alone.

What the runtime hands an invocation back is read here rather than through the Sandbox that usually reads it, because a binding that shifted a field's shape would still satisfy every assertion the Sandbox makes about the value it derived. Which is also why the two channels are separated twice: once at this seam, and once as a difference the two frontends must agree on.

An artifact that satisfies the whole invocation ABI while doing no guest work is what makes the host's own per-invocation cost measurable as a total. That is a claim about the artifact, so it is held to both verbs and to the capture it leaves — the ways it could satisfy the loader without satisfying the ABI.

### Behaviors without a witness

Invocations running at once, on one Sandbox or on several, each capture only what they wrote.

A Service bound once on a Sandbox shared across Threads may be called by several invocations at once; kobako does not serialize those calls.

Guest code on distinct Threads, each invoking a Sandbox constructed to release the lock, runs in parallel rather than one at a time.

A Sandbox that holds the lock runs no other host Thread while its guest code does.

A reference from an earlier invocation resolves to nothing whether the Sandbox releases the lock or holds it.

An invocation that fails, fails the same way whether its Sandbox releases the lock or holds it.

At either posture the guest reaches no filesystem, environment variable, or network through the WASI layer.

At either posture the Guest Binary's only host import is the dispatch entry.

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

## `RT-005` Releasing the lock does not cross the Handle arguments either

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

## `RT-038` And nothing is written back into it

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
| Then | the outcome names its successful arm, carries payload bytes, and carries no record |

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

## `RT-045` And answers an entrypoint run carrying arguments

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

## `RT-053` And is handed the registrations the Sandbox sealed

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
