---- MODULE NestedRun ----
\* A run started inside a Ruby Filesystem's callback, reaching back to the
\* Filesystem another such callback holds. Thread 1's callback holds a
\* directory lock of A and runs a Sandbox that asks for one of B; thread 2's
\* holds B's and asks for A's. Each nested wait gives the GVL up, as release
\* before wait does, yet each waits on what the other holds, so a callback's
\* run must reach only Filesystems ranked after the one calling back.
EXTENDS Locks

S == << << <<"acq", "i_rwsem_a">>, <<"acq", "GVL">>, <<"acqR", "i_rwsem_b", "GVL">>,
           <<"rel", "i_rwsem_b">>, <<"rel", "GVL">>, <<"rel", "i_rwsem_a">> >>,
        << <<"acq", "i_rwsem_b">>, <<"acq", "GVL">>, <<"acqR", "i_rwsem_a", "GVL">>,
           <<"rel", "i_rwsem_a">>, <<"rel", "GVL">>, <<"rel", "i_rwsem_b">> >> >>
====
