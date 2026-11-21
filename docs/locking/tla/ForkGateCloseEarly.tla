---- MODULE ForkGateCloseEarly ----
\* The run gate closing as the prepare starts to wait. A run whose callback
\* waits for another Thread's run never ends, as that run cannot enter.
EXTENDS ForkGate
====
