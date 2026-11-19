---- MODULE VfsCallbackHoldingGvl ----
\* VfsCallback with thread 2 waiting for the directory lock while it keeps
\* the GVL, as a wait outside the Blocking would. The two threads wait on
\* each other, so release before wait is what keeps VfsCallback free of deadlock.
EXTENDS Locks

S == << << <<"acq", "i_rwsem">>, <<"acq", "GVL">>, <<"rel", "GVL">>, <<"rel", "i_rwsem">> >>,
        << <<"acq", "GVL">>, <<"acq", "i_rwsem">>, <<"rel", "i_rwsem">>, <<"rel", "GVL">> >> >>
====
