---- MODULE HandoffThreadIdentity ----
\* The handoff with each lock knowing its owner by thread. The nested run's
\* guest stands on a thread of its own, so it waits on the lock its own run's
\* outer guest holds across the callback, and nothing ends that wait.
EXTENDS Handoff
====
