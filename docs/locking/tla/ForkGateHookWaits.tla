---- MODULE ForkGateHookWaits ----
\* The Host App boundary: a _fork hook inside the gate waiting for a run,
\* which waits for the gate to open, as HostMutex is one.
EXTENDS ForkGate
====
