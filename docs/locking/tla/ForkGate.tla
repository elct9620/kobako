---- MODULE ForkGate ----
\* The run gate: a fork raises at once while any run is in progress, and else
\* closes the gate in the same step. The one wait left is a run entering while
\* a fork is under way: it gives the GVL up, and the fork that opens the gate
\* lets it in, in that same step. A run is in progress until its guest ends,
\* its caller collected or not. Closing has one step more: the fork lets go of
\* the directories Binds keep under the cache's leaf lock and closes them with
\* it let go, the GVL kept and no Ruby called.
\*
\* Threads a and b run Sandboxes; f, and g where two overlap, fork. The state
\* chooses the GVL mode and what a's callback does, so one check covers each:
\*   returns    nothing that waits
\*   inner      a run nested on the same Fiber
\*   run        waits for b's run, a nested run on another Thread
\*   forker     waits for f, which forks
\*   collected  never returns: the collector frees the caller
\* Under :hold a computing run keeps the GVL until its caller yields it at the
\* end of its quantum. Breaks takes one rule away.
\*
\* Assumes, the fairness at the end, is all this module takes from others.
EXTENDS Naturals, FiniteSets

CONSTANTS
  Breaks,   \* "none", or the rule taken away
  Forkers,  \* {"f"}, or {"f", "g"}
  Again,    \* each Thread starts over, for ever
  Stops,    \* a wait may be interrupted, which is never counted as progress
  Hosts,    \* what Caller's callback may do
  Modes,    \* the GVL modes
  Runs,     \* the Threads that run Sandboxes
  Caller,   \* the one of them that calls back, as a above
  Other     \* the one a callback may wait for, as b above; the victim

Threads == Runs \cup Forkers
NoOne == "none"

VARIABLES mode, host, gvl, pc, reg, closed
vars == <<mode, host, gvl, pc, reg, closed>>
same == UNCHANGED <<mode, host>>

Init == /\ mode \in Modes
        /\ host \in Hosts
        /\ gvl = NoOne
        /\ pc = [t \in Threads |-> "idle"]
        /\ reg = {}
        /\ closed = 0

\* The GVL is free to t, or already t's.
Gvl(t) == gvl \in {NoOne, t}
To(t, s) == pc' = [pc EXCEPT ![t] = s]
Computes(r) == IF mode = "hold" THEN r ELSE NoOne
\* What a forked child would inherit a lock of.
Guest(r) == pc[r] \in {"run", "callback", "inner", "resumed", "ending"}
Waiting(r) == pc[r] \in {"wait", "woken"}

----
\* The gate's rules, for the integration to use as they stand.

MayEnter(forks) == forks = 0
MayFork(inProgress) == inProgress = {}
\* Who enters in the step that leaves the gate with `forks`: of those that
\* wait, whom admission has room for.
LetsIn(forks, waiters, fits) == IF MayEnter(forks) THEN waiters \cap fits ELSE {}

----
\* A run.

Ask(r) ==
  /\ pc[r] = "idle" /\ gvl = NoOne
  /\ IF MayEnter(closed)
       THEN /\ To(r, "run") /\ reg' = reg \cup {r} /\ gvl' = Computes(r)
       ELSE /\ To(r, "wait") /\ UNCHANGED reg
            /\ gvl' = IF Breaks = "waiterKeepsGvl" THEN r ELSE NoOne
  /\ UNCHANGED closed /\ same

\* Only where opening wakes and does not hand over: the woken looks again,
\* holding no GVL, as do_poll's poll does.
Recheck(r) ==
  /\ pc[r] = "woken"
  /\ IF MayEnter(closed) THEN To(r, "handed") /\ reg' = reg \cup {r}
                   ELSE To(r, "wait") /\ UNCHANGED reg
  /\ UNCHANGED <<gvl, closed>> /\ same

TakeBack(r) ==
  /\ pc[r] = "handed" /\ Gvl(r)
  /\ To(r, "run") /\ gvl' = Computes(r)
  /\ UNCHANGED <<reg, closed>> /\ same

Interrupted(r) ==
  /\ Stops /\ (Waiting(r) \/ pc[r] = "handed") /\ Gvl(r)
  /\ To(r, "out") /\ gvl' = NoOne
  /\ reg' = IF Breaks = "stoppedKeeps" THEN reg ELSE reg \ {r}
  /\ UNCHANGED closed /\ same

Yield(r) ==
  /\ pc[r] \in {"run", "resumed"} /\ gvl = r /\ Breaks # "gvlNeverYields"
  /\ gvl' = NoOne
  /\ UNCHANGED <<pc, reg, closed>> /\ same

Call(r) ==
  /\ r = Caller /\ pc[r] = "run" /\ host # "returns" /\ Gvl(r)
  /\ To(r, "callback") /\ gvl' = NoOne
  /\ UNCHANGED <<reg, closed>> /\ same

\* A nested run on the same Fiber is found in progress, so it never looks at
\* the gate.
Inner(r) ==
  /\ pc[r] = "callback" /\ host = "inner" /\ Gvl(r)
  /\ To(r, "inner") /\ gvl' = NoOne
  /\ UNCHANGED <<reg, closed>> /\ same

Return(r) ==
  /\ Gvl(r)
  /\ \/ pc[r] = "inner"
     \/ /\ pc[r] = "callback"
        /\ \/ host = "run" /\ pc[Other] \in {"done", "out"}
           \/ host = "forker" /\ \E f \in Forkers : pc[f] = "done"
  /\ To(r, "resumed") /\ gvl' = Computes(r)
  /\ UNCHANGED <<reg, closed>> /\ same

End(r) ==
  /\ pc[r] \in {"run", "resumed"} /\ Gvl(r)
  /\ r = Caller /\ pc[r] = "run" => host = "returns"
  /\ To(r, "done") /\ gvl' = NoOne /\ reg' = reg \ {r}
  /\ UNCHANGED closed /\ same

\* A guest may end just before the collector frees the caller that left it.
EndsFirst(r) ==
  /\ pc[r] = "callback" /\ host = "collected"
  /\ To(r, "endedFirst")
  /\ UNCHANGED <<gvl, reg, closed>> /\ same

Collect(r) ==
  /\ gvl = NoOne
  /\ \/ /\ pc[r] = "callback" /\ host = "collected"
        /\ To(r, "ending")
        /\ reg' = IF Breaks = "collectedEarly" THEN reg \ {r} ELSE reg
     \/ /\ pc[r] = "endedFirst"
        /\ To(r, "done")
        /\ reg' = IF Breaks = "countsEnded" THEN reg ELSE reg \ {r}
  /\ UNCHANGED <<gvl, closed>> /\ same

\* The guest's own thread ends it, taking only a leaf lock.
GuestEnds(r) ==
  /\ pc[r] = "ending"
  /\ To(r, "done") /\ reg' = reg \ {r}
  /\ UNCHANGED <<gvl, closed>> /\ same

----
\* A fork.

\* A count no greater than the forks there are: only a broken rule reaches
\* the bound, which keeps its states finite.
Closed == IF closed < Cardinality(Forkers) THEN closed + 1 ELSE closed

Prepare(f) ==
  /\ pc[f] = "idle" /\ gvl = NoOne
  /\ IF Breaks = "shedBeforeClose"
       THEN To(f, "shed") /\ gvl' = f /\ UNCHANGED closed
     ELSE IF \/ ~MayFork(reg)
        \/ /\ Breaks = "refusesPastWaiter"  \* a candidate, not the rule
           /\ closed > 0 /\ \E r \in Runs : Waiting(r)
       THEN /\ To(f, IF Breaks = "waits" THEN "waitRuns" ELSE "done")
            /\ gvl' = NoOne /\ UNCHANGED closed
       ELSE IF Breaks = "checkApart"
         THEN /\ To(f, "checked") /\ gvl' = NoOne /\ UNCHANGED closed
         ELSE /\ To(f, "shed") /\ gvl' = f /\ closed' = Closed
  /\ UNCHANGED reg /\ same

\* Under the cache's lock, which no guest can hold here; then outside it.
ShedTakes(f) ==
  /\ pc[f] = "shed"
  /\ To(f, "shedCloses")
  /\ UNCHANGED <<gvl, reg, closed>> /\ same
ShedCloses(f) ==
  /\ pc[f] = "shedCloses"
  /\ To(f, "hook")
  /\ UNCHANGED <<gvl, reg, closed>> /\ same

WaitedRuns(f) ==
  /\ pc[f] = "waitRuns" /\ gvl = NoOne /\ reg = {}
  /\ To(f, "shed") /\ gvl' = f /\ closed' = Closed
  /\ UNCHANGED reg /\ same

CloseApart(f) ==
  /\ pc[f] = "checked" /\ gvl = NoOne
  /\ To(f, "shed") /\ gvl' = f /\ closed' = Closed
  /\ UNCHANGED reg /\ same

\* Code inside the gate may give the GVL up: an inner hook's IO, or a switch
\* as Ruby checks interrupts.
HookGivesGvl(f) ==
  /\ pc[f] = "hook" /\ gvl = f /\ Breaks # "hookKeepsGvl"
  /\ gvl' = NoOne
  /\ UNCHANGED <<pc, reg, closed>> /\ same

\* The child runs on only where no guest held a lock.
Fork(f) ==
  /\ pc[f] = "hook" /\ Gvl(f)
  /\ Breaks = "hookWaits" => pc[Other] = "done"
  /\ IF \E r \in Runs : Guest(r) THEN To(f, "childStuck") /\ gvl' = NoOne
                                 ELSE To(f, "forked") /\ gvl' = f
  /\ UNCHANGED <<reg, closed>> /\ same

HookRaises(f) ==
  /\ pc[f] = "hook" /\ Gvl(f) /\ Breaks # "hookWaits"
  /\ To(f, "raised") /\ gvl' = f
  /\ UNCHANGED <<reg, closed>> /\ same

Opened == IF closed = 0 \/ Breaks = "opensEarly" THEN 0 ELSE closed - 1

Open(f) ==
  /\ pc[f] \in {"forked", "raised"} /\ Gvl(f)
  /\ gvl' = NoOne
  /\ IF Breaks = "neverOpens" /\ pc[f] = "raised"
       THEN To(f, "done") /\ UNCHANGED <<reg, closed>>
       ELSE LET left == Opened
                lets == LetsIn(left, {r \in Runs : Waiting(r)}, Runs)
                wakes == Breaks = "wakeOnly"
            IN /\ closed' = left
               /\ reg' = IF wakes THEN reg ELSE reg \cup lets
               /\ pc' = [t \in Threads |->
                           IF t = f THEN "done"
                           ELSE IF t \in lets THEN (IF wakes THEN "woken" ELSE "handed")
                           ELSE pc[t]]
  /\ same

----
StartsOver(t) ==
  /\ Again /\ pc[t] \in {"done", "out"}
  /\ To(t, "idle")
  /\ UNCHANGED <<gvl, reg, closed>> /\ same

Finished == /\ ~Again
            /\ \A t \in Threads : pc[t] \in {"done", "out"}
            /\ reg = {} /\ closed = 0 /\ gvl = NoOne
            /\ UNCHANGED vars

RunStep(r) == \/ Ask(r) \/ Recheck(r) \/ TakeBack(r) \/ Interrupted(r) \/ Yield(r)
              \/ Call(r) \/ Inner(r) \/ Return(r) \/ End(r)
              \/ EndsFirst(r) \/ Collect(r) \/ GuestEnds(r)
ForkStep(f) == \/ Prepare(f) \/ WaitedRuns(f) \/ CloseApart(f) \/ HookGivesGvl(f)
               \/ ShedTakes(f) \/ ShedCloses(f)
               \/ Fork(f) \/ HookRaises(f) \/ Open(f)

Next == \/ \E r \in Runs : RunStep(r)
        \/ \E f \in Forkers : ForkStep(f)
        \/ \E t \in Threads : StartsOver(t)
        \/ Finished

Spec == Init /\ [][Next]_vars

----
\* What is assumed, and no more: no interrupt, no deadline, no winning of a
\* race to an open gate, and no run that chooses to end or call back.
\*
\*   below  each wait for the GVL ends, as its LetsGo has it: a computing
\*          caller yields, and a step needing the GVL is in the end taken
\*   OS     a thread that needs nothing is scheduled; a guest whose caller
\*          was collected is interrupted by kobako, and so ends
\*   host   the boundary: code inside the gate returns or raises
\*   above  nothing: runs may compute or stay in a callback for ever, forks
\*          and runs may repeat for ever, a collection may never come
\*
\* One strong fairness for each thread stands for its every step that needs
\* the GVL: stuck at any of them, that step alone is enabled.
ForkGoes(f) == \/ Prepare(f) \/ WaitedRuns(f) \/ CloseApart(f)
               \/ ShedTakes(f) \/ ShedCloses(f)
               \/ Fork(f) \/ HookRaises(f) \/ Open(f)
Assumes == /\ WF_vars(\E r \in Runs : Yield(r))
           /\ WF_vars(\E r \in Runs : Recheck(r))
           /\ WF_vars(GuestEnds(Caller))
           /\ \A f \in Forkers : SF_vars(ForkGoes(f))
FairSpec == Spec /\ Assumes

\* The victim alone is to end: its script is finite and calls nothing.
VictimGoes == Ask(Other) \/ TakeBack(Other) \/ End(Other)
JamSpec == FairSpec /\ SF_vars(VictimGoes)

\* What a forked child is owed, whatever the order.
InGate(f) == pc[f] \in {"shed", "shedCloses", "hook", "forked"}
ForksAlone == \A f \in Forkers : InGate(f) => \A r \in Runs : ~Guest(r)
\* The shed is the closing's: never before the gate closed.
ShedInGate == \A f \in Forkers : pc[f] \in {"shed", "shedCloses"} => closed > 0

\* Downward: a fork waits for no run, whoever holds the gate holds it
\* beside no run, who waits for the gate holds nothing, and a nested run
\* never meets it closed.
NoForkWaits == \A f \in Forkers : pc[f] # "waitRuns"
ClosedBesideNoRun == closed > 0 => reg = {}
WaiterKeepsNothing == \A r \in Runs : Waiting(r) => gvl # r /\ r \notin reg
NestedNeverWaits == /\ pc[Caller] \in {"callback", "inner"} => closed = 0
                    /\ host = "run" /\ pc[Caller] = "callback" => ~Waiting(Other)
Downward == NoForkWaits /\ ClosedBesideNoRun /\ WaiterKeepsNothing /\ NestedNeverWaits

\* Turn: who waits for the gate enters, holding from then.
Turn == \A r \in Runs : Waiting(r) ~> (r \in reg)

\* LetsGo: a fork opens; a run that is over, its guest ended, is no longer
\* in progress. One that never ends need not let go: nobody waits for it.
ForkLetsGo == \A f \in Forkers : InGate(f) ~> (pc[f] = "done")
GuestLetsGo == (pc[Caller] = "ending") ~> (Caller \notin reg)
OverLetsGo == \A r \in Runs : (pc[r] \in {"idle", "done", "out"}) ~> (r \notin reg)
LetsGo == ForkLetsGo /\ GuestLetsGo /\ OverLetsGo

\* NeverJammed: beside runs that never end and forks that never stop, a fork
\* answers and a run finishes.
ForkNeverJammed == \A f \in Forkers : (pc[f] \in {"idle", "waitRuns"}) ~> (pc[f] = "done")
RunNeverJammed == (pc[Other] \in {"idle", "wait", "woken"}) ~> (pc[Other] = "done")
NeverJammed == ForkNeverJammed /\ RunNeverJammed

\* Witnesses: each negation must be violated, so what is proved is reached.
NeverWaitsHold == ~(mode = "hold" /\ \E r \in Runs : Waiting(r))
NeverWaitsRelease == ~(mode = "release" /\ \E r \in Runs : Waiting(r))
NeverHandedBehindHold == ~(mode = "hold" /\ \E r, s \in Runs : pc[r] = "handed" /\ gvl = s)
NeverHanded == ~(\E r \in Runs : pc[r] = "handed")
NeverForksPastWaiter == ~(Cardinality({f \in Forkers : InGate(f)}) > 1
                          /\ \E r \in Runs : Waiting(r))
NeverRefusedAtYield == ~(mode = "hold" /\ pc[Caller] = "run" /\ gvl = NoOne /\ (\E f \in Forkers : pc[f] = "done"))
NeverNestedBesideFork == ~(host = "run" /\ pc[Caller] = "callback" /\ pc[Other] = "run" /\ (\E f \in Forkers : pc[f] = "done"))
NeverInnerBesideFork == ~(pc[Caller] = "inner" /\ (\E f \in Forkers : pc[f] = "done"))
NeverJoinsForker == ~(host = "forker" /\ pc[Caller] = "resumed" /\ (\E f \in Forkers : pc[f] = "done"))
NeverRefusedByEnding == ~(pc[Caller] = "ending" /\ pc[Other] = "idle" /\ (\E f \in Forkers : pc[f] = "done"))
NeverSheds == ~(\E f \in Forkers : pc[f] = "shedCloses")
NeverEndsFirst == ~(pc[Caller] = "endedFirst")
NeverVictimBesideEndless == ~(pc[Other] = "done" /\ pc[Caller] = "run" /\ mode = "hold")
NeverInterrupted == ~(\E r \in Runs : pc[r] = "out")
====
