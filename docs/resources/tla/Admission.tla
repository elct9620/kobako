---- MODULE Admission ----
\* Admission: the totals, the line and the fork's instant, as one wait. A run
\* takes all at once. What is pinned (what nil holds, what instances keep)
\* never ends by itself, so together it always leaves one reservation and what
\* that may leave. Who cannot fit only for what is pinned is passed over.
\*
\* Assumes: a run with a timeout ends, and what it closes returns (Reactor).
\* Nothing of nil runs, of the host app letting content go, or of forks.
\* No deadline. One pool, one victim "v": the two pools are taken together.
\*
\* Breaks takes one rule away:
\*   seedHeld          a waiter's seed counts before it runs
\*   piecemeal         a run takes half, then waits for the rest
\*   uncounted         what is closing is not counted
\*   nilTotal          nil may take all the total
\*   writerTwice       the share leaves no room for what one run may leave
\*   idleTakesReserve  what is pinned need not leave one reservation
\*   shareBlocks       who cannot fit for what is pinned holds up the line
\*   handlerBlocks     who runs host app code as it waits holds up the line
\*   barging           any waiter that fits goes first
\*   wakeOnly          an opening gate only wakes
\*   forkWaits         a fork closes the gate and waits for the runs
\*   waitsPeer         what is closing returns only as its peer helps
EXTENDS Naturals, Sequences

CONSTANTS Breaks,
          Writer,   \* the victim leaves content, the neighbour none
          Keeps     \* the neighbour leaves content the host app keeps

Total == 5
One == 2
Leave == 1
Runs == {"v", "h", "n", "m"}   \* victim, timed neighbour, two without timeout
NoEnd(r) == r \in {"n", "m"}
Ask(r) == IF NoEnd(r) THEN 1 ELSE One
Adds(r) == CASE r = "v" /\ Writer -> Leave
             [] r = "h" /\ Keeps -> Leave
             [] OTHER -> 0
Share == CASE Breaks = "nilTotal"    -> Total
           [] Breaks = "writerTwice" -> Total - One
           [] OTHER -> Total - One - Leave

VARIABLES st, line, used, kept, gate
vars == <<st, line, used, kept, gate>>

Holding(r) == CASE NoEnd(r) -> 0                  \* counted in used
                [] st[r] = "part" -> 1
                [] st[r] = "run" -> Ask(r)
                [] st[r] = "closing" /\ Breaks # "uncounted" -> Ask(r)
                [] OTHER -> 0
Timed == Holding("v") + Holding("h")
Closing(r) == IF st[r] = "closing" THEN Ask(r) ELSE 0
Standing == IF Breaks = "uncounted" THEN Closing("v") + Closing("h") ELSE 0
Leaving(r) == IF st[r] \in {"run", "closing", "part"} THEN Adds(r) ELSE 0
Pinned == used + kept + Leaving("v") + Leaving("h")
Running == \E r \in Runs : st[r] \in {"run", "part"}

FitsBeside(r, timed) ==
  /\ Ask(r) + timed + used + kept <= Total
  /\ NoEnd(r) => /\ used + Ask(r) <= Share
                 /\ Breaks = "idleTakesReserve" \/ Breaks = "nilTotal" \/ Pinned + Ask(r) + One <= Total
  /\ Adds(r) > 0 => Breaks = "idleTakesReserve" \/ Pinned + Adds(r) + One <= Total
Fits(r) == IF Breaks = "piecemeal" /\ ~NoEnd(r)
             THEN 1 + Timed + used + kept <= Total
             ELSE FitsBeside(r, Timed)
Passed(r) == \/ Breaks # "handlerBlocks" /\ st[r] = "away"
             \/ Breaks # "shareBlocks" /\ ~FitsBeside(r, 0)
InLine == SelectSeq(line, LAMBDA r : ~Passed(r))
Next1(r) == IF Breaks = "barging" THEN st[r] = "wait"
            ELSE IF InLine = <<>> THEN FALSE ELSE Head(InLine) = r

Init == /\ st = [r \in Runs |-> "idle"] /\ line = <<>>
        /\ used = 0 /\ kept = 0 /\ gate = FALSE
Arrive(r) ==
  /\ st[r] = "idle"
  /\ st' = [st EXCEPT ![r] = "wait"] /\ line' = Append(line, r)
  /\ kept' = IF Breaks = "seedHeld" /\ kept + Timed + used < Total THEN kept + 1 ELSE kept
  /\ UNCHANGED <<used, gate>>
Granted(r) ==
  /\ st' = [st EXCEPT ![r] = IF Breaks = "piecemeal" /\ ~NoEnd(r) THEN "part" ELSE "run"]
  /\ line' = SelectSeq(line, LAMBDA x : x # r)
  /\ used' = IF NoEnd(r) THEN used + Ask(r) ELSE used
Admit(r) == ~gate /\ st[r] = "wait" /\ Next1(r) /\ Fits(r) /\ Granted(r) /\ UNCHANGED <<kept, gate>>
Rest(r) ==
  /\ st[r] = "part" /\ 1 + Timed + used + kept <= Total
  /\ st' = [st EXCEPT ![r] = "run"] /\ UNCHANGED <<line, used, kept, gate>>
Grow(r) ==
  /\ NoEnd(r) /\ st[r] = "run"
  /\ used < Share /\ 1 + Timed + used + kept <= Total
  /\ Breaks \in {"idleTakesReserve", "nilTotal"} \/ Pinned + 1 + One <= Total
  /\ used' = used + 1 /\ UNCHANGED <<st, line, kept, gate>>
Ends(r) ==
  /\ ~NoEnd(r) /\ st[r] = "run"
  /\ st' = [st EXCEPT ![r] = "closing"] /\ UNCHANGED <<line, used, kept, gate>>
Closed(r) ==
  /\ st[r] = "closing" /\ ~(Breaks = "waitsPeer" /\ r = "h")
  /\ st' = [st EXCEPT ![r] = "idle"]
  /\ \/ kept' = kept
     \/ Adds(r) > 0 /\ r = "h" /\ kept < Total /\ kept' = kept + Adds(r)
  /\ UNCHANGED <<line, used, gate>>
\* A waiter's host app code, such as a trap handler, waits for the one behind.
Away == /\ st["h"] = "wait" /\ st["v"] = "wait"
        /\ st' = [st EXCEPT !["h"] = "away"] /\ UNCHANGED <<line, used, kept, gate>>
Back == /\ st["h"] = "away" /\ st["v"] # "wait"
        /\ st' = [st EXCEPT !["h"] = "wait"] /\ UNCHANGED <<line, used, kept, gate>>
Drop == kept > 0 /\ kept' = kept - 1 /\ UNCHANGED <<st, line, used, gate>>
\* A fork beside a run raises at once, so it has no state.
Fork == /\ ~gate /\ (Breaks = "forkWaits" \/ ~Running)
        /\ gate' = TRUE /\ UNCHANGED <<st, line, used, kept>>
Open ==
  /\ gate /\ ~Running
  /\ gate' = FALSE /\ kept' = kept
  /\ IF Breaks # "wakeOnly" /\ InLine # <<>> /\ st[Head(InLine)] = "wait" /\ Fits(Head(InLine))
       THEN Granted(Head(InLine))
       ELSE UNCHANGED <<st, line, used>>

Next == \/ \E r \in Runs : Arrive(r) \/ Admit(r) \/ Rest(r) \/ Grow(r) \/ Ends(r) \/ Closed(r)
        \/ Away \/ Back \/ Drop \/ Fork \/ Open
Spec == /\ Init /\ [][Next]_vars /\ WF_vars(Open) /\ WF_vars(Arrive("v"))
        /\ \A r \in Runs : WF_vars(Admit(r)) /\ WF_vars(Rest(r))
        /\ \A r \in {"v", "h"} : WF_vars(Ends(r)) /\ WF_vars(Closed(r))

LockOrdering == \A r \in Runs : st[r] = "wait" => Holding(r) = 0
WithinTotal == Timed + Standing + used + kept <= Total
StarvationFreedom == (st["v"] = "wait") ~> (st["v"] = "run")
ForwardProgress == \A r \in {"v", "h"} : (st[r] = "run") ~> (st[r] = "idle")
NeverJammed == []<>(st["v"] = "run")
====
