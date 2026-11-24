---- MODULE GroupOrder ----
\* Every lock in its lock group, and every edge "takes To while holding
\* From". TLC checks the ASSUMEs below as it starts: no cycle runs through
\* the locks, and none through the groups, so each group can be checked alone
\* and only the edges between groups together.
\*
\* An edge the code never waits on is left out: a trylock (rename_lock ->
\* rename_mutex, i_rwsem in __d_unalias), and a wait that lets the GVL go first
\* (GVL -> i_rwsem, in_lookup). A lock taken while another of its kind is held
\* is its group's own obligation, kept by the order its Context document sets.
\*
\* The layer_* locks belong to a Filesystem an Overlay stacks on, whose locks
\* come after the Overlay's, as Linux's directory-locking ranks filesystems.
EXTENDS FiniteSets, TLC

Groups == [
  vfs      |-> {"rename_mutex", "i_rwsem", "ovl_lock", "in_lookup",
                "layer_rename_mutex", "layer_i_rwsem", "layer_in_lookup"},
  \* Ruby's own interrupt_lock, under which it calls a run's unblocking function.
  ruby     |-> {"GVL", "interrupt_lock"},
  dcache   |-> {"inode_dentry", "inode_hashtable", "dentry_hashtable",
                "in_lookup_hashtable", "rename_lock", "dentry_state", "lru"},
  lockprim |-> {"rwsem_holders"},
  fsi      |-> {"tmpfs_state", "tmpfs_content", "whiteout_times", "pipe_state",
                "OpenRubyFile", "open_fids", "open_dir_fid",
                "ovl_level", "ovl_real", "ovl_unfinished"},
  mem      |-> {"memcg"},
  sched    |-> {"task_sleeping", "waiter_woken", "wq_waiters", "wq_tasks"},
  \* The process's caches: the wasmtime ones, and the directories Binds keep.
  cache    |-> {"module_cache", "instance_pre_cache", "ticker", "fid_cache"},
  \* An initializer one thread runs while the rest wait for it, as a
  \* OnceLock or LazyLock is; one takes neither the GVL nor a sleeping lock.
  \* One that builds a plain value and takes no lock is left out.
  init     |-> {"platform_once", "mnt_once"},
  netg     |-> {"SharedFile", "connection_budget", "exchange_task", "translations",
                "exchange_place", "exchange_memory", "fields_list", "request_body"},
  request  |-> {"task_future"},
  sock     |-> {"tcp_state", "udp_state", "exchange_state", "lookup_answer",
                "bindhash", "open_conn", "tcp_sockets"},
  mock     |-> {"mock_state"},
  resolv   |-> {"ready_waker", "query_ctx"},
  rleaf    |-> {"CURRENT", "sockets", "repoll", "timers"},
  \* The Rust SDK's own, which the Ruby gem does not link.
  sdk      |-> {"registry", "handle_table"},
  \* A fork's gate, closed from its check to its return. Asking at it is no
  \* edge: a fork never waits for it, and a run waits holding nothing.
  gate     |-> {"fork_gate"},
  \* The runs in progress, which a fork checks and a run enters.
  runs     |-> {"RUNS"}
]

LockSet == UNION {Groups[g] : g \in DOMAIN Groups}
GroupOf(l) == CHOOSE g \in DOMAIN Groups : l \in Groups[g]

E(from, tos) == {<<from, to>> : to \in tos}

Edges ==
  \* vfs: lock_rename, lock_two_directories, lookup under the directory, copy up
     E("rename_mutex", {"i_rwsem", "GVL"})
  \cup E("rename_mutex", {"rename_lock"})
  \cup E("i_rwsem", {"i_rwsem", "in_lookup", "ovl_lock", "GVL", "rwsem_holders",
                     "rename_lock",
                     "inode_dentry", "inode_hashtable", "dentry_hashtable",
                     "dentry_state", "lru", "tmpfs_state", "tmpfs_content",
                     "pipe_state", "OpenRubyFile", "fid_cache", "open_fids",
                     "open_dir_fid", "ovl_level", "ovl_real", "ovl_unfinished"})
  \cup E("in_lookup", {"GVL", "inode_dentry", "inode_hashtable", "ovl_lock",
                       "layer_rename_mutex", "layer_i_rwsem", "ovl_level",
                       "fid_cache", "open_fids", "open_dir_fid"})
  \cup E("layer_in_lookup", {"GVL", "inode_dentry", "inode_hashtable"})
  \* A Layer may be a Ruby Filesystem, which copy up reads under ovl_lock.
  \cup E("ovl_lock", {"layer_rename_mutex", "layer_i_rwsem", "ovl_level", "ovl_real",
                      "ovl_unfinished", "tmpfs_content", "memcg", "GVL"})
  \cup E("layer_rename_mutex", {"layer_i_rwsem"})
  \cup E("layer_i_rwsem", {"layer_i_rwsem", "layer_in_lookup", "inode_dentry", "dentry_hashtable",
                           "tmpfs_state", "tmpfs_content", "GVL"})
  \* dcache: d_splice_alias records an inode under i_lock, then hashes it
  \cup E("inode_dentry", {"dentry_state", "dentry_hashtable"})
  \* fsi: a Tmpfs directory before an entry it holds, which is a tree
  \cup E("tmpfs_state", {"tmpfs_state", "tmpfs_content", "whiteout_times", "dentry_state"})
  \* init: a socket's or pipe's Filesystem built on first use
  \cup E("mnt_once", {"inode_dentry", "dentry_state"})
  \* ruby: what a thread holding the GVL takes without letting it go, as
  \* lookup_fast reads the dentry cache with no directory lock
  \cup E("GVL", {"rwsem_holders", "inode_dentry", "dentry_hashtable",
                 "in_lookup_hashtable", "rename_lock", "dentry_state", "lru",
                 "tmpfs_state", "tmpfs_content", "OpenRubyFile", "module_cache",
                 "instance_pre_cache", "ticker", "task_sleeping", "platform_once",
                 "mnt_once", "whiteout_times", "pipe_state", "fid_cache", "open_fids",
                 "open_dir_fid", "ovl_level", "ovl_real", "ovl_unfinished"})
  \cup E("interrupt_lock", {"task_sleeping"})
  \* ruby: a guest under :hold makes every network call with the GVL held
  \cup E("GVL", {"SharedFile", "connection_budget", "translations", "fields_list",
                 "request_body", "exchange_task", "task_future", "exchange_state",
                 "exchange_place", "exchange_memory", "mock_state"})
  \* ruby: a guest under :hold grows its Store with the GVL held, as one run
  \* counts as one thread
  \cup E("GVL", {"memcg"})
  \* dcache: d_move, __d_unalias, d_lookup_done, retain_dentry
  \cup E("dentry_hashtable", {"rename_lock", "in_lookup_hashtable", "dentry_state"})
  \cup E("rename_lock", {"dentry_state", "rwsem_holders"})
  \cup E("in_lookup_hashtable", {"dentry_state"})
  \cup E("dentry_state", {"lru"})
  \* sched and wait: an interrupt or a wake_up reaching a sleeping waiter
  \cup E("task_sleeping", {"waiter_woken"})
  \cup E("wq_waiters", {"waiter_woken"})
  \* netg: the guest thread's side
  \cup E("SharedFile", {"tcp_state", "udp_state", "exchange_state", "lookup_answer",
                        "wq_waiters", "wq_tasks", "exchange_task", "task_future",
                        "request_body", "pipe_state"})
  \cup E("exchange_place", {"open_conn"})
  \cup E("exchange_memory", {"memcg"})
  \cup E("fields_list", {"memcg"})
  \cup E("request_body", {"memcg"})
  \cup E("connection_budget", {"tcp_state", "udp_state", "CURRENT", "sockets",
                               "exchange_task", "mock_state", "open_conn", "memcg", "repoll",
                               "task_future", "query_ctx"})
  \cup E("translations", {"memcg"})
  \* request: what a request's future takes as the guest waiting on it polls it
  \cup E("task_future", {"task_future", "tcp_state", "exchange_state", "lookup_answer",
                         "wq_waiters", "wq_tasks", "mock_state", "ready_waker",
                         "CURRENT", "timers", "sockets", "tcp_sockets", "open_conn", "memcg",
                         "query_ctx"})
  \* sock: a socket registering on the reactor
  \cup E("tcp_state", {"CURRENT", "sockets", "mock_state", "bindhash", "open_conn", "memcg"})
  \cup E("udp_state", {"mock_state", "repoll", "sockets", "bindhash", "open_conn", "memcg"})
  \* sock: an exchange charging its body as a frame arrives or the guest reads
  \cup E("exchange_state", {"memcg"})
  \* mock, resolv: a wake made under the lock reaches the request's wait queue
  \cup E("mock_state", {"wq_waiters", "wq_tasks"})
  \cup E("ready_waker", {"wq_waiters", "wq_tasks"})
  \cup E("query_ctx", {"wq_waiters", "wq_tasks"})
  \* sdk: sealing a Sandbox compiles its catalog
  \cup E("registry", {"ticker", "memcg"})
  \* gate: inside its gate a fork may give the GVL up and take it again, and
  \* lets go of the directories Binds keep before it opens
  \cup E("fork_gate", {"GVL", "RUNS", "fid_cache"})
  \* runs: a run enters under the GVL, a waiter queueing on the gate
  \cup E("GVL", {"RUNS"})
  \cup E("RUNS", {"wq_waiters"})
  \* cache: a Bind keeps or lets go a directory under whichever Filesystem,
  \* dentry, or GVL lock its call holds, and takes nothing more
  \cup UNION {E(l, {"fid_cache"}) :
              l \in (Groups.vfs \cup Groups.dcache \cup Groups.fsi \cup {"GVL"})}

\* Every node an edge of R leads to from X, X itself included.
RECURSIVE Reach(_, _)
Reach(R, X) == LET Y == X \cup {r[2] : r \in {s \in R : s[1] \in X}}
               IN IF Y = X THEN X ELSE Reach(R, Y)

\* No node reaches itself again.
Acyclic(R) == \A r \in R : r[1] \notin Reach(R, {r[2]})

LockEdges == {e \in Edges : e[1] # e[2]}
GroupEdges == {<<GroupOf(e[1]), GroupOf(e[2])>> : e \in Edges} \ {<<g, g>> : g \in DOMAIN Groups}

ASSUME \A g, h \in DOMAIN Groups : g # h => Groups[g] \cap Groups[h] = {}
ASSUME \A e \in Edges : e[1] \in LockSet /\ e[2] \in LockSet
ASSUME Acyclic(LockEdges)
ASSUME Acyclic(GroupEdges)

VARIABLE checked
Spec == checked = TRUE /\ [][UNCHANGED checked]_checked
====
