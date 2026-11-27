---- MODULE HandoffResumedScheduled ----
\* The handoff with the nested run on a Fiber the callback resumes and waits
\* on, while a scheduler takes the nested run's waits. The nested run owns
\* locks as itself and waits on the outer guest, which waits on the callback,
\* and nothing but an interrupt ends that wait.
EXTENDS Handoff
====
