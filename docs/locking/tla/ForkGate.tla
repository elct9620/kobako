---- MODULE ForkGate ----
\* The run gate a fork's prepare waits on. Thread A runs a Sandbox whose
\* callback waits for Thread B's run to end; Thread F forks. The prepare waits
\* for every run to end, and the gate closes to new runs only once none is in
\* progress, so B still enters and A still ends. With CloseEarly the gate
\* closes as the prepare starts to wait, so B waits on F, F on A, A on B.
EXTENDS Naturals

CONSTANT CloseEarly

VARIABLES pc, inRun, closed

vars == << pc, inRun, closed >>

Init == /\ pc = [a |-> "enter", b |-> "enter", f |-> "prepare"]
        /\ inRun = {}
        /\ closed = FALSE

\* A run enters only while the gate is open.
Enter(t, next) == /\ pc[t] = "enter"
                  /\ ~closed
                  /\ inRun' = inRun \cup {t}
                  /\ pc' = [pc EXCEPT ![t] = next]
                  /\ UNCHANGED closed

Exit(t) == /\ pc[t] = "exit"
           /\ inRun' = inRun \ {t}
           /\ pc' = [pc EXCEPT ![t] = "done"]
           /\ UNCHANGED closed

\* A's callback waits for B's run to end.
Callback == /\ pc.a = "callback"
            /\ pc.b = "done"
            /\ pc' = [pc EXCEPT !.a = "exit"]
            /\ UNCHANGED << inRun, closed >>

Prepare == /\ pc.f = "prepare"
           /\ IF CloseEarly
                THEN /\ closed' = TRUE
                     /\ pc' = [pc EXCEPT !.f = "wait"]
                ELSE /\ inRun = {}
                     /\ closed' = TRUE
                     /\ pc' = [pc EXCEPT !.f = "fork"]
           /\ UNCHANGED inRun

Wait == /\ pc.f = "wait"
        /\ inRun = {}
        /\ pc' = [pc EXCEPT !.f = "fork"]
        /\ UNCHANGED << inRun, closed >>

\* The fork finds no run in progress, then the parent opens the gate.
Fork == /\ pc.f = "fork"
        /\ inRun = {}
        /\ closed' = FALSE
        /\ pc' = [pc EXCEPT !.f = "done"]
        /\ UNCHANGED inRun

Finished == /\ \A t \in DOMAIN pc : pc[t] = "done"
            /\ UNCHANGED vars

Next == \/ Enter("a", "callback") \/ Enter("b", "exit")
        \/ Exit("a") \/ Exit("b") \/ Callback
        \/ Prepare \/ Wait \/ Fork \/ Finished

Spec == Init /\ [][Next]_vars

ForksAlone == pc.f = "fork" => inRun = {}
====
