---- MODULE RequestPoll ----
\* The ruby -> netg -> request -> sock group edges. Thread 1 is the guest:
\* holding the GVL in :hold mode and its open file, it polls its request's
\* future, which drives the socket, then cancels the future on the same
\* thread. Thread 2 is the reactor: it finds the socket in its table, then
\* moves what arrived and wakes the socket's waiters, and takes no future.
EXTENDS Locks

S == << << <<"acq", "GVL">>, <<"acq", "SharedFile">>, <<"acq", "task_future">>, <<"acq", "tcp_state">>,
           <<"rel", "tcp_state">>, <<"rel", "task_future">>, <<"acq", "task_future">>,
           <<"rel", "task_future">>, <<"rel", "SharedFile">>, <<"rel", "GVL">> >>,
        << <<"acq", "sockets">>, <<"rel", "sockets">>, <<"acq", "tcp_state">>, <<"rel", "tcp_state">>,
           <<"acq", "wq_waiters">>, <<"rel", "wq_waiters">> >> >>
====
