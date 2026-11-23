---- MODULE ForkGateWaits ----
\* A fork waiting for the runs in progress. A run whose callback waits for
\* that fork never ends, nor the wait.
EXTENDS ForkGate
====
