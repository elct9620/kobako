---- MODULE Gvl ----
\* GVL: callers of runs whose guests never help, beside a Thread that needs
\* it again and again. A caller gives a turn up each quantum, counted from
\* when it took the GVL, and looks at its mailbox once before that.
\* A caller under :release waits with no GVL, which is the "hold" case less.
\*
\* Assumes: time passes, a runnable Thread runs, CRuby serves its queue in
\* order, other locks are let go, host app code returns. No guest step is fair.
\*
\* Breaks takes one rule away:
\*   kept         the caller waits with no limit, as today
\*   guestYields  it yields only where its guest reaches a tick
\*   restarts     the quantum starts again with each message
\*   timeFirst    the time is judged before the mailbox is looked at
EXTENDS Naturals, Sequences

CONSTANT Breaks
Callers == {"c", "d"}
Threads == Callers \cup {"v"}

VARIABLES gvl, line, phase, mail, tick
vars == <<gvl, line, phase, mail, tick>>

Init == gvl = "none" /\ line = <<>> /\ phase = "fresh" /\ mail = {} /\ tick = FALSE
Ask(t) == /\ gvl # t /\ \A i \in 1..Len(line) : line[i] # t
          /\ line' = Append(line, t) /\ UNCHANGED <<gvl, phase, mail, tick>>
Take == /\ gvl = "none" /\ line # <<>>
        /\ gvl' = Head(line) /\ line' = Tail(line)
        /\ phase' = "fresh" /\ tick' = FALSE /\ UNCHANGED mail
Looks == /\ gvl \in Callers /\ phase = "fresh"
         /\ Breaks = "timeFirst" => gvl \notin mail
         /\ phase' = "runs" /\ mail' = mail \ {gvl} /\ UNCHANGED <<gvl, line, tick>>
Expires == /\ gvl \in Callers /\ Breaks # "kept"
           /\ phase = "runs" \/ (Breaks = "timeFirst" /\ phase = "fresh")
           /\ phase' = "over" /\ UNCHANGED <<gvl, line, mail, tick>>
Says(c) == /\ c \notin mail
           /\ mail' = mail \cup {c} /\ UNCHANGED <<gvl, line, phase, tick>>
Serves == /\ gvl \in Callers /\ phase = "runs" /\ gvl \in mail
          /\ mail' = mail \ {gvl}
          /\ phase' = IF Breaks = "restarts" THEN "fresh" ELSE phase
          /\ UNCHANGED <<gvl, line, tick>>
Ticks == /\ Breaks = "guestYields" /\ gvl \in Callers /\ ~tick
         /\ tick' = TRUE /\ UNCHANGED <<gvl, line, phase, mail>>
Yields == /\ gvl \in Callers /\ phase = "over"
          /\ Breaks = "guestYields" => tick
          /\ line' = Append(line, gvl) /\ gvl' = "none"
          /\ phase' = "fresh" /\ UNCHANGED <<mail, tick>>
VictimDone == /\ gvl = "v"
              /\ gvl' = "none" /\ UNCHANGED <<line, phase, mail, tick>>
Next == \/ Take \/ Looks \/ Expires \/ Serves \/ Ticks \/ Yields \/ VictimDone
        \/ (\E t \in Threads : Ask(t)) \/ \E c \in Callers : Says(c)
Spec == /\ Init /\ [][Next]_vars
        /\ WF_vars(Take) /\ WF_vars(Looks) /\ WF_vars(Expires) /\ WF_vars(Yields)
        /\ WF_vars(VictimDone) /\ WF_vars(Ask("v")) /\ WF_vars(Ask("c"))

StarvationFreedom == []<>(gvl = "v")
ForwardProgress == \A c \in Callers : (gvl = c) ~> (gvl # c)
Served == ("c" \in mail) ~> ("c" \notin mail)
====
