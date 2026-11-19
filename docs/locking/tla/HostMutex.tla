---- MODULE HostMutex ----
\* The Host App's own Mutex closing a cycle through the vfs -> ruby group edge.
\* Thread 1's Ruby Filesystem callback, under a directory lock, waits for a
\* Host App Mutex; Ruby gives the GVL up while it waits. Thread 2 holds that
\* Mutex and runs a Sandbox that asks for the same directory lock. Each
\* waits on what the other holds, so the Host App must not do this.
EXTENDS Locks

S == << << <<"acq", "i_rwsem">>, <<"acq", "GVL">>, <<"acqR", "host_mutex", "GVL">>,
           <<"rel", "host_mutex">>, <<"rel", "GVL">>, <<"rel", "i_rwsem">> >>,
        << <<"acq", "GVL">>, <<"acq", "host_mutex">>, <<"acqR", "i_rwsem", "GVL">>,
           <<"rel", "i_rwsem">>, <<"rel", "host_mutex">>, <<"rel", "GVL">> >> >>
====
