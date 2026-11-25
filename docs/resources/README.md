# Resources

Resources holds how invocations share what one process has, so that none jams another. It is for maintainers changing what an invocation holds, waits for, or gives back.

| Piece | Lives in |
|---|---|
| what an invocation may use, and what kobako spends for it | this document |
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
| an invocation's caps hold whatever others do | each runs as fast as alone |
| each invocation bounded by its own caps | a total across invocations |

How many invocations run at once, and so what they hold together, is the host app's to set, with a pool of its own. kobako bounds one invocation and gives back all it held.

## Memory

An invocation is a small virtual machine, as a sandbox runtime such as gVisor or Firecracker treats one. It has two parts, and neither waits for room.

```
+- Invocation ----------------------------------------------+
| +------------+  host call  +-------------+                |
| | Store (a)  |------------>| Runtime (b) |                |
| +-----+------+             +------+------+                |
|       |                           | sockets               |
|       |                           v                       |
|       |                    +-------------+                |
|       |                    | Kernel (c)  |                |
|       |                    +-------------+                |
+-------+---------------------------------------------------+
        | payload
        v
 +------------+  dispatch   +-------------+
 | Codec (d)  |------------>| Service (e) |
 +------------+             +-------------+
```

- (a) The guest's memory, its RAM: linear memory, tables, and the GC heap that wasmtime grows. Only this counts against `memory_limit`.
- (b) What the runtime spends answering the guest, as gVisor's Sentry does: names, listings, lifted lists, TLS sessions, socket queues.
- (c) What the host kernel holds for the runtime's sockets.
- (d) The Codec decoding a guest payload. It bounds what a payload makes it build.
- (e) The host app's Services. What they hold is the host app's.

### Guest RAM

`memory_limit` is the guest's RAM. A growth past it is refused at once, before the engine allocates. The guest sees it fail, as `memory.grow` answers -1, so it can collect what it dropped and try again, as memcg reclaims before it kills. A run that fails after a refusal fails as `memory_limit`'s.

| Growth | Counted |
|---|---|
| what the guest binary declares | no, it is the baseline |
| linear memory, tables, GC heap past that | yes, from the Store's start |
| the host's copy of what one call lifts | no, the hostcall fuel bounds it |

### Overhead

The runtime and the kernel are the invocation's overhead, as a virtual machine's device emulation is. Nothing of it is charged as the invocation runs. Each part is bounded instead by a count the caps already set, times a fixed size, or it streams.

| Part | Count | Each at most |
|---|---|---|
| a lookup | `open_connection_limit` | a name of 255 bytes, and one DNS message's addresses |
| a socket's queues | `open_connection_limit` | `sk_rcvbuf` and `sk_sndbuf` |
| a TLS session, an exchange's buffers | `open_connection_limit` | the library's own buffers |
| a host socket's buffers | `open_connection_limit` | the host's autotuning maximum |
| a listing, an overlay's merge | `open_file_limit` | the directory's own names |
| a lifted list | one host call | the hostcall fuel, set to `memory_limit` |

A filesystem's cached names belong to the filesystem instance, bounded by its own cache, as gVisor's per-mount dentry cache is. A shared object is charged to no invocation, so no holder can make another pay.

### Outside the Machine

A Codec and a Service live outside the machine, as a host daemon behind a virtual machine's socket does.

| Part | Bounded by |
|---|---|
| what a payload makes a Codec build | the Codec, which alone knows its amplification |
| the Handles one invocation is issued | a table of 1,000,000, as wasmtime's `ResourceTable` |
| what a Service builds or keeps | the host app |
| what a Tmpfs holds | its own `size` and `nr_inodes`, outliving every invocation that wrote it |

## Five Tiers

Everything one invocation can wait on sits in one tier. An invocation holding one tier waits only for a lower one, as in locking.

```
+----------------------+
| fork gate (a)        |
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

- (a) The instant of a fork, which a run waits on only while a fork closes it.
- (b) The locks a shared filesystem holds across an operation.
- (c) The one thread that waits on every host socket.
- (d) Ruby's GVL.
- (e) Every other lock, with the accounts of the guest's memory, descriptors, and counts.

What the host refuses to give, such as a thread or address space, is answered at once as (e) is. Cores are the host's to schedule, so a neighbour's use of them only slows.

### Holding Spans

Each tier is held for a span of its own, and that span ends without help from above.

| Tier | Held for | Ends by |
|---|---|---|
| fork gate | one fork's instant | the fork's own step |
| sleeping locks | one operation | bounded work |
| reactor | one turn | a bounded share for each socket |
| GVL | one quantum | the waiting caller's yield |
| other locks | one charge or short hold | an answer at once |

### Locking Tiers

Four of the tiers are locking's, unchanged, and every lock keeps its lock group there. Locking's tier (a) holds two of them, with the gate above the rest. Resources adds the reactor, which is waited for and is no lock.

| Tier | In locking |
|---|---|
| fork gate | tier (a), the gate group |
| sleeping locks | tier (a), every other group |
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
| waiting for room while holding | no charge waits; one past a cap fails |
| one invocation paying for what another holds | a shared object is charged to none |
| waiting while holding the GVL | giving it up first |
| a holder that needs a guest's help to let go | the caller yields by itself |
| a wait that closes a cycle through a fork | the fork refuses instead |

No deadline is part of a proof, and no interface exists to stop an invocation.

## Fork Gate

A fork never waits for an invocation. It raises while any is in progress, and a run waits only while a fork holds the gate.

| Step | Rule |
|---|---|
| check and close | one step |
| while closed | idle caches are let go, and no run enters |
| open | the waiter is handed the gate in the same step |

Opening that only woke a waiter would let repeated forks keep it out for ever. Locking's `ForkGate` model proves the gate.

## Sleeping Locks

A sleeping lock is held for one operation, and the operation's work is bounded.

| Promise | Kept by |
|---|---|
| lock ordering | no wait at the fork gate under a lock |
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

A closed connection returns once its peer takes it, or at the latest as its invocation ends. Until then it keeps its place among the invocation's connections.

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

Each invocation's account is its own, and nothing another holds is charged to it. A count is taken before the effect leaves the host, and is never given back. A count covers only what leaves the host, such as a lookup or a dial, as a Worker counts its subrequests. A check refusing an attempt first spends nothing.

## Host App Code

The host app's code is outside the order, as `HostMutex.tla` shows for locking. Put into the models it closes a cycle, so it is a boundary that the guides state.

| Boundary | Whose cost |
|---|---|
| a callback that never returns | the host app's |
| how often a Service, a filesystem, or a policy is called | the host app's |
| a lock a callback needs, held around a run | the host app's |
| host I/O that never returns | the host's |
| time in the host that a timeout does not count | the host app's |
| how many invocations run at once | the host app's |
| content a filesystem instance keeps | the host app's, up to the instance's own size |
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

Each tier below the fork gate has one model, and the whole has two. The fork gate's is locking's `ForkGate`. A model of each broken rule must still fail, so each rule is proved to hold the design up.

| Model | Checks |
|---|---|
| `ResourceOrder` | every edge added to `GroupOrder` still runs downward |
| `Assembly` | the promises alone leave nobody jammed |
| `SleepingLocks` | the handoff and the bounded operation |
| `Reactor` | the bounded turn and its order |
| `Gvl` | the yield without a guest's help |
| `OtherLocks` | an account of one's own, and no wait for room |

### Model Limits

The models prove the rules, and each abstracts what another witness decides.

| Abstracted | Decided by |
|---|---|
| each overhead's bound | a test of the count and the size |
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
