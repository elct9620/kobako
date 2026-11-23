---- MODULE ForkGateCheckApart ----
\* The fork looking for a run and closing the gate in two steps. A run
\* entering between leaves the child a lock nobody there lets go of.
EXTENDS ForkGate
====
