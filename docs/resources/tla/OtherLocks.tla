---- MODULE OtherLocks ----
\* The other locks: the accounts and their short locks. An invocation's
\* account is its own, a charge answers at once, and the totals' short lock is
\* let go with no call out under it. One resource stands for memory, fds and
\* counts.
\*
\* Assumes: nothing below; the OS hands a free short lock to who waits.
\*
\* Breaks takes one rule away:
\*   shared     the two draw on one account
\*   othersPay  what another holds is charged to the victim
\*   waits      a charge waits for room
\*   callsOut   the short lock is kept across a call out
EXTENDS Naturals

CONSTANT Breaks
Runs == {"v", "h"}
Limit == 2

VARIABLES spent, ask, lock
vars == <<spent, ask, lock>>

Init == spent = [r \in Runs |-> 0] /\ ask = [r \in Runs |-> "idle"] /\ lock = "none"
Account(r) == IF Breaks = "shared" THEN "v"
              ELSE IF Breaks = "othersPay" /\ r = "h" THEN "v"
              ELSE r
Asks(r) == /\ ask[r] \in {"idle", "refused", "ok"}
           /\ r = "v" => ask[r] = "idle"
           /\ ask' = [ask EXCEPT ![r] = "asks"] /\ UNCHANGED <<spent, lock>>
Takes(r) == /\ ask[r] = "asks" /\ lock = "none"
            /\ Breaks = "waits" => spent[Account(r)] < Limit \/ r = "h"
            /\ lock' = r /\ ask' = [ask EXCEPT ![r] = "in"] /\ UNCHANGED spent
Answers(r) ==
  /\ ask[r] = "in"
  /\ ~(Breaks = "callsOut" /\ r = "h")
  /\ LET a == IF Breaks = "waits" THEN "v" ELSE Account(r) IN
       IF spent[a] < Limit
         THEN spent' = [spent EXCEPT ![a] = @ + 1] /\ ask' = [ask EXCEPT ![r] = "ok"]
         ELSE spent' = spent /\ ask' = [ask EXCEPT ![r] = "refused"]
  /\ lock' = "none"
Next == \E r \in Runs : Asks(r) \/ Takes(r) \/ Answers(r)
Spec == /\ Init /\ [][Next]_vars /\ WF_vars(Asks("v"))
        /\ \A r \in Runs : SF_vars(Takes(r)) /\ WF_vars(Answers(r))

NeverRefused == ask["v"] # "refused"
StarvationFreedom == (ask["v"] = "asks") ~> (ask["v"] \in {"ok", "refused"})
ForwardProgress == \A r \in Runs : (lock = r) ~> (lock # r)
====
