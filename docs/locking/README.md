# Locking

Locking holds the order every lock is taken in, across the Contexts and the Frontends. It is for maintainers changing a lock, a wait, or what a call holds while it calls out.

| Piece | Lives in |
|---|---|
| the tiers, the lock groups, the edges between them | this document |
| one group's own order | its Context's Lock Order section |
| the models that check both | [`tla/`](tla/) |
| what a Host App must not do | [`guides/concurrency.md`](../guides/concurrency.md) |

A lock confined to one type keeps its rule in that type's doc comment.

## Three Tiers

Every lock sits in one of three tiers. A thread holding one tier waits only for a lower one.

```
+----------------------+
| sleeping locks (a)   |
+----------------------+
           | callback takes
           v
+----------------------+
| GVL (b)              |
+----------------------+
           | holds briefly
           v
+----------------------+
| other locks (c)      |
+----------------------+
```

- (a) `i_rwsem`, `rename_mutex`, an Overlay entry's copy-up lock, and a dentry in lookup. A callback is a Ruby Filesystem's method or a mount's block, which takes the GVL while the run holds one of these.
- (b) Ruby's GVL. A thread holding it may take a sleeping lock that is free, but gives the GVL up before it waits, through the registered `Blocking`. A waiting thread takes it back only to handle a signal, holding no lock of tier (c).
- (c) Every other lock. It never calls Ruby and never waits through the `Blocking`.

### GVL Modes

The GVL decides which runs contend for a lock, so every rule here holds in both modes.

| Mode | Runs contend |
|---|---|
| `:hold` | only while one waits or calls back |
| `:release` | at any moment |

Under `:hold` the GVL runs the guests one at a time; under `:release` each run drops it while its guest runs.

### Run Threads

A run's guest stands on a thread of its own, and its caller serves what the guest needs of Ruby. The two take turns, so the models treat one run as one thread.

| Thread | Holds | Waits for |
|---|---|---|
| the guest's | the Wasm frames, and the sleeping locks the run takes | its caller's answer to a callback |
| the caller's | the GVL while it runs a callback | the guest's next callback or its end |

A lock knows its holder by the run, which a run nested in a callback shares. So a nested run asking for its own run's lock is answered, never left waiting. A run on a Fiber a callback starts on the same Thread holds as that run too, through the mark Fiber storage carries. The caller returns only once its guest ends. `Handoff.tla` checks both.

| Stop | Reaches the guest |
|---|---|
| a deadline, or the run's interrupt | at its next wait, or within an epoch tick as it computes |
| a signal Ruby sends the run | handled at its next wait or tick, as Linux's `exit_to_user_mode` does |

A contention only `:release` shows, such as a `..` meeting a rename on a shared Filesystem, is a lock's scope showing, not the GVL's.

## Lock Groups

Each lock belongs to one lock group, and the groups form an order with no cycle. So a group is checked alone, and only the edges between groups are checked together. `GroupOrder.tla` lists every lock a thread can wait on and every edge.

| Group | Holds | Tier |
|---|---|---|
| vfs | `i_rwsem`, `rename_mutex`, the copy-up lock, a dentry in lookup | sleeping |
| ruby | the GVL | GVL |
| dcache | an inode's dentry, the dentry tables, the rename lock, a dentry's state, the LRU | other |
| fsi | a Filesystem's own state | other |
| netg | the guest's open sockets, its connection budget | other |
| request | an exchange's or a lookup's future | other |
| sock | a socket's state, an exchange's state | other |

The leaf groups take nothing beyond one another, except init, whose initializers build a Filesystem's dentry cache.

| Leaf group | Holds |
|---|---|
| mem, lockprim, sched | the memory budget, a semaphore's holders, the waiters |
| cache, init | the wasmtime caches, a one-time initializer |
| mock, resolv, rleaf | the network Mock, a resolver's waker, the reactor's queues |
| sdk | the Rust SDK's registry and Handle tables |

### Group Order

The Filesystem and network groups meet only at the GVL, since a guest under `:hold` makes its network calls holding it. The reactor never takes the GVL or a request's future, so nothing leads back.

```
+----------+  callback  +----------+  holds  +----------+
| vfs      |----------->| ruby     |-------->| netg     |
+----------+            +----------+         +----------+
      |                                            |
      | holds                                      | polls
      v                                            v
+------------------+                         +----------+
| dcache, fsi (a)  |                         | request  |
+------------------+                         +----------+
                                                   |
                                                   | drives
                                                   v
                                             +----------+
                                             | sock     |
                                             +----------+
```

- (a) Each taken under vfs on its own; ruby takes a few of their locks without waiting too.

Every arrow means "takes the lower group while holding the upper", and every group ends in the leaf groups.

## Group Edges

A group edge is an edge that leaves a group. Each states what the group holds as it crosses, since a model that hides the crossing hides any cycle through it.

| Edge | Held while crossing | Rule kept |
|---|---|---|
| vfs to ruby | a sleeping lock | release before wait |
| ruby to netg, request | the GVL, under `:hold` | a request's future never calls Ruby |
| netg to request | the guest's open socket | a future never re-locks its own `task_future` |
| request to sock | the request's `task_future` | a socket takes only leaf locks |
| ruby to sock | the GVL, under `:hold` | a socket takes only leaf locks |
| netg to sock | the guest's open socket | a socket takes only leaf locks |

### Release Before Wait

A thread holding the GVL that must wait for a sleeping lock gives the GVL up first. The `Blocking` the Ruby gem registers does this for every wait, under [`:hold`](../guides/concurrency.md#gvl-modes) too. There a guest's thread has its caller give the GVL up for the wait.

Ruby wakes a waiting thread alike for an exception, a trap, or `Thread#wakeup`. The `Blocking` takes the GVL to tell them apart, then gives it up and waits on, unless the wakeup raised. It takes the GVL as a callback does, so the crossing is the same vfs to ruby edge. `Locks.tla` lets each waiting thread do this once.

```
Thread A        i_rwsem       Thread B          GVL
   | holds         |              |               |
   |-------------->|              |               |
   |               | asks         |               |
   |               |<-------------|               |
   |               |              | gives up      |
   |               |              |-------------->|
   | callback takes GVL           |               |
   |--------------------------------------------->|
   | releases      |              |               |
   |-------------->|              |               |
   |               | wakes        |               |
   |               |------------->|               |
```

Without it, thread B keeps the GVL that thread A's callback needs, and each waits on the other. `VfsCallbackHoldingGvl.tla` reaches that deadlock.

### Request Futures

An exchange or a lookup is a future that only the guest waiting on it polls, under its `task_future`. The reactor never polls one; it only wakes the request's wait queue. So the guest cancels its own request, and no other thread holds the future meanwhile.

| When the guest polls | It holds |
|---|---|
| on its first look, or a read that does not wait | the GVL, under `:hold` |
| after its wait wakes | nothing of Ruby, inside the `Blocking` |

| A request's future never | Since |
|---|---|
| calls Ruby (takes the GVL) | the GVL leads to it, so the group order would close a cycle |
| re-locks its own `task_future` | it would wait on itself |

`RequestPoll.tla` checks the guest's poll beside the reactor's wake.

## Group Obligations

A lock taken while another of the same kind is held is its group's obligation, since no edge between groups shows it. Each Context sets that order.

| Group | Obligation | Set in |
|---|---|---|
| vfs | ranks, ancestors first, addresses | [`filesystem/`](../filesystem/README.md#lock-order) |
| dcache | no rename lock taken inside a rename lock's write | [`filesystem/`](../filesystem/README.md#lock-order) |
| fsi | a Tmpfs directory before an entry it holds | [`filesystem/`](../filesystem/README.md#lock-order) |
| request | an exchange's future before its lookup's | [`network/`](../network/README.md#lock-order) |
| init | an initializer takes neither the GVL nor a sleeping lock | `GroupOrder.tla` |

## Host App Code

The Host App's own code is a lock group too, and the order holds only while it keeps to the rules in [Waits in Callbacks](../guides/concurrency.md#waits-in-callbacks).

```
Thread A         i_rwsem       Host Mutex       Thread B
   | holds          |              |               |
   |--------------->|              |               |
   |                |              | holds         |
   |                |              |<--------------|
   |                | waits        |               |
   |                |<-----------------------------|
   | callback waits |              |               |
   |------------------------------>|               |
```

`HostMutex.tla` reaches this deadlock, and `NestedRun.tla` the one two callbacks' runs reach through each other's Filesystems.

## Lifecycles

The order keeps threads from waiting on each other; a lifecycle keeps every waiter woken. An owner that ends what a thread waits on must wake that thread, or the thread waits until its run's deadline. The rows below and in Abnormal Exits are rules; code that breaks one is a defect.

| Owner | Ends | Wakes its waiters by |
|---|---|---|
| a `RwSemaphoreGuard` | its hold | releasing on drop |
| a walk holding a dentry in lookup | the lookup | `lookup_done` on drop, as Linux's `d_lookup_done` |
| a `SeqLock` write | the write | ending its count on drop, as `write_sequnlock` |
| the `Namespace` | every TCP socket left | aborting it, as `tcp_abort` |
| an exchange or a lookup | its future | cancelling it |
| the reactor | every socket and timer it watches | aborting each from a guard as its thread stops |

### Invocation Teardown

An Invocation ends what it holds as its Store drops, then the Frontend lets the run's Ruby objects go. Only the second order is load-bearing.

```
+------------------+
| Store drop (a)   |
+------------------+
         | then
         v
+------------------+
| Ruby pins (b)    |
+------------------+
```

- (a) Every open file, socket, exchange, and lookup goes, cancelling its future, and the `Namespace` aborts the TCP sockets left. Their order is free: only the guest polls a request, so nothing dials while the Store drops.
- (b) The Frontend holds the Ruby objects the run pinned until the Store is gone, so a Ruby Filesystem's `close` still reaches its Mount.

### Abnormal Exits

An owner must wake its waiters on every exit that runs code, a panic unwinding past it included. An exit that runs no code is kept away from a run instead.

| Exit | Runs drops | Rule |
|---|---|---|
| a panic unwinding | yes | the owner must wake its waiters from a guard's drop |
| a longjmp over Rust frames | no | Ruby runs only where no guest frame stands |
| a fork | no thread but the caller | the fork waits for every run to end, as [Fork Gate](#fork-gate) sets |
| a run's caller that never returns | no | collecting the caller ends its guest's waits |

No rule keeps a fork away from a background thread, the engine's own included. So a lock one takes after every run has ended is never one a forked child waits on. A process-local value, such as the reactor or the connection count, holds this. The child puts a lock of its own in place and never takes the parent's.

#### Fork Gate

A fork through Ruby waits for every run in progress to end, so the child inherits no lock a run held. A run on the forking Thread, another Fiber's included, would wait on itself, so the fork raises instead. The gate closes to new runs only once none is in progress, and opens again as the fork returns in the parent.

```
fork        prepare           runs
  |  (a)       |                |
  |----------->|  wait (b)      |
  |            |--------------->|
  |            |<---------------|
  |            |  close (c)     |
  |<-----------|                |
  |  open (d)  |                |
```

- (a) `Process._fork` and `Process.daemon` reach the prepare; a fork(2) another C extension makes does not.
- (b) The wait gives the GVL up, as any other wait does, and an interrupt ends it.
- (c) Closing while it waited would leave a run whose callback waits for another Thread's run that can no longer enter.
- (d) The child needs no opening: its gate is its own, as the run registry is process-local.

`ForkGate.tla` checks the close at zero, and `ForkGateCloseEarly.tla` reaches the deadlock of closing as the wait starts.

#### Collected Callers

A run's caller never returns when its Fiber is left suspended in a callback, or a longjmp passes over its frame.

| Step | What happens |
|---|---|
| nothing reaches the caller any more | the object it kept on its stack, `RunCaller`, goes too |
| the collector frees `RunCaller` | the guest is interrupted, and its calls into Ruby answer nothing |
| the guest ends | its locks go |

`RunCaller` marks the Ruby objects the run pinned rather than rooting them, so none of them keeps the Fiber alive.

## Models

The models check the group order and each edge's rule with TLC. A model of a broken rule must still deadlock, so each proves its rule holds the order up; each `.cfg` opens with its expected outcome.

| Model | Checks | Expect |
|---|---|---|
| `GroupOrder` | no cycle through the locks or the groups | ok |
| `VfsCallback` | vfs to ruby under release before wait | ok |
| `VfsCallbackHoldingGvl` | the same wait keeping the GVL | deadlock |
| `RequestPoll` | ruby to netg to request to sock, beside the reactor | ok |
| `HostMutex` | a callback waiting on a Host App Mutex | deadlock |
| `NestedRun` | a callback's run reaching back to another callback's Filesystem | deadlock |
| `Handoff` | a guest's thread served by its caller, with a nested run and an interrupt | ok |
| `HandoffThreadIdentity` | the same with each lock knowing its holder by thread | deadlock |
| `HandoffNoJoin` | the caller returning before its guest ends | deadlock |
| `ForkGate` | a fork's prepare beside a callback waiting for another Thread's run | ok |
| `ForkGateCloseEarly` | the same with the gate closing as the wait starts | deadlock |

`Locks.tla` is the thread model each edge's model builds on; `Handoff.tla` stands apart, as it models one run's two threads, and so does `ForkGate.tla`, as it models runs rather than locks.

### Model Checks

A change to a lock, an edge, or a rule runs the models again.

1. Download `tla2tools.jar` from the TLA+ tools release.
2. Set `TLA2TOOLS_JAR` to its path.
3. Run `bundle exec rake locking:check`.

A new edge goes into `GroupOrder.tla` first. A new rule gets a model where it holds and one where it breaks. Each model keeps to two threads and a few locks, so the whole check runs in seconds. A rule that needs more states is a design grown too complex.
