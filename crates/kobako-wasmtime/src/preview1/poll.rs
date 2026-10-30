//! `poll_oneoff`, the guest's wait: for a clock, or for a descriptor to be
//! ready.
//!
//! Every descriptor kobako holds is ready at once, so only a clock ever
//! waits. A wait passes on the guest's own clocks: the hermetic rung's
//! never advance, so its waits end at once, and a permissive wait takes
//! host time but never past the invocation's deadline — the host is the
//! one waiting, where no epoch check can interrupt it.

use std::thread;
use std::time::{Duration, Instant};

use wiggle::{GuestPtr, GuestType};

use super::errno::{Answer, Errno};
use super::memory::GuestMemory;
use super::types::{self, Clockid, Eventrwflags, Eventtype, Subclockflags, SubscriptionU};
use crate::descriptors::Descriptor;
use crate::invocation::Invocation;
use crate::trap::TimeoutTrap;

/// One subscription as the guest wrote it.
struct Subscription {
    userdata: u64,
    kind: Kind,
}

enum Kind {
    /// A clock, as the nanoseconds left before it fires.
    Clock(u64),
    /// A descriptor to read; `hangup` when a file has nothing left past
    /// its position.
    FdRead {
        hangup: bool,
    },
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
        Kind::FdRead { .. } | Kind::FdWrite => None,
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
            Kind::FdRead { .. } | Kind::FdWrite => (true, 1),
        };
        if ready {
            mem.write(events.add(written)?, event(sub, nbytes))?;
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
    let kind = match sub.u {
        SubscriptionU::Clock(clock) => read_clock(invocation, &clock)?,
        SubscriptionU::FdRead(subscribed) => match descriptor(invocation, subscribed)? {
            Descriptor::File(file) if file.is_stream() && file.readable() => {
                Kind::FdRead { hangup: false }
            }
            Descriptor::File(file) if !file.is_stream() => Kind::FdRead {
                hangup: file.getattr()?.len <= file.pos(),
            },
            Descriptor::File(_) | Descriptor::Directory(_) => return Err(Errno::Badf.into()),
        },
        SubscriptionU::FdWrite(subscribed) => match descriptor(invocation, subscribed)? {
            Descriptor::File(file) if file.is_stream() && file.writable() => Kind::FdWrite,
            Descriptor::File(file) if file.is_stream() => return Err(Errno::Badf.into()),
            Descriptor::File(file) if !file.path().mnt().is_readonly() => Kind::FdWrite,
            Descriptor::File(_) => return Err(Errno::Perm.into()),
            Descriptor::Directory(_) => return Err(Errno::Badf.into()),
        },
    };
    Ok(Subscription {
        userdata: sub.userdata,
        kind,
    })
}

fn descriptor(
    invocation: &Invocation,
    subscribed: types::SubscriptionFdReadwrite,
) -> Answer<&Descriptor> {
    invocation
        .wasi()
        .descriptors()
        .get(subscribed.file_descriptor.into())
        .ok_or_else(|| Errno::Badf.into())
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
/// timeout.
fn wait(invocation: &Invocation, nanos: u64) -> Answer {
    if nanos == 0 || !invocation.wasi().clocks().advances() {
        return Ok(());
    }
    let wanted = Duration::from_nanos(nanos);
    if let Some(deadline) = invocation.deadline() {
        let left = deadline.saturating_duration_since(Instant::now());
        if wanted >= left {
            thread::sleep(left);
            return Err(types::Error::trap(wasmtime::Error::new(TimeoutTrap)));
        }
    }
    thread::sleep(wanted);
    Ok(())
}

fn event(sub: &Subscription, nbytes: u64) -> types::Event {
    let (type_, flags) = match sub.kind {
        Kind::Clock(_) => (Eventtype::Clock, Eventrwflags::empty()),
        Kind::FdRead { hangup: true } => (Eventtype::FdRead, Eventrwflags::FD_READWRITE_HANGUP),
        Kind::FdRead { hangup: false } => (Eventtype::FdRead, Eventrwflags::empty()),
        Kind::FdWrite => (Eventtype::FdWrite, Eventrwflags::empty()),
    };
    types::Event {
        userdata: sub.userdata,
        error: Errno::Success,
        type_,
        fd_readwrite: types::EventFdReadwrite { nbytes, flags },
    }
}

#[cfg(test)]
mod tests {
    //! Witness that a guest's clock wait passes on its posture's clocks:
    //! at once on the frozen ones, and on the live ones no further than
    //! the deadline.
    use std::time::{Duration, Instant};

    use wasmtime::{Module, Store};

    use crate::cache::shared_engine;
    use crate::config::Config;
    use crate::imports;
    use crate::invocation::Invocation;
    use crate::trap::{to_trap, NO_TIMEOUT_EPOCH_DELTA};
    use crate::wasi::Wasi;
    use kobako_runtime::error::Trap;
    use kobako_runtime::profile::Profile;

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

    /// Run the ten-second wait at `profile` under `deadline`, answering
    /// how it ended and how long the host spent on it.
    fn wait_ten_seconds(
        profile: Profile,
        deadline: Option<Duration>,
    ) -> (wasmtime::Result<i32>, Duration) {
        let engine = shared_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile,
        };
        let wasi = Wasi::for_invocation(&config, &[], &[]).expect("WASI context must build");
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
        store
            .data_mut()
            .set_deadline(deadline.map(|left| started + left));
        let ended = wait.call(&mut store, ());
        (ended, started.elapsed())
    }

    // @behavior RT-073
    #[test]
    fn a_hermetic_wait_ends_without_the_host_waiting() {
        let (ended, spent) = wait_ten_seconds(Profile::Hermetic, None);
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
        let engine = shared_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile: Profile::Hermetic,
        };
        let wasi = Wasi::for_invocation(&config, &[], &[]).expect("WASI context must build");
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

    // @behavior RT-074
    #[test]
    fn a_permissive_wait_ends_at_the_deadline_as_a_timeout() {
        let (ended, spent) = wait_ten_seconds(Profile::Permissive, Some(Duration::from_millis(50)));
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
