---- MODULE Assembly ----
\* The whole: the five tiers, each only what its own model proves.
\*   StarvationFreedom  who waits gets it. Assumed as SF of taking, which a
\*                      proved "waits ~> has" implies; no order is assumed.
\*   ForwardProgress    who holds, waiting for nothing below, lets go: WF of
\*                      letting go.
\*   LockOrdering       ResourceOrder's, broken here to show why it is needed.
\* "v" is the victim under :hold. "h" and "g" are hostile runs under :release
\* that repeat for ever. "n" has no timeout: it stays past the gate and comes
\* back to the tiers below for ever, so each tier has a rival: for the GVL it
\* skips the reactor, which the victim holds as it waits there.
\* The gate holds every run at once: a run waits there only for a fork, which
\* refuses rather than waits, as locking's ForkGate proves. No deadline is here.
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Breaks, At

Runs == {"v", "h", "g", "n"}
Path(r) ==
  CASE Breaks = "lockOrdering" /\ r = "h" -> <<"gate", "reactor", "sleeping", "other">>
    [] r = "v" -> <<"gate", "sleeping", "reactor", "gvl", "other">>
    [] r = "h" -> <<"gate", "sleeping", "other">>
    [] r = "g" -> <<"gate", "reactor", "other">>
    [] r = "n" /\ At = "gvl" -> <<"gate", "gvl", "other">>
    [] r = "n" -> <<"gate", "reactor", "gvl", "other">>
Room(m) == IF m = "gate" THEN Cardinality(Runs) ELSE 1
NeverEnds(r) == r = "n"

VARIABLES held, down
vars == <<held, down>>

Holders(m) == {r \in Runs : \E i \in 1..held[r] : Path(r)[i] = m}
Top(r) == Path(r)[held[r]]
Wanted(r) == Path(r)[held[r] + 1]

Init == held = [r \in Runs |-> 0] /\ down = [r \in Runs |-> FALSE]
Take(r) ==
  /\ ~down[r] /\ held[r] < Len(Path(r))
  /\ Cardinality(Holders(Wanted(r))) < Room(Wanted(r))
  /\ held' = [held EXCEPT ![r] = @ + 1]
  /\ down' = [down EXCEPT ![r] = held[r] + 1 = Len(Path(r))]
LetGo(r) ==
  /\ down[r]
  /\ ~(NeverEnds(r) /\ held[r] = 1)
  /\ ~(Breaks = "forwardProgress" /\ r # "v" /\ Top(r) = At)
  /\ held' = [held EXCEPT ![r] = @ - 1]
  /\ down' = [down EXCEPT ![r] = held[r] > 1]
Again(r) ==
  /\ down[r] /\ held[r] = 1 /\ NeverEnds(r)
  /\ down' = [down EXCEPT ![r] = FALSE] /\ UNCHANGED held
Next == \E r \in Runs : Take(r) \/ LetGo(r) \/ Again(r)

Unfair(r) == Breaks = "starvationFreedom" /\ ~down[r] /\ held[r] < Len(Path(r)) /\ Wanted(r) = At
Spec == /\ Init /\ [][Next]_vars
        /\ \A r \in Runs : /\ SF_vars(Take(r) /\ ~Unfair(r))
                           /\ WF_vars(Take(r) /\ Unfair(r))
                           /\ WF_vars(LetGo(r))

NeverJammed == []<>(down["v"] /\ held["v"] = 1)
====
