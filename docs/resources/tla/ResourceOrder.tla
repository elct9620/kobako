---- MODULE ResourceOrder ----
\* The whole's order: every edge resource allocation adds to GroupOrder, at
\* the grain of the locks, of the lock groups, and of the five tiers
\* fork gate > sleeping locks > reactor > GVL > other locks. With no cycle,
\* each tier is checked alone. Each forbidden edge, put in by itself, leads
\* upward: so the order is what forbids it. TLC checks both as it starts, as
\* GroupOrder does.
EXTENDS GroupOrder, Naturals

Forbidden == {"waitKeepsGvl", "gateKeepsGvl", "otherCallsRuby", "registryHeld",
              "waitsAgain", "reactorToSleeping", "gvlWaitsReactor"}

NewGroup == [fork_gate |-> "gate", runs |-> "runs"]
Group(l) == IF l \in DOMAIN NewGroup THEN NewGroup[l] ELSE GroupOf(l)
TierOfLock(l) == CASE Group(l) = "gate" -> "gate"
                   [] Group(l) = "vfs"  -> "sleeping"
                   [] Group(l) = "ruby" -> "gvl"
                   [] OTHER             -> "other"

Gone == {e \in Edges : "connection_budget" \in {e[1], e[2]}} \cup E("registry", {"ticker", "memcg"})
Resources ==
  \* fork gate: the gate and the runs' registry
     E("fork_gate", {"GVL", "runs", "fid_cache"})
  \cup E("runs", {"task_sleeping"})
  \* sleeping locks: a callback's nested run reaches the network
  \cup UNION {E(l, {"SharedFile", "task_future", "tcp_state"}) : l \in Groups.vfs}
  \* reactor: a guest's poll wakes itself
  \cup E("GVL", {"SharedFile", "task_future", "wq_waiters", "wq_tasks", "waiter_woken", "runs"})
  \cup E("task_future", {"wq_waiters", "wq_tasks", "waiter_woken"})
  \* other locks: the guest's memory as a :hold run grows it, the counts
  \cup E("GVL", {"memcg"})
  \cup E("GVL", {"tcp_state", "udp_state", "open_conn", "CURRENT", "sockets", "repoll", "query_ctx"})

BrokenLock(adds) ==
  CASE adds = "waitKeepsGvl"   -> E("GVL", {"i_rwsem"})
    [] adds = "gateKeepsGvl"   -> E("GVL", {"fork_gate"})
    [] adds = "otherCallsRuby" -> E("memcg", {"GVL"})
    [] adds = "registryHeld"   -> E("registry", {"fork_gate"}) \cup E("GVL", {"registry"})
    [] OTHER -> {}
BrokenTier(adds) ==
  CASE adds = "waitsAgain"        -> E("sleeping", {"gate"})
    [] adds = "reactorToSleeping" -> E("reactor", {"sleeping"})
    [] adds = "gvlWaitsReactor"   -> E("gvl", {"reactor"})
    [] OTHER -> {}

All(adds) == (Edges \ Gone) \cup Resources \cup BrokenLock(adds)
ByLock == {e \in All("none") : e[1] # e[2]}
ByGroup == {e \in {<<Group(d[1]), Group(d[2])>> : d \in All("none")} : e[1] # e[2]}
ByTier(adds) ==
  {e \in {<<TierOfLock(d[1]), TierOfLock(d[2])>> : d \in All(adds)} : e[1] # e[2]}
  \cup E("gate", {"sleeping", "reactor", "gvl", "other"}) \cup E("sleeping", {"reactor"})
  \cup E("reactor", {"other"})
  \cup BrokenTier(adds)
Level == [gate |-> 5, sleeping |-> 4, reactor |-> 3, gvl |-> 2, other |-> 1]

Downhill(adds) == \A e \in ByTier(adds) : Level[e[1]] > Level[e[2]]

ASSUME Acyclic(ByLock)
ASSUME Acyclic(ByGroup)
ASSUME Downhill("none")
ASSUME \A adds \in Forbidden : ~Downhill(adds)
====
