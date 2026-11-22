---- MODULE SleepingLocks ----
\* One sleeping lock. Who comes may take it ahead of who waits, as rwsem lets
\* them, until the first waiter waited past its time; then it is handed to
\* that one. A waiter may leave, as its run is interrupted. A signal it
\* handles and waits on from leaves its place as it was, so that is no step.
\*
\* Assumes: what an operation waits for below ends, and host app code
\* returns, so a holder's operation ends. Nothing of who comes, or how often.
\* Exclusive holds only: which readers share is alignment, and a test's.
\*
\* Breaks takes one rule away:
\*   noHandoff    the first waiter is never handed it
\*   keptOnLeave  the handoff stays with the waiter that left
\*   losesPlace   a waiter that handled a signal goes to the back
\*   endless     an operation under the lock has no bound
EXTENDS Sequences

CONSTANT Breaks
Tasks == {"v", "a", "b"}

VARIABLES st, line, handed
vars == <<st, line, handed>>

Free == \A t \in Tasks : st[t] # "hold"
Init == st = [t \in Tasks |-> "idle"] /\ line = <<>> /\ handed = "none"
Ask(t) == /\ st[t] = "idle"
          /\ st' = [st EXCEPT ![t] = "wait"] /\ line' = Append(line, t)
          /\ UNCHANGED handed
Expire == /\ Breaks # "noHandoff" /\ handed = "none" /\ line # <<>>
          /\ handed' = Head(line) /\ UNCHANGED <<st, line>>
Take(t) == /\ st[t] = "wait" /\ Free /\ handed \in {"none", t}
           /\ st' = [st EXCEPT ![t] = "hold"]
           /\ line' = SelectSeq(line, LAMBDA x : x # t)
           /\ handed' = "none"
Leave(t) == /\ t # "v" /\ st[t] = "wait"
            /\ st' = [st EXCEPT ![t] = "gone"]
            /\ line' = SelectSeq(line, LAMBDA x : x # t)
            /\ handed' = IF handed = t /\ Breaks # "keptOnLeave" THEN "none" ELSE handed
Signal(t) == /\ Breaks = "losesPlace" /\ st[t] = "wait"
             /\ line' = Append(SelectSeq(line, LAMBDA x : x # t), t)
             /\ handed' = IF handed = t THEN "none" ELSE handed
             /\ UNCHANGED st
Release(t) == /\ st[t] = "hold" /\ ~(Breaks = "endless" /\ t # "v")
              /\ st' = [st EXCEPT ![t] = "idle"] /\ UNCHANGED <<line, handed>>
Next == Expire \/ \E t \in Tasks : Ask(t) \/ Take(t) \/ Leave(t) \/ Signal(t) \/ Release(t)
Spec == /\ Init /\ [][Next]_vars /\ WF_vars(Expire) /\ WF_vars(Ask("v"))
        /\ \A t \in Tasks : WF_vars(Take(t)) /\ WF_vars(Release(t))

StarvationFreedom == (st["v"] = "wait") ~> (st["v"] = "hold")
ForwardProgress == \A t \in Tasks : (st[t] = "hold") ~> (st[t] # "hold")
NeverJammed == []<>(st["v"] = "hold")
====
