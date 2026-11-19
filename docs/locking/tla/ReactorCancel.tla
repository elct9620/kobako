---- MODULE ReactorCancel ----
\* The ruby -> netg -> rtask -> sock group edges. Thread 1 is the guest: holding its open
\* file, it cancels the reactor task, waiting while the reactor polls it,
\* then reads the socket. Thread 2 is the reactor: holding the task's
\* future through a poll, it drives the socket. The guest holds the GVL in
\* :hold mode, which the reactor never takes.
EXTENDS Locks

S == << << <<"acq", "GVL">>, <<"acq", "SharedFile">>, <<"acq", "task_future">>, <<"rel", "task_future">>,
           <<"acq", "tcp_state">>, <<"rel", "tcp_state">>, <<"rel", "SharedFile">>, <<"rel", "GVL">> >>,
        << <<"acq", "task_future">>, <<"acq", "tcp_state">>, <<"rel", "tcp_state">>, <<"rel", "task_future">> >> >>
====
