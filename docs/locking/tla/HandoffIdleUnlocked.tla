---- MODULE HandoffIdleUnlocked ----
\* An idle thread deciding to end apart from leaving the list. A run takes it
\* in between and waits on a job the thread never runs.
EXTENDS HandoffIdle
====
