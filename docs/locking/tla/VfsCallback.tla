---- MODULE VfsCallback ----
\* The vfs -> ruby group edge. Thread 1 holds a directory lock and its Ruby
\* Filesystem's callback takes the GVL. Thread 2 holds the GVL and asks for
\* the same directory lock, waiting through the Blocking, which gives the
\* GVL up first.
EXTENDS Locks

S == << << <<"acq", "i_rwsem">>, <<"acq", "GVL">>, <<"rel", "GVL">>, <<"rel", "i_rwsem">> >>,
        << <<"acq", "GVL">>, <<"acqR", "i_rwsem", "GVL">>, <<"rel", "i_rwsem">>, <<"rel", "GVL">> >> >>
====
