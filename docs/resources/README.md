# Resources

Resources holds how invocations share what one process has, so that none jams another. It is for maintainers changing what an invocation holds, waits for, or gives back.

| Piece | Lives in |
|---|---|
| the tiers, their order, the promises between them | this document |
| the order of the locks within them | [`locking/`](../locking/README.md) |
| the models that check both levels | [`tla/`](tla/) |
| what kobako promises a host app | [`spec/`](../spec/) |

These rules are the architecture a change is built to. A scenario states each behavior as it is promised.

## One Requirement

**What one invocation occupies never jams another.** Becoming slower is an accepted answer, and waiting for ever is not.

| Fairness is | Fairness is not |
|---|---|
| each invocation has a chance to finish | equal shares between tenants |
| a waiter is served in turn | priorities or weights |
| a reservation holds whatever others do | each runs as fast as alone |
| past the totals, a wait or a clear refusal | a limit on how many run at once |

A host app that wants a pace sets it with a pool of its own.

## Five Tiers

Everything one invocation can occupy sits in one tier. An invocation holding one tier waits only for a lower one, as in locking.

```
+----------------------+
| admission (a)        |
+----------+-----------+
           | waits
           v
+----------------------+
| sleeping locks (b)   |
+----------+-----------+
           | waits
           v
+----------------------+
| reactor (c)          |
+----------+-----------+
           | waits
           v
+----------------------+
| GVL (d)              |
+----------+-----------+
           | takes
           v
+----------------------+
| other locks (e)      |
+----------------------+
```

- (a) The totals, the line for them, and the instant of a fork.
- (b) The locks a shared filesystem holds across an operation.
- (c) The one thread that waits on every host socket.
- (d) Ruby's GVL.
- (e) Every other lock, with the accounts of memory, descriptors, and counts.

What the host refuses to give, such as a thread or address space, is answered at once as (e) is. Cores are the host's to schedule, so a neighbour's use of them only slows.

### Holding Spans

Each tier is held for a span of its own, and that span ends without help from above.

| Tier | Held for | Ends by |
|---|---|---|
| admission | one invocation | its deadline or its end |
| sleeping locks | one operation | bounded work |
| reactor | one turn | a bounded share for each socket |
| GVL | one quantum | the waiting caller's yield |
| other locks | one charge or short hold | an answer at once |

### Locking Tiers

Three of the tiers are locking's, unchanged, and every lock keeps its lock group there. Resources adds the two that are waited for and are no lock.

| Tier | In locking |
|---|---|
| admission | added, above every tier |
| sleeping locks | tier (a) |
| reactor | added, waited for through the `Blocking` |
| GVL | tier (b) |
| other locks | tier (c) |

Sleeping locks stand above the reactor, since a callback under a directory lock may start a nested invocation that reaches the network. Nothing leads back.

## Three Promises

A tier owes the whole three promises and nothing else. So each tier is proved alone, and only the promises are assembled.

| Promise | States | Proved by |
|---|---|---|
| lock ordering | a holder waits only for a lower tier | the whole |
| starvation freedom | a waiter that is to end gets it | each tier |
| forward progress | a holder lets go, given those below do | each tier |

They follow XFS's log reservation as a reference architecture: reserve all first, serve in order, never pin what frees space.

### Proof Levels

The proof has two levels, as locking's has. Neither stands in for the other.

```
+----------------------------------------------+
| each tier (a)                                |
+----------------------+-----------------------+
                       | proves
                       v
+----------------------------------------------+
| promises (b)                                 |
+----------------------+-----------------------+
                       | assembled into
                       v
+----------------------------------------------+
| whole (c)                                    |
+----------------------------------------------+
```

- (a) A tier's own rules, proved alone, whatever the others are.
- (b) The only words the two levels share.
- (c) No cycle between tiers, and no jam once assembled.

A change inside a tier therefore asks one question: does it still keep the three promises?

### Prevention

A rule removes the condition that would jam. It never detects a jam or cuts one short.

| Condition | Prevented by |
|---|---|
| waiting while holding what another needs | taking all at admission |
| waiting while holding the GVL | giving it up first |
| a holder that needs a guest's help to let go | the caller yields by itself |
| a wait that closes a cycle through a fork | the fork refuses instead |

No deadline is part of a proof, and no interface exists to stop an invocation.

## Admission

An invocation takes its whole reservation in one step before it starts. It holds none of it while it waits, and never waits for the totals again.

```
+-------------+  asks   +--------------+  grants  +--------------+
| waiter (a)  |-------->| line (b)     |--------->| invocation   |
+-------------+         +------+-------+          +------+-------+
                               ^                         |
                               |                         | ends
                               |   returns               v
                               +-----------------+--------------+
                                                 | totals (c)   |
                                                 +--------------+
```

- (a) Holds no part of the totals, no lock, and no GVL.
- (b) First come, first served, with a deadline of its own.
- (c) Memory and host handles. The number of invocations is never one.

### Reservation

A reservation is the worst case: what the host app's caps allow, and kobako's fixed cost.

| Part | Rule |
|---|---|
| a capped resource | reserved whole |
| a nested invocation | declared, and inside the outer's reservation |
| what is still closing | counted until it has closed |
| a seed | counted as the instance is first mounted |

An undeclared nested invocation is refused at once, whatever the load. So is a reservation larger than a total.

### Pinned Share

A pinned holding never ends by itself: what an uncapped invocation holds, and what a tmpfs instance keeps. Together they always leave room for one invocation at the default caps. A larger one waits as the second row of Passed Waiters does.

```
reserved = cores * one reservation
left     = one reservation + what that one may leave
share    = max(0, min(max(total - reserved, one reservation), total - left))
grant that pins: pinned + adds + one reservation <= total
```

An invocation without a timeout holds its whole reservation from the pinned share. Uncapped growth past the share fails at once with that cap's own error.

### Passed Waiters

A waiter keeps its place and is passed over only while it cannot be the one to run.

| Waiter | Passed over |
|---|---|
| runs host app code, such as a trap handler | until it waits again |
| fits only once what is pinned is gone | while that stays pinned |
| is larger and waits for invocations that end | never |

Without this, one waiter the share cannot hold would jam everyone behind it.

### Fork Instant

A fork never waits for an invocation. It raises while any is in progress.

| Step | Rule |
|---|---|
| check and close | one step |
| while closed | idle caches are let go, and nobody is admitted |
| open | the waiter is handed the gate in the same step |

Opening that only woke a waiter would let repeated forks keep it out for ever.

## Sleeping Locks

A sleeping lock is held for one operation, and the operation's work is bounded.

| Promise | Kept by |
|---|---|
| lock ordering | no wait for admission under a lock |
| starvation freedom | a handoff to the first waiter past its time |
| forward progress | bounded work, with safepoints in long loops |

The handoff follows Linux's rwsem: a waiter keeps its place through a signal, and one that leaves passes the handoff on. A shared writable filesystem shares its space too, so one filling it is expected.

## Reactor

The reactor serves every invocation's sockets on one thread, so one turn is bounded.

| Promise | Kept by |
|---|---|
| lock ordering | the reactor takes neither the GVL nor a future |
| starvation freedom | leftover work goes behind new events, and none is dropped |
| forward progress | a bounded share for each socket, one waiter for each source |

A closed connection returns once its peer takes it, or at the latest as its invocation ends. Until then it counts inside that invocation's own reservation.

## GVL

Both `gvl:` modes keep the promises. `:hold` keeps the GVL for short runs, and `:release` drops it while the guest runs.

| Promise | Kept by |
|---|---|
| lock ordering | given up before any wait |
| starvation freedom | CRuby's own ready queue |
| forward progress | the caller yields each quantum, as CRuby's timer does |

The quantum starts as the GVL is taken, and no message restarts it. The caller looks at its mailbox once before it judges the time. A guest runs on through its caller's yield.

## Other Locks

An account answers a charge at once, so nobody waits on one for room. These locks keep the order between their groups that locking gives them.

| Promise | Kept by |
|---|---|
| lock ordering | no call into Ruby under a short lock |
| starvation freedom | no wait for room |
| forward progress | each charge is one step |

Each invocation's account is its own, and what another takes over is charged to that one. A count is taken before the effect leaves the host, and is never given back.

## Host App Code

The host app's code is outside the order, as `HostMutex.tla` shows for locking. Put into the models it closes a cycle, so it is a boundary that the guides state.

| Boundary | Whose cost |
|---|---|
| a callback that never returns | the host app's |
| a lock a callback needs, held around a run | the host app's |
| host I/O that never returns | the host's |
| time in the host that a timeout does not count | the host app's |
| content the host app keeps | storing invocations are refused at their deadline |
| forks overlapping for ever | the host app's |
| how a guest binary is built | the host app's |

A cap set to `nil` costs whoever set it, and never jams another.

## Models

A rule enters a model only where taking it away jams another invocation. So the suite is a sum over the tiers, never a product.

| Kind of rule | Witnessed by |
|---|---|
| taking it away jams another | a model |
| whether an amount is right | a test |
| what a guest reads | a scenario |
| how fast it is | a benchmark |

### Model Suite

Each tier has one model, and the whole has two. A model of each broken rule must still fail, so each rule is proved to hold the design up.

| Model | Checks |
|---|---|
| `ResourceOrder` | every edge added to `GroupOrder` still runs downward |
| `Assembly` | the promises alone leave nobody jammed |
| `Admission` | all at once, the share, the line, the fork |
| `SleepingLocks` | the handoff and the bounded operation |
| `Reactor` | the bounded turn and its order |
| `Gvl` | the yield without a guest's help |
| `OtherLocks` | an account of one's own, and no wait for room |

### Model Limits

The models prove the rules, and each abstracts what another witness decides.

| Abstracted | Decided by |
|---|---|
| two totals as one | a test of the arithmetic |
| the cores' reserve as one reservation | the same test |
| shared holds of a sleeping lock | Linux's baseline |
| that code follows a rule | the scenario's test |
| that a run with a timeout ends | the timeout's scenarios, up to the host app's boundary |
| the step from a promise to the assembly's fairness | this argument: waits leads to has implies it |

### Model Checks

A change to a tier, an edge, or a promise runs the models again. They need a JDK, as a launcher is built so that TLC opens no socket.

1. Set `TLA2TOOLS_JAR` to the path of `tla2tools.jar`.
2. Run `bundle exec rake resources:check`.
3. Run `bundle exec rake locking:check` where a lock's edge changed.

A new edge goes into `ResourceOrder.tla` first. A new rule gets one configuration where it is taken away. The checks run at once and end within thirty seconds, since a rule that needs more is a design grown too complex.
