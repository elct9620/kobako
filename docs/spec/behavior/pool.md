# Pool

How a Pool hands out warm Sandboxes, and what a checkout leaves behind.

## Why these scenarios

A Pool is observable in three places: which Sandbox a checkout receives, how many times the setup block has prepared one, and whether a slot survives what its holder did with it. Each scenario settles one of those and stops.

The checkout wait bound and the constructor's argument validation raise rather than answer, so they belong to the error taxonomy and are specified there.

That a pooled Sandbox satisfies every other behavior identically to a directly constructed one is a claim about the whole corpus rather than a difference one observation settles; the isolation scenarios above are its pool-side witnesses.

## PL-001 — Construction builds no Sandbox

| Step | Statement |
| --- | --- |
| Given | a setup block that counts the pooled Sandboxes it prepares |
| When | `Kobako::Pool.new(slots: 2)` runs |
| Then | the setup block has not run |

## PL-002 — The checkout wait has a default bound

| Step | Statement |
| --- | --- |
| Given | a Pool constructed without `checkout_timeout:` |
| When | the default checkout wait bound is read |
| Then | the bound is 5.0 seconds |

## PL-003 — An idle Sandbox is preferred over a new one

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 2` |
| Given | a completed checkout that left its Sandbox idle |
| When | `Pool#with` runs again |
| Then | the block receives the Sandbox the first checkout held |

## PL-004 — Setup prepares a Sandbox, not a checkout

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 2` and a setup block that records each Sandbox it prepares |
| When | two sequential checkouts complete |
| Then | the setup block has run once |

## PL-005 — Sandbox keywords reach the Sandboxes the Pool builds

| Step | Statement |
| --- | --- |
| Given | a Pool constructed with `slots: 1` and `timeout: 0.05` |
| When | a checked-out Sandbox evaluates a non-terminating loop |
| Then | `Kobako::TimeoutError` is raised |

## PL-006 — A setup failure surfaces where it was triggered

| Step | Statement |
| --- | --- |
| Given | a Pool whose setup block raises on its first run |
| When | the first `Pool#with` triggers that construction |
| Then | the setup block's own exception reaches the `#with` caller unchanged |

## PL-007 — A failed construction does not consume its slot

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` |
| Given | a first checkout whose construction failed in the setup block |
| When | `Pool#with` runs again |
| Then | the checkout succeeds and the block's value comes back |

## PL-008 — Checkout answers with what the caller computed

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` |
| When | `Pool#with` runs with a block returning a value |
| Then | `#with` returns that value |

## PL-009 — Setup registrations outlive the checkout that first used them

| Step | Statement |
| --- | --- |
| Given | a Pool whose setup block binds a Service on each Sandbox it prepares |
| Given | a completed checkout that reached that Service |
| When | a later checkout evaluates guest code naming the same Service |
| Then | the Service returns its value to the guest |

## PL-010 — Guest state does not travel between holders

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` |
| Given | a completed checkout that set a guest global variable |
| When | a later checkout reads that global |
| Then | it reads `nil` |

## PL-011 — A full Pool makes the caller wait rather than refuse

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` whose only Sandbox is held by another thread |
| Given | a checkout blocked waiting for it |
| When | the holder checks its Sandbox back in |
| Then | the blocked checkout completes and returns its block's value |

## PL-012 — A nested checkout is an ordinary second holder

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 2` |
| Given | a running `Pool#with` block on this thread |
| When | `Pool#with` runs again inside that block |
| Then | the inner block receives a different Sandbox than the outer one |

## PL-013 — A trapped Sandbox is never handed out again

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` |
| Given | a checkout whose block raised `Kobako::TrapError` |
| When | `Pool#with` runs again |
| Then | the block receives a different Sandbox than the one the trap left |

## PL-014 — The refilled slot is a working one

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` |
| Given | a checkout whose block raised `Kobako::TrapError` |
| When | the next checkout's Sandbox evaluates guest code |
| Then | the evaluation returns its value |

## PL-015 — Refilling builds a Sandbox rather than reviving one

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` and a setup block that records each Sandbox it prepares |
| Given | a checkout whose block raised `Kobako::TrapError` |
| When | `Pool#with` runs again |
| Then | the setup block has run twice |

## PL-016 — Only a trap costs the Pool its Sandbox

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` |
| Given | a checkout whose block raised `Kobako::SandboxError` |
| When | `Pool#with` runs again |
| Then | the block receives the Sandbox that error left |

## PL-017 — A guest error costs no construction

| Step | Statement |
| --- | --- |
| Given | a Pool with `slots: 1` and a setup block that records each Sandbox it prepares |
| Given | a checkout whose block raised `Kobako::SandboxError` |
| When | `Pool#with` runs again |
| Then | the setup block has run once |

## PL-018 — Reachability is the whole of the lifecycle

| Step | Statement |
| --- | --- |
| Given | a Pool that has constructed one Sandbox |
| Given | no remaining Host App reference to either |
| When | garbage collection runs |
| Then | the Pool and the Sandbox are both reclaimed |

## PL-019 — A holder outlives the Pool it borrowed from

| Step | Statement |
| --- | --- |
| Given | a running `Pool#with` block that has dropped the last Pool reference and run a collection |
| When | the checked-out Sandbox evaluates guest code inside that block |
| Then | the evaluation returns its value |
