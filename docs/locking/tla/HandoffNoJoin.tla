---- MODULE HandoffNoJoin ----
\* The handoff with an interrupt returning the caller before its guest ends.
\* The guest still asks for its callback, which no one serves, so it waits
\* forever holding the directory lock.
EXTENDS Handoff
====
