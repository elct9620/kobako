---- MODULE Handoff ----
\* A run whose guest stands on a thread of its own while the caller serves
\* it. The guest takes a directory lock, then asks the caller for a callback;
\* the callback starts a nested run whose guest asks for the same lock. The
\* caller may be interrupted at any point, which stops a guest's wait.
\*
\*   RunIdentity  a lock knows its owner by run, which a nested run inherits;
\*                otherwise by thread, so the nested guest is someone else
\*   Joins        the caller returns only once its guest has finished;
\*                otherwise an interrupt returns it at once
\*   Interrupts   whether the caller is ever interrupted
\*   Resumed      the callback waits for the nested run, as on a Fiber it
\*                resumes; otherwise it returns at once, as from a Fiber a
\*                scheduler runs on
\*   Offloaded    a Fiber scheduler takes the nested run's waits
\*   Exempt       a run whose waits a scheduler takes owns locks as itself
EXTENDS Naturals

CONSTANTS RunIdentity, Joins, Interrupts, Resumed, Offloaded, Exempt

\* Whether locks know the nested run as the outer one.
Shared == RunIdentity /\ ~(Exempt /\ Offloaded)

VARIABLES c, g1, g2, msg1, reply1, owner, intr
vars == <<c, g1, g2, msg1, reply1, owner, intr>>

Init == /\ c = "start"
        /\ g1 = "idle"
        /\ g2 = "idle"
        /\ msg1 = "none"
        /\ reply1 = FALSE
        /\ owner = "none"
        /\ intr = FALSE

\* The caller: starts the guest, serves its one callback, and waits for its end.
Caller ==
  \/ c = "start" /\ c' = "wait" /\ g1' = "take"
     /\ UNCHANGED <<g2, msg1, reply1, owner, intr>>
  \/ c = "wait" /\ msg1 = "job" /\ c' = "job" /\ msg1' = "none"
     /\ UNCHANGED <<g1, g2, reply1, owner, intr>>
  \/ c = "wait" /\ msg1 = "done" /\ c' = "end" /\ msg1' = "none"
     /\ UNCHANGED <<g1, g2, reply1, owner, intr>>
  \/ c = "wait" /\ intr /\ ~Joins /\ c' = "end"
     /\ UNCHANGED <<g1, g2, msg1, reply1, owner, intr>>
  \* The callback starts a nested run. Resumed, it waits for the guest's end;
  \* otherwise it answers at once, and the nested run goes on beside it.
  \/ c = "job" /\ g2 = "idle" /\ g2' = "take" /\ Resumed
     /\ UNCHANGED <<c, g1, msg1, reply1, owner, intr>>
  \/ c = "job" /\ g2 = "idle" /\ g2' = "take" /\ ~Resumed
     /\ c' = "wait" /\ reply1' = TRUE
     /\ UNCHANGED <<g1, msg1, owner, intr>>
  \/ c = "job" /\ g2 \in {"end", "refused"} /\ c' = "wait" /\ reply1' = TRUE
     /\ UNCHANGED <<g1, g2, msg1, owner, intr>>

\* The outer guest: holds the lock across its callback. It sees an interrupt
\* only in a wait, so it asks for the callback whatever the caller does.
Guest1 ==
  \/ g1 = "take" /\ owner = "none" /\ owner' = "g1" /\ g1' = "call"
     /\ UNCHANGED <<c, g2, msg1, reply1, intr>>
  \/ g1 = "call" /\ msg1 = "none" /\ msg1' = "job" /\ g1' = "park"
     /\ UNCHANGED <<c, g2, reply1, owner, intr>>
  \/ g1 = "park" /\ reply1 /\ reply1' = FALSE /\ g1' = "release"
     /\ UNCHANGED <<c, g2, msg1, owner, intr>>
  \/ g1 = "release" /\ owner' = "none" /\ g1' = "finish"
     /\ UNCHANGED <<c, g2, msg1, reply1, intr>>
  \/ g1 = "finish" /\ msg1 = "none" /\ msg1' = "done" /\ g1' = "end"
     /\ UNCHANGED <<c, g2, reply1, owner, intr>>

\* The nested guest: the lock held by its own run fails at once, as EDEADLK;
\* held by someone else, it waits until the lock is free or an interrupt.
Guest2 ==
  \/ g2 = "take" /\ owner = "none" /\ owner' = "g2" /\ g2' = "release"
     /\ UNCHANGED <<c, g1, msg1, reply1, intr>>
  \/ g2 = "take" /\ owner # "none" /\ Shared /\ g2' = "refused"
     /\ UNCHANGED <<c, g1, msg1, reply1, owner, intr>>
  \/ g2 = "take" /\ owner # "none" /\ intr /\ g2' = "end"
     /\ UNCHANGED <<c, g1, msg1, reply1, owner, intr>>
  \/ g2 = "release" /\ owner' = "none" /\ g2' = "end"
     /\ UNCHANGED <<c, g1, msg1, reply1, intr>>

Interrupt == Interrupts /\ ~intr /\ intr' = TRUE
             /\ UNCHANGED <<c, g1, g2, msg1, reply1, owner>>

\* Both runs ended and the caller returned; stuttering here is no deadlock.
Finished == /\ c = "end" /\ g1 = "end" /\ g2 \in {"idle", "end", "refused"}
            /\ UNCHANGED vars

Next == Caller \/ Guest1 \/ Guest2 \/ Interrupt \/ Finished
Spec == Init /\ [][Next]_vars

\* Once the caller returns, its guest has let go of the lock.
ReturnsFree == c = "end" => owner # "g1"

\* The nested guest is refused only where its callback waits for it, so the
\* lock could never come free while it waited.
RefusedOnlyWhenWaitedOn == g2 = "refused" => Resumed
====
