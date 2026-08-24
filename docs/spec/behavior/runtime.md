# Runtime

What the host checks before a guest runs, which posture it builds, and how invocations share a process.

## Why these scenarios

The runtime is what stands between a request and a guest that runs. Three checks happen before any guest does: the artifact states an ABI version the host implements, the requested isolation posture is one the ladder names, and the posture actually built is not weaker than the one accepted.

The scheduling scenarios pair off deliberately. Releasing the lock is a scheduling change and nothing else, so each witness runs one scenario under both modes and compares — a value, a dispatch, a nested dispatch, a capture. The host-parallel run is the one that shows what the mode is for.

The ambient denial the hermetic posture rests on has no scenario of its own: the default Guest Binary exposes no time or entropy surface for guest code to read, which is the posture itself, so there is nothing to observe from inside. What the determinism buys is witnessed where it shows — two invocations beginning from the same interpreter state — and what is checked here is the seam the request travels along.

## RT-001 — Threads holding their own Sandboxes hold their own guest state

| Step | Statement |
| --- | --- |
| Given | two Threads, each with its own Sandbox |
| Given | one of them having set a guest global |
| When | the other Thread reads that global |
| Then | it reads nothing the first Thread set |

## RT-002 — Threads sharing one Sandbox still evaluate on their own identity

| Step | Statement |
| --- | --- |
| Given | several Threads evaluating on one shared Sandbox, each supplying its own identity for its invocation |
| When | each Thread's guest code resolves the identity it was given |
| Then | each resolves only its own |

## RT-003 — The entrypoint verb shares a Sandbox the same way

| Step | Statement |
| --- | --- |
| Given | several Threads running an entrypoint on one shared Sandbox, each supplying its own identity for its invocation |
| When | each Thread's entrypoint resolves the identity it was given |
| Then | each resolves only its own |

## RT-004 — Releasing the lock does not pool the Handles

| Step | Statement |
| --- | --- |
| Given | several Threads invoking their own Sandboxes constructed with `gvl: :release` |
| When | each Thread restores the Handles its own invocation minted |
| Then | each restores only its own |

## RT-005 — Releasing the lock does not cross the Handle arguments either

| Step | Statement |
| --- | --- |
| Given | several Threads invoking their own Sandboxes constructed with `gvl: :release` |
| When | each Thread passes its own Handles back as dispatch arguments |
| Then | each resolves only its own |

## RT-006 — A guest that cannot state its ABI version does not run

| Step | Statement |
| --- | --- |
| Given | a Guest Binary exporting no ABI version |
| When | a Sandbox is constructed over it |
| Then | `Kobako::SetupError` is raised |

## RT-007 — A guest stating another ABI version does not run

| Step | Statement |
| --- | --- |
| Given | a Guest Binary reporting an ABI version the Host Gem does not implement |
| When | a Sandbox is constructed over it |
| Then | `Kobako::SetupError` is raised |

## RT-008 — A refused artifact is never remembered as usable

| Step | Statement |
| --- | --- |
| Given | a Guest Binary already refused once for its ABI version |
| When | a Sandbox is constructed over the same path again |
| Then | `Kobako::SetupError` is raised again |

## RT-009 — The strongest posture is the one nobody has to ask for

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying no `profile:` |
| When | the options are read |
| Then | the profile is `:hermetic` |

## RT-010 — Both rungs of the ladder may be requested

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying each ladder rung in turn |
| When | the options are read |
| Then | each reports the rung it was given |

## RT-011 — A posture off the ladder is not a posture

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying a `profile:` value the ladder does not name |
| When | the options are built |
| Then | `ArgumentError` is raised |

## RT-012 — A runtime that built less than was asked for does not run

| Step | Statement |
| --- | --- |
| Given | options requesting a rung |
| Given | a runtime declaring a rung below it |
| When | the request is enforced as a floor |
| Then | the declaration is refused |

## RT-013 — A posture nobody can place ranks below every request

| Step | Statement |
| --- | --- |
| Given | options requesting any rung |
| Given | a runtime declaring a posture the ladder does not name |
| When | the request is enforced as a floor |
| Then | the declaration is refused |

## RT-014 — A stronger posture satisfies a weaker request

| Step | Statement |
| --- | --- |
| Given | options requesting a rung |
| Given | a runtime declaring that rung or a stronger one |
| When | the request is enforced as a floor |
| Then | the declaration is accepted |

## RT-015 — The bundled runtime builds whichever rung is asked for

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a Sandbox is constructed at each ladder rung in turn |
| Then | each construction reports the rung it requested |

## RT-016 — The request reaches the runtime and comes back

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a runtime is built from that path at each ladder rung in turn |
| Then | each runtime declares the rung it was built for |

## RT-017 — The runtime refuses an unnameable posture on its own

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a runtime is built from that path with a `profile:` the ladder does not name |
| Then | `ArgumentError` is raised |

## RT-018 — Holding the lock is what happens when nobody chooses

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying no `gvl:` |
| When | the options are read |
| Then | the mode is `:hold` |

## RT-019 — Both scheduling modes may be requested

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying each scheduling mode in turn |
| When | the options are read |
| Then | each reports the mode it was given |

## RT-020 — A mode outside the set is not a mode

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying a `gvl:` value the mode set does not name |
| When | the options are built |
| Then | `ArgumentError` is raised |

## RT-021 — The runtime refuses an unknown mode on its own

| Step | Statement |
| --- | --- |
| Given | the bundled Guest Binary |
| When | a runtime is built from that path with a `gvl:` the mode set does not name |
| Then | `ArgumentError` is raised |

## RT-022 — Releasing the lock changes no value

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode |
| When | each evaluates the same guest source |
| Then | both answer the same value |

## RT-023 — Releasing the lock changes no dispatch result

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode, each with the same Service bound |
| When | each evaluates guest source that dispatches to that Service |
| Then | both answer the same value |

## RT-024 — The lock is reacquired deep enough for a nested dispatch

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode, each with the same yielding Service bound |
| When | each evaluates guest source whose dispatch dispatches again |
| Then | both answer the same value |

## RT-025 — Releasing the lock changes no capture

| Step | Statement |
| --- | --- |
| Given | one Sandbox under each scheduling mode |
| When | each evaluates guest source that writes to standard output |
| Then | both carry the same captured bytes |

## RT-026 — Released Sandboxes on distinct Threads each answer their own

| Step | Statement |
| --- | --- |
| Given | several Threads, each with its own Sandbox constructed with `gvl: :release` |
| When | every Thread evaluates guest source computing from its own input |
| Then | each Thread receives the result of its own input |

## RT-029 — Randomly generated dispatch programs answer the same under either mode

| Step | Statement |
| --- | --- |
| Given | generated dispatch programs run on Sandboxes under each scheduling mode |
| When | each program runs under both |
| Then | the two modes answer identically |

## RT-030 — Generated programs keep each Thread's references to itself

| Step | Statement |
| --- | --- |
| Given | generated dispatch programs run concurrently on distinct released Sandboxes |
| When | each Thread resolves the references it minted |
| Then | each resolves only its own |

## RT-031 — Generated programs keep each invocation's identity to itself on a shared Sandbox

| Step | Statement |
| --- | --- |
| Given | generated dispatch programs run concurrently on one shared released Sandbox, each supplying its own identity |
| When | each Thread resolves the identity it was given |
| Then | each resolves only its own |

## RT-027 — A requested posture is honored the same way by either frontend

| Step | Statement |
| --- | --- |
| Given | a scenario requesting the hermetic posture explicitly |
| When | both frontends run it |
| Then | they observe the same result |

## RT-028 — A posture switch resolves the same way on either frontend

| Step | Statement |
| --- | --- |
| Given | a scenario switching to the permissive posture |
| When | both frontends run it |
| Then | they resolve it the same way |
