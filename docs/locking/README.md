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

A contention that shows only under `:release`, such as a `..` meeting another Sandbox's rename, is a lock's scope showing, not the GVL's.

## Lock Groups

Each lock belongs to one lock group, and the groups form an order with no cycle. So a group is checked alone, and only the edges between groups are checked together. `GroupOrder.tla` lists every lock a thread can wait on and every edge.

| Group | Holds | Tier |
|---|---|---|
| vfs | `i_rwsem`, `rename_mutex`, the copy-up lock, a dentry in lookup | sleeping |
| ruby | the GVL | GVL |
| dcache | an inode's dentry, the dentry tables, `RENAME_LOCK`, a dentry's state, the LRU | other |
| fsi | a Filesystem's own state | other |
| netg | the guest's open sockets, its connection budget | other |
| rtask | a reactor task's future | other |
| sock | a socket's state, an exchange's state | other |

The leaf groups take nothing beyond one another, except init, whose initializers build a Filesystem's dentry cache.

| Leaf group | Holds |
|---|---|
| mem, lockprim, sched | the memory budget, a semaphore's holders, the waiters |
| cache, init | the wasmtime caches, a one-time initializer |
| mock, resolv, rleaf | the network Mock, a resolver's waker, the reactor's queues |
| sdk | the Rust SDK's registry and Handle tables |

### Group Order

The Filesystem and network groups meet only at the GVL, since a guest under `:hold` makes its network calls holding it. The reactor never takes the GVL, so nothing leads back.

```
+----------+  callback  +----------+  holds  +----------+
| vfs      |----------->| ruby     |-------->| netg     |
+----------+            +----------+         +----------+
      |                                            |
      | holds                                      | cancels
      v                                            v
+------------------+                         +----------+
| dcache, fsi (a)  |                         | rtask    |
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
| ruby to netg, rtask | the GVL, under `:hold` | the reactor never takes the GVL |
| netg to rtask | the guest's open socket | a reactor future never re-locks its own `task_future` |
| rtask to sock | the task's `task_future` | a socket takes only leaf locks |

### Release Before Wait

A thread holding the GVL that must wait for a sleeping lock gives the GVL up first. The `Blocking` the Ruby gem registers does this for every wait, under [`:hold`](../guides/concurrency.md#gvl-modes) too.

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

### Reactor Futures

The reactor holds a task's `task_future` while it polls the future. The guest cancels a task by locking that `task_future`, without giving the GVL up.

| A reactor future never | Since |
|---|---|
| calls Ruby (takes the GVL) | the guest holds the GVL while it cancels |
| re-locks its own `task_future` | it would wait on itself |

`ReactorTakesGvl.tla` reaches the deadlock the first rule keeps away.

## Group Obligations

A lock taken while another of the same kind is held is its group's obligation, since no edge between groups shows it. Each Context sets that order.

| Group | Obligation | Set in |
|---|---|---|
| vfs | ranks, ancestors first, addresses | [`filesystem/`](../filesystem/README.md#lock-order) |
| dcache | `RENAME_LOCK` never taken again inside a write | [`filesystem/`](../filesystem/README.md#lock-order) |
| fsi | a Tmpfs directory before an entry it holds | [`filesystem/`](../filesystem/README.md#lock-order) |
| rtask | an exchange's task before its lookup's | [`network/`](../network/README.md#lock-order) |
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
| a walk holding a dentry in lookup | the lookup | `lookup_done`, as Linux's `d_lookup_done` |
| the `Namespace` | every TCP socket left | aborting it, as `tcp_abort` |
| an exchange | its reactor task | cancelling it |

### Invocation Teardown

An Invocation ends what it holds in the field order of its `Wasi`, then the Frontend lets the run's Ruby objects go. Both orders are load-bearing.

```
+------------------+
| Store drop       |
+------------------+
         | first
         v
+------------------+
| table (a)        |
+------------------+
         | then
         v
+------------------+
| network (b)      |
+------------------+
         | then
         v
+------------------+
| Ruby pins (c)    |
+------------------+
```

- (a) Every open file, socket, exchange, and lookup goes first, cancelling its reactor task.
- (b) The `Namespace` then aborts the TCP sockets left, so no task still dials into it.
- (c) The Frontend holds the Ruby objects the run pinned until the Store is gone, so a Ruby Filesystem's `close` still reaches its Mount.

### Abnormal Exits

An owner must wake its waiters on every exit that runs code, a panic unwinding past it included. An exit that runs no code is kept away from a run instead.

| Exit | Runs drops | Rule |
|---|---|---|
| a panic unwinding | yes | the owner must wake its waiters from a guard's drop |
| a longjmp over Rust frames | no | the guide keeps a raising trap off the main Thread |
| a fork | no thread but the caller | the guide keeps a fork outside any run |

No rule keeps a fork away from a background thread. So a lock one takes after every run has ended is never one a forked child waits on.

## Models

The models check the group order and each edge's rule with TLC. A model of a broken rule must still deadlock, so each proves its rule holds the order up; each `.cfg` opens with its expected outcome.

| Model | Checks | Expect |
|---|---|---|
| `GroupOrder` | no cycle through the locks or the groups | ok |
| `VfsCallback` | vfs to ruby under release before wait | ok |
| `VfsCallbackHoldingGvl` | the same wait keeping the GVL | deadlock |
| `ReactorCancel` | ruby to netg to rtask to sock | ok |
| `ReactorTakesGvl` | a reactor future taking the GVL | deadlock |
| `HostMutex` | a callback waiting on a Host App Mutex | deadlock |
| `NestedRun` | a callback's run reaching back to another callback's Filesystem | deadlock |

`Locks.tla` is the thread model each edge's model builds on.

### Model Checks

A change to a lock, an edge, or a rule runs the models again.

1. Download `tla2tools.jar` from the TLA+ tools release.
2. Set `TLA2TOOLS_JAR` to its path.
3. Run `bundle exec rake locking:check`.

A new edge goes into `GroupOrder.tla` first. A new rule gets a model where it holds and one where it breaks. Each model keeps to two threads and a few locks, so the whole check runs in seconds. A rule that needs more states is a design grown too complex.
