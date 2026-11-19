---- MODULE ReactorTakesGvl ----
\* ReactorCancel with a reactor future that takes the GVL, as one running a
\* Ruby resolver would. The guest holds the GVL while it cancels the task,
\* and the reactor holds the task while it waits for the GVL, so a reactor
\* future must never call Ruby.
EXTENDS Locks

S == << << <<"acq", "GVL">>, <<"acq", "SharedFile">>, <<"acq", "task_future">>, <<"rel", "task_future">>,
           <<"rel", "SharedFile">>, <<"rel", "GVL">> >>,
        << <<"acq", "task_future">>, <<"acq", "GVL">>, <<"rel", "GVL">>, <<"rel", "task_future">> >> >>
====
