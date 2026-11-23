---- MODULE ForkGateCollectedEarly ----
\* A collected caller taking its run out of progress before its guest ends.
\* The fork goes on and the child inherits that guest's lock.
EXTENDS ForkGate
====
