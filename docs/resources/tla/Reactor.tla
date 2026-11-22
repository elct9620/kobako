---- MODULE Reactor ----
\* The reactor's turn. A turn takes what is ready, does one bounded unit of
\* each, and what is left over waits for the next turn.
\*
\* Assumes: its thread runs, the OS hands out ready events, and other locks
\* are let go. Nothing of sockets: "a" and "b" may always have more, and
\* never help.
\*
\* Breaks takes one rule away:
\*   endless  one socket's share of a turn has no bound
\*   joins    what is left over joins the turn under way
\*   again    what is left over goes ahead of new events
\*   lost     an interrupted poll drops what it took
EXTENDS Naturals

CONSTANT Breaks
Sources == {"v", "a", "b"}

VARIABLES ready, turn
vars == <<ready, turn>>

Init == ready = {} /\ turn = {}
Ready(s) == /\ s \notin ready \cup turn
            /\ IF Breaks = "joins" /\ turn # {}
                 THEN turn' = turn \cup {s} /\ UNCHANGED ready
                 ELSE ready' = ready \cup {s} /\ UNCHANGED turn
Starts ==
  /\ turn = {} /\ ready # {}
  /\ LET takes == IF Breaks = "again" /\ ready # {"v"} THEN ready \ {"v"} ELSE ready IN
       /\ ready' = ready \ takes
       /\ \/ turn' = takes
          \/ Breaks = "lost" /\ turn' = {}
Serves ==
  \E s \in turn :
    IF Breaks = "endless" /\ s # "v"
      THEN UNCHANGED vars
      ELSE \E more \in (IF s = "v" THEN {FALSE} ELSE BOOLEAN) :
             /\ turn' = turn \ {s}
             /\ ready' = IF more THEN ready \cup {s} ELSE ready
Next == Starts \/ Serves \/ \E s \in Sources : Ready(s)
Spec == Init /\ [][Next]_vars /\ WF_vars(Starts) /\ WF_vars(Serves)

StarvationFreedom == ("v" \in ready) ~> ("v" \in turn)
ForwardProgress == []<>(turn = {})
====
