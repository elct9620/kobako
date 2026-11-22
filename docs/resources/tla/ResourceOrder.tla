---- MODULE ResourceOrder ----
\* The whole's order: every edge resource allocation adds to GroupOrder, at
\* the grain of the locks, of the lock groups, and of the five tiers
\* admission > sleeping locks > reactor > GVL > other locks. With no cycle,
\* each tier is checked alone. Each forbidden edge, put in by itself, leads
\* upward: so the order is what forbids it. TLC checks both as it starts, as
\* GroupOrder does.
EXTENDS GroupOrder, Naturals

Forbidden == {"waitKeepsGvl", "admissionKeepsGvl", "otherCallsRuby", "registryHeld",
              "waitsAgain", "reactorToSleeping"}

NewGroup == [fork_gate |-> "gate", runs |-> "runs", ledger |-> "account", fid_cache |-> "cache"]
Group(l) == IF l \in DOMAIN NewGroup THEN NewGroup[l] ELSE GroupOf(l)
TierOfLock(l) == CASE Group(l) = "gate" -> "admission"
                   [] Group(l) = "vfs"  -> "sleeping"
                   [] Group(l) = "ruby" -> "gvl"
                   [] OTHER             -> "other"

Gone == {e \in Edges : "connection_budget" \in {e[1], e[2]}} \cup E("registry", {"ticker", "memcg"})
Resources ==
  \* admission: the gate, the runs' registry, the totals' ledger
     E("fork_gate", {"GVL", "runs", "fid_cache"})
  \cup E("runs", {"task_sleeping"})
  \cup UNION {E(l, {"ledger"}) :
              l \in {"GVL", "i_rwsem", "tmpfs_content", "tmpfs_state", "fid_cache", "lru", "memcg",
                     "open_conn", "SharedFile", "task_future", "tcp_state", "udp_state",
                     "exchange_state", "exchange_place", "sockets", "tcp_sockets", "query_ctx"}}
  \* sleeping locks: a callback's nested run reaches the network
  \cup UNION {E(l, {"SharedFile", "task_future", "tcp_state"}) : l \in Groups.vfs}
  \* reactor: a guest's poll wakes itself
  \cup E("GVL", {"SharedFile", "task_future", "wq_waiters", "wq_tasks", "waiter_woken", "runs"})
  \cup E("task_future", {"wq_waiters", "wq_tasks", "waiter_woken"})
  \* other locks: the memory, the counts, the fid cache
  \cup UNION {E(l, {"memcg"}) :
              l \in {"GVL", "i_rwsem", "layer_i_rwsem", "in_lookup", "layer_in_lookup",
                     "in_lookup_hashtable", "dentry_state"}}
  \cup E("GVL", {"tcp_state", "udp_state", "open_conn", "CURRENT", "sockets", "repoll", "query_ctx"})
  \cup UNION {E(l, {"fid_cache"}) :
              l \in (Groups.vfs \cup Groups.dcache \cup Groups.fsi \cup {"GVL"}) \ {"fid_cache"}}

BrokenLock(adds) ==
  CASE adds = "waitKeepsGvl"   -> E("GVL", {"i_rwsem"})
    [] adds = "admissionKeepsGvl"  -> E("GVL", {"fork_gate"})
    [] adds = "otherCallsRuby" -> E("ledger", {"GVL"})
    [] adds = "registryHeld"   -> E("registry", {"fork_gate"}) \cup E("GVL", {"registry"})
    [] OTHER -> {}
BrokenTier(adds) ==
  CASE adds = "waitsAgain"        -> E("sleeping", {"admission"})
    [] adds = "reactorToSleeping" -> E("reactor", {"sleeping"})
    [] OTHER -> {}

All(adds) == (Edges \ Gone) \cup Resources \cup BrokenLock(adds)
ByLock == {e \in All("none") : e[1] # e[2]}
ByGroup == {e \in {<<Group(d[1]), Group(d[2])>> : d \in All("none")} : e[1] # e[2]}
ByTier(adds) ==
  {e \in {<<TierOfLock(d[1]), TierOfLock(d[2])>> : d \in All(adds)} : e[1] # e[2]}
  \cup E("admission", {"sleeping", "reactor", "gvl", "other"}) \cup E("sleeping", {"reactor"})
  \cup E("reactor", {"other"})
  \cup BrokenTier(adds)
Level == [admission |-> 5, sleeping |-> 4, reactor |-> 3, gvl |-> 2, other |-> 1]

Downhill(adds) == \A e \in ByTier(adds) : Level[e[1]] > Level[e[2]]

ASSUME Acyclic(ByLock)
ASSUME Acyclic(ByGroup)
ASSUME Downhill("none")
ASSUME \A adds \in Forbidden : ~Downhill(adds)
====
