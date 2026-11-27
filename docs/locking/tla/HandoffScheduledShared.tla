---- MODULE HandoffScheduledShared ----
\* The scheduled handoff with the nested run owning locks as the outer one,
\* though a scheduler takes its waits. It is refused the lock the outer guest
\* would let go once the callback answers.
EXTENDS Handoff
====
