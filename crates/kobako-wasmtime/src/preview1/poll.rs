//! `poll_oneoff`, the guest's wait: for a clock, or for a descriptor to be
//! ready.
//!
//! Every descriptor kobako holds is ready at once, so only a clock ever
//! waits. A wait passes on the guest's own clocks: the hermetic rung's
//! never advance, so its waits end at once, and a permissive wait takes
//! host time but never past the invocation's deadline. The host is the one
//! waiting, where no epoch check reaches, so the wait sleeps on the runtime's
//! wait primitive, which a deadline or an interrupt ends.

use std::time::{Duration, Instant};

use kobako_runtime::vfs::PollMask;
use kobako_runtime::wait::do_poll;
use wiggle::{GuestPtr, GuestType};

use super::errno::{Answer, Errno};
use super::fd::file;
use super::memory::GuestMemory;
use super::types::{self, Clockid, Eventrwflags, Eventtype, Subclockflags, SubscriptionU};
use crate::invocation::Invocation;
use crate::trap::TimeoutTrap;

/// One subscription as the guest wrote it, and the error its event reports.
struct Subscription {
    userdata: u64,
    kind: Kind,
    error: Errno,
}

enum Kind {
    /// A clock, as the nanoseconds left before it fires.
    Clock(u64),
    FdRead,
    FdWrite,
}

pub(crate) fn poll_oneoff(
    invocation: &Invocation,
    mem: &mut GuestMemory<'_, '_>,
    subs: GuestPtr<types::Subscription>,
    events: GuestPtr<types::Event>,
    count: u32,
) -> Answer<types::Size> {
    if count == 0 {
        return Err(Errno::Inval.into());
    }
    mem.consume(count, types::Subscription::guest_size())?;
    mem.consume(count, types::Event::guest_size())?;
    let subscriptions = subs
        .as_array(count)
        .iter()
        .map(|sub| read_subscription(invocation, mem, sub?))
        .collect::<Answer<Vec<_>>>()?;

    let clocks = subscriptions.iter().filter_map(|sub| match sub.kind {
        Kind::Clock(left) => Some(left),
        Kind::FdRead | Kind::FdWrite => None,
    });
    let waited = if clocks.clone().count() == subscriptions.len() {
        let soonest = clocks.min().unwrap_or(0);
        wait(invocation, soonest)?;
        soonest
    } else {
        0
    };

    let mut written = 0;
    for sub in &subscriptions {
        let (ready, nbytes) = match sub.kind {
            Kind::Clock(left) => (left <= waited, 0),
            Kind::FdRead | Kind::FdWrite => (true, u64::from(sub.error == Errno::Success)),
        };
        if ready {
            mem.write(events.add(written)?, to_event(sub, nbytes))?;
            written += 1;
        }
    }
    Ok(written)
}

fn read_subscription(
    invocation: &Invocation,
    mem: &GuestMemory<'_, '_>,
    sub: GuestPtr<types::Subscription>,
) -> Answer<Subscription> {
    let sub = mem.read(sub)?;
    let (kind, error) = match sub.u {
        SubscriptionU::Clock(clock) => (read_clock(invocation, &clock)?, Errno::Success),
        SubscriptionU::FdRead(subscribed) => (
            Kind::FdRead,
            poll_descriptor(invocation, subscribed, PollMask::IN)?,
        ),
        SubscriptionU::FdWrite(subscribed) => (
            Kind::FdWrite,
            poll_descriptor(invocation, subscribed, PollMask::OUT)?,
        ),
    };
    Ok(Subscription {
        userdata: sub.userdata,
        kind,
        error,
    })
}

/// Hold a subscription to what its descriptor would find ready, answering the
/// error its event reports. A number no descriptor holds, or one standing only
/// for its path, is reported in its event, as Linux's `do_pollfd` marks it
/// `POLLNVAL` when `fdget` refuses it and the wait goes on. A
/// pipe's end is never ready for what the other end does, and nothing here
/// would wake a wait on it, so that is `BADF` rather than a wait that never
/// ends.
fn poll_descriptor(
    invocation: &Invocation,
    subscribed: types::SubscriptionFdReadwrite,
    event: PollMask,
) -> Answer<Errno> {
    let descriptors = invocation.wasi().descriptors();
    let Ok(file) = file(descriptors, subscribed.file_descriptor.into()) else {
        return Ok(Errno::Badf);
    };
    match file.poll(None).contains(event) {
        true => Ok(Errno::Success),
        false => Err(Errno::Badf.into()),
    }
}

/// A clock, read against the guest's own clocks. Only the realtime and
/// monotonic clocks can be waited on, alone or among other
/// subscriptions, as in wasmtime-wasi.
fn read_clock(invocation: &Invocation, clock: &types::SubscriptionClock) -> Answer<Kind> {
    let absolute = clock
        .flags
        .contains(Subclockflags::SUBSCRIPTION_CLOCK_ABSTIME);
    let clocks = invocation.wasi().clocks();
    let now = match (clock.id, absolute) {
        (Clockid::Realtime | Clockid::Monotonic, false) => 0,
        (Clockid::Monotonic, true) => clocks.monotonic_now(),
        (Clockid::Realtime, true) => clocks.wall_now(),
        _ => return Err(Errno::Inval.into()),
    };
    Ok(Kind::Clock(clock.timeout.saturating_sub(now)))
}

/// Wait `nanos` of the guest's time: none on frozen clocks, and on live
/// ones no further than the invocation's deadline, which ends it as a
/// timeout. An interrupt of the thread ends it with EINTR, as a signal ends
/// Linux's `poll`.
fn wait(invocation: &Invocation, nanos: u64) -> Answer {
    if nanos == 0 || !invocation.wasi().clocks().advances() {
        return Ok(());
    }
    match do_poll(Some(Duration::from_nanos(nanos)), |_| None::<()>) {
        Ok(_) => Ok(()),
        Err(_) if invocation.deadline().is_some_and(|at| Instant::now() >= at) => {
            Err(types::Error::trap(wasmtime::Error::new(TimeoutTrap)))
        }
        Err(_) => Err(Errno::Intr.into()),
    }
}

fn to_event(sub: &Subscription, nbytes: u64) -> types::Event {
    types::Event {
        userdata: sub.userdata,
        error: sub.error,
        type_: match sub.kind {
            Kind::Clock(_) => Eventtype::Clock,
            Kind::FdRead => Eventtype::FdRead,
            Kind::FdWrite => Eventtype::FdWrite,
        },
        fd_readwrite: types::EventFdReadwrite {
            nbytes,
            flags: Eventrwflags::empty(),
        },
    }
}

#[cfg(test)]
mod tests {
    //! Witness that a guest's clock wait passes on its posture's clocks:
    //! at once on the frozen ones, and on the live ones no further than
    //! the deadline.
    use std::time::{Duration, Instant};

    use wasmtime::{Module, Store};

    use crate::cache::load_engine;
    use crate::config::Config;
    use crate::imports;
    use crate::invocation::Invocation;
    use crate::trap::{to_trap, NO_TIMEOUT_EPOCH_DELTA};
    use crate::wasi::{sandbox_workers, Wasi};
    use kobako_runtime::error::Trap;
    use kobako_runtime::profile::Profile;
    use kobako_runtime::sched::{run_task, Task};

    /// One relative monotonic-clock subscription of ten seconds at 0,
    /// its event written at 64 and the event count at 96.
    const WAIT_WAT: &str = r#"
        (module
          (import "wasi_snapshot_preview1" "poll_oneoff"
            (func $poll (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "wait_ten_seconds") (result i32)
            (i32.store8 (i32.const 8) (i32.const 0))
            (i32.store (i32.const 16) (i32.const 1))
            (i64.store (i32.const 24) (i64.const 10000000000))
            (call $poll (i32.const 0) (i32.const 64) (i32.const 1) (i32.const 96))))
    "#;

    /// Run the ten-second wait at `profile` under `deadline`, handing the
    /// running thread's Task to `interrupted` first, answering how it ended
    /// and how long the host spent on it.
    fn wait_ten_seconds(
        profile: Profile,
        deadline: Option<Duration>,
        interrupted: Option<fn(Task)>,
    ) -> (wasmtime::Result<i32>, Duration) {
        let engine = load_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile,
            security: Vec::new(),
        };
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_workers())
            .expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(None, wasi));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let linker = imports::linker().expect("host imports must link");
        let module = Module::new(engine, WAIT_WAT).expect("wait module must compile");
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("wait module must instantiate");
        let wait = instance
            .get_typed_func::<(), i32>(&mut store, "wait_ten_seconds")
            .expect("wait export must resolve");
        let started = Instant::now();
        let deadline = deadline.map(|left| started + left);
        store.data_mut().set_deadline(deadline);
        let ended = run_task(deadline, || {
            if let Some(interrupted) = interrupted {
                interrupted(Task::current());
            }
            wait.call(&mut store, ())
        });
        (ended, started.elapsed())
    }

    // @behavior RT-337
    #[test]
    fn an_interrupted_wait_answers_eintr_at_once() {
        let (ended, spent) = wait_ten_seconds(
            Profile::Permissive,
            Some(Duration::from_secs(20)),
            Some(|task: Task| {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    task.interrupt();
                });
            }),
        );
        assert!(
            ended.as_ref().ok() == Some(&ERRNO_INTR) && spent < Duration::from_secs(5),
            "a ten-second clock wait through a permissive guest's poll_oneoff, its thread \
             interrupted, must answer EINTR at once, got {:?} after {spent:?}",
            ended.map_err(|trap| trap.to_string())
        );
    }

    const ERRNO_INTR: i32 = 27;

    // @behavior RT-073
    #[test]
    fn a_hermetic_wait_ends_without_the_host_waiting() {
        let (ended, spent) = wait_ten_seconds(Profile::Hermetic, None, None);
        assert_eq!(
            ended.ok(),
            Some(0),
            "a ten-second clock wait through a hermetic guest's poll_oneoff must succeed"
        );
        assert!(
            spent < Duration::from_secs(1),
            "a ten-second clock wait through a hermetic guest's poll_oneoff must return at once, \
             not after {spent:?}"
        );
    }

    /// What a lone relative wait of no time on clock `id` answers.
    fn wait_alone_on(id: i32) -> i32 {
        const WAIT_ON_WAT: &str = r#"
            (module
              (import "wasi_snapshot_preview1" "poll_oneoff"
                (func $poll (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "wait_on") (param $id i32) (result i32)
                (i32.store (i32.const 16) (local.get $id))
                (call $poll (i32.const 0) (i32.const 64) (i32.const 1) (i32.const 96))))
        "#;
        let engine = load_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile: Profile::Hermetic,
            security: Vec::new(),
        };
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_workers())
            .expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(None, wasi));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let linker = imports::linker().expect("host imports must link");
        let module = Module::new(engine, WAIT_ON_WAT).expect("wait module must compile");
        linker
            .instantiate(&mut store, &module)
            .expect("wait module must instantiate")
            .get_typed_func::<i32, i32>(&mut store, "wait_on")
            .expect("wait export must resolve")
            .call(&mut store, id)
            .expect("the wait must not trap")
    }

    // @behavior RT-131
    #[test]
    fn a_lone_wait_on_a_cpu_time_clock_is_invalid() {
        assert_eq!(
            [0, 1, 2, 3].map(wait_alone_on),
            [0, 0, ERRNO_INVAL, ERRNO_INVAL],
            "a lone relative wait through poll_oneoff on the process or thread CPU-time clock \
             must answer EINVAL as wasmtime-wasi does, while the realtime and monotonic clocks wait"
        );
    }

    const ERRNO_INVAL: i32 = 28;

    /// What a lone wait on standard output answers for event `tag`, 1 to read
    /// and 2 to write.
    fn wait_on_stdout(tag: i32) -> i32 {
        const WAIT_ON_FD_WAT: &str = r#"
            (module
              (import "wasi_snapshot_preview1" "poll_oneoff"
                (func $poll (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "wait_on") (param $tag i32) (result i32)
                (i32.store8 (i32.const 8) (local.get $tag))
                (i32.store (i32.const 16) (i32.const 1))
                (call $poll (i32.const 0) (i32.const 64) (i32.const 1) (i32.const 96))))
        "#;
        let engine = load_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile: Profile::Hermetic,
            security: Vec::new(),
        };
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_workers())
            .expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(None, wasi));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let linker = imports::linker().expect("host imports must link");
        let module = Module::new(engine, WAIT_ON_FD_WAT).expect("wait module must compile");
        linker
            .instantiate(&mut store, &module)
            .expect("wait module must instantiate")
            .get_typed_func::<i32, i32>(&mut store, "wait_on")
            .expect("wait export must resolve")
            .call(&mut store, tag)
            .expect("the wait must not trap")
    }

    const ERRNO_BADF: i32 = 8;

    // Nothing ever makes standard output ready to read, so without the refusal
    // the wait would only end with the invocation.
    // @behavior RT-285
    #[test]
    fn a_wait_on_what_a_descriptor_never_does_answers_ebadf() {
        assert_eq!(
            [1, 2].map(wait_on_stdout),
            [ERRNO_BADF, 0],
            "poll_oneoff on standard output must answer EBADF waiting to read and be ready to \
             write"
        );
    }

    // @behavior RT-074
    #[test]
    fn a_permissive_wait_ends_at_the_deadline_as_a_timeout() {
        let (ended, spent) =
            wait_ten_seconds(Profile::Permissive, Some(Duration::from_millis(50)), None);
        let trap = ended.expect_err("a wait past the deadline must not return to the guest");
        assert!(
            matches!(to_trap(trap), Trap::Timeout(_)),
            "a clock wait past the deadline through a permissive guest's poll_oneoff must end \
             as a timeout"
        );
        assert!(
            spent >= Duration::from_millis(50) && spent < Duration::from_secs(5),
            "a clock wait past the deadline through a permissive guest's poll_oneoff must end \
             at the deadline, not after {spent:?}"
        );
    }
}
