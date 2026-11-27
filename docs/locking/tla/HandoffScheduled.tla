---- MODULE HandoffScheduled ----
\* The handoff with the nested run on a Fiber a scheduler runs, which the
\* callback leaves running. The nested run owns locks as itself, so it waits
\* until the outer guest lets go, then takes the lock.
EXTENDS Handoff
====
