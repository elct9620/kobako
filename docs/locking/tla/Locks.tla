---- MODULE Locks ----
\* Threads that each run a script of lock steps, so TLC can try every
\* interleaving and report a state where no thread can move: a deadlock.
\*
\*   <<"acq", l>>      take l, waiting while another thread holds it
\*   <<"rel", l>>      let go of l
\*   <<"acqR", l, r>>  take l; where another thread holds it, let go of r
\*                     first and wait, then take l, then wait to take r back,
\*                     as a wait through the registered Blocking gives up the
\*                     GVL and takes it back once the lock is held; while it
\*                     waits for l, it may take r and let it go once, as the
\*                     Blocking takes the GVL to handle a signal
EXTENDS Naturals, Sequences

CONSTANTS Threads, LockSet, NoOne, Scripts

VARIABLES owner, pc, dropped, signalled
vars == <<owner, pc, dropped, signalled>>

Init == /\ owner = [l \in LockSet |-> NoOne]
        /\ pc = [t \in Threads |-> 1]
        /\ dropped = [t \in Threads |-> NoOne]
        /\ signalled = [t \in Threads |-> FALSE]

Done(t) == pc[t] > Len(Scripts[t])

Take(t, l) == /\ owner[l] = NoOne
              /\ owner' = [owner EXCEPT ![l] = t]
              /\ pc' = [pc EXCEPT ![t] = @ + 1]
              /\ UNCHANGED <<dropped, signalled>>

Step(t) ==
  /\ ~Done(t)
  /\ LET op == Scripts[t][pc[t]] IN
     CASE op[1] = "acq" -> Take(t, op[2])
       [] op[1] = "rel" ->
            /\ owner' = [owner EXCEPT ![op[2]] = NoOne]
            /\ pc' = [pc EXCEPT ![t] = @ + 1]
            /\ UNCHANGED <<dropped, signalled>>
       [] op[1] = "acqR" ->
            \/ dropped[t] = NoOne /\ Take(t, op[2])
            \/ /\ dropped[t] = NoOne
               /\ owner[op[2]] # NoOne
               /\ owner[op[3]] = t
               /\ owner' = [owner EXCEPT ![op[3]] = NoOne]
               /\ dropped' = [dropped EXCEPT ![t] = op[3]]
               /\ UNCHANGED <<pc, signalled>>
            \/ /\ dropped[t] = op[3]
               /\ owner[op[2]] = NoOne
               /\ owner[op[3]] # t
               /\ owner' = [owner EXCEPT ![op[2]] = t]
               /\ UNCHANGED <<pc, dropped, signalled>>
            \/ /\ dropped[t] = op[3]
               /\ owner[op[2]] = t
               /\ owner[op[3]] = NoOne
               /\ owner' = [owner EXCEPT ![op[3]] = t]
               /\ dropped' = [dropped EXCEPT ![t] = NoOne]
               /\ pc' = [pc EXCEPT ![t] = @ + 1]
               /\ UNCHANGED signalled
            \* Handling a signal: take r while still waiting for l ...
            \/ /\ dropped[t] = op[3]
               /\ owner[op[2]] # t
               /\ owner[op[3]] = NoOne
               /\ ~signalled[t]
               /\ owner' = [owner EXCEPT ![op[3]] = t]
               /\ signalled' = [signalled EXCEPT ![t] = TRUE]
               /\ UNCHANGED <<pc, dropped>>
            \* ... and let it go again before waiting on.
            \/ /\ dropped[t] = op[3]
               /\ owner[op[2]] # t
               /\ owner[op[3]] = t
               /\ owner' = [owner EXCEPT ![op[3]] = NoOne]
               /\ UNCHANGED <<pc, dropped, signalled>>

\* Every script ran to its end; stuttering here is no deadlock.
Finished == /\ \A t \in Threads : Done(t)
            /\ UNCHANGED vars

Next == (\E t \in Threads : Step(t)) \/ Finished
Spec == Init /\ [][Next]_vars
====
