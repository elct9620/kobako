---- MODULE HandoffIdle ----
\* A guest's thread left idle past its limit, beside a run taking it for the
\* next job. The thread ends only while it is still on the idle list; a run
\* that took it off first hands it the job, which it waits for. With Locked
\* both look at the list under its one lock. Without it the thread decides to
\* end, a run takes it in between, and the run waits on a job nobody runs.
EXTENDS Naturals

CONSTANT Locked

VARIABLES w, c, listed, job, done

vars == << w, c, listed, job, done >>

Init == /\ w = "idle"
        /\ c = "pick"
        /\ listed = TRUE
        /\ job = FALSE
        /\ done = FALSE

\* The thread: a job before the limit runs; past it, the list decides.
TakeJob == /\ w \in {"idle", "take"}
           /\ job
           /\ job' = FALSE
           /\ w' = "run"
           /\ UNCHANGED << c, listed, done >>

TimeOut == /\ w = "idle"
           /\ ~job
           /\ w' = IF Locked THEN IF listed THEN "gone" ELSE "take" ELSE "decide"
           /\ listed' = IF Locked THEN FALSE ELSE listed
           /\ UNCHANGED << c, job, done >>

\* Without the lock the thread reads the list, then leaves it later.
Decide == /\ w = "decide"
          /\ w' = IF listed THEN "leave" ELSE "take"
          /\ UNCHANGED << c, listed, job, done >>

Leave == /\ w = "leave"
         /\ listed' = FALSE
         /\ w' = "gone"
         /\ UNCHANGED << c, job, done >>

Run == /\ w = "run"
       /\ done' = TRUE
       /\ w' = "finished"
       /\ UNCHANGED << c, listed, job >>

\* The run: takes the listed thread, or starts one of its own.
Pick == /\ c = "pick"
        /\ IF listed
             THEN /\ listed' = FALSE
                  /\ c' = "put"
                  /\ UNCHANGED done
             ELSE /\ c' = "end"
                  /\ done' = TRUE
                  /\ UNCHANGED listed
        /\ UNCHANGED << w, job >>

Put == /\ c = "put"
       /\ job' = TRUE
       /\ c' = "wait"
       /\ UNCHANGED << w, listed, done >>

Wait == /\ c = "wait"
        /\ done
        /\ c' = "end"
        /\ UNCHANGED << w, listed, job, done >>

Finished == /\ c = "end"
            /\ w \in {"gone", "finished", "idle"}
            /\ UNCHANGED vars

Next == TakeJob \/ TimeOut \/ Decide \/ Leave \/ Run
        \/ Pick \/ Put \/ Wait \/ Finished

Spec == Init /\ [][Next]_vars

\* The thread never ends holding a job a run handed it.
NoJobLost == w = "gone" => ~job
====
