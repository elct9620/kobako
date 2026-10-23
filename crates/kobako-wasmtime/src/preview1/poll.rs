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

use super::errno::{Answer, Errno, Failure};
use super::memory::GuestMemory;
use crate::descriptors::Descriptor;
use crate::invocation::Invocation;
use crate::trap::TimeoutTrap;

const SUBSCRIPTION_SIZE: u32 = 48;
const EVENT_SIZE: u32 = 32;

const TAG_CLOCK: u8 = 0;
const TAG_FD_READ: u8 = 1;
const TAG_FD_WRITE: u8 = 2;

const CLOCK_REALTIME: u32 = 0;
const CLOCK_MONOTONIC: u32 = 1;
/// The highest `clockid` variant.
const CLOCK_MAX: u32 = 3;
const SUBCLOCKFLAGS_ABSTIME: u16 = 1;

/// One subscription as the guest wrote it.
struct Subscription {
    userdata: u64,
    kind: Kind,
}

enum Kind {
    /// A lone relative clock: a sleep of this many nanoseconds, which
    /// wasmtime-wasi reports as one byte ready.
    Sleep(u64),
    /// A clock among others, as the nanoseconds left before it fires.
    Clock(u64),
    /// A descriptor to read; `hangup` when a file has nothing left past
    /// its position.
    FdRead {
        hangup: bool,
    },
    FdWrite,
}

const EVENTRWFLAGS_HANGUP: u16 = 1;

pub(crate) fn poll_oneoff(
    invocation: &Invocation,
    mem: &mut GuestMemory,
    subs: u32,
    events: u32,
    count: u32,
    nevents: u32,
) -> Answer {
    if count == 0 {
        return Err(Errno::Inval.into());
    }
    let lone = count == 1;
    mem.consume(count, SUBSCRIPTION_SIZE)?;
    mem.consume(count, EVENT_SIZE)?;
    let subscriptions = (0..count)
        .map(|index| {
            let sub = GuestMemory::element(subs, index, SUBSCRIPTION_SIZE)?;
            read_subscription(invocation, mem, sub, lone)
        })
        .collect::<Answer<Vec<_>>>()?;

    let clocks = subscriptions.iter().filter_map(|sub| match sub.kind {
        Kind::Sleep(left) | Kind::Clock(left) => Some(left),
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
            Kind::Sleep(_) => (true, 1),
            Kind::Clock(left) => (left <= waited, 0),
            Kind::FdRead { .. } | Kind::FdWrite => (true, 1),
        };
        if ready {
            let event = GuestMemory::element(events, written, EVENT_SIZE)?;
            write_event(mem, event, sub, nbytes)?;
            written += 1;
        }
    }
    mem.write_u32(nevents, written)
}

fn read_subscription(
    invocation: &Invocation,
    mem: &GuestMemory,
    sub: u32,
    lone: bool,
) -> Answer<Subscription> {
    let userdata = mem.read_u64(sub)?;
    let kind = match mem.read_u8(GuestMemory::field(sub, 8)?)? {
        TAG_CLOCK => read_clock(invocation, mem, sub, lone)?,
        TAG_FD_READ => match descriptor(invocation, mem, sub)? {
            Descriptor::Stdin(_) => Kind::FdRead { hangup: false },
            Descriptor::File(file) => Kind::FdRead {
                hangup: file.file.metadata()?.len <= file.position,
            },
            Descriptor::Channel(_) | Descriptor::Directory(_) => return Err(Errno::Badf.into()),
        },
        TAG_FD_WRITE => match descriptor(invocation, mem, sub)? {
            Descriptor::Channel(_) | Descriptor::File(_) => Kind::FdWrite,
            Descriptor::Stdin(_) | Descriptor::Directory(_) => return Err(Errno::Badf.into()),
        },
        _ => return Err(Errno::Inval.into()),
    };
    Ok(Subscription { userdata, kind })
}

fn descriptor<'a>(
    invocation: &'a Invocation,
    mem: &GuestMemory,
    sub: u32,
) -> Answer<&'a Descriptor> {
    let fd = mem.read_u32(GuestMemory::field(sub, 16)?)?;
    invocation
        .wasi()
        .descriptors()
        .get(fd)
        .ok_or_else(|| Errno::Badf.into())
}

/// A clock, read against the guest's own clocks. Only the realtime and
/// monotonic clocks can be waited on among other subscriptions; a lone
/// relative clock is a sleep whichever it names, as in wasmtime-wasi.
fn read_clock(invocation: &Invocation, mem: &GuestMemory, sub: u32, lone: bool) -> Answer<Kind> {
    let id = mem.read_u32(GuestMemory::field(sub, 16)?)?;
    let timeout = mem.read_u64(GuestMemory::field(sub, 24)?)?;
    let flags = mem.read_u16(GuestMemory::field(sub, 40)?)?;
    if id > CLOCK_MAX || flags & !SUBCLOCKFLAGS_ABSTIME != 0 {
        return Err(Errno::Inval.into());
    }
    let absolute = flags & SUBCLOCKFLAGS_ABSTIME != 0;
    if lone && !absolute {
        return Ok(Kind::Sleep(timeout));
    }
    let clocks = invocation.wasi().clocks();
    let now = match (id, absolute) {
        (CLOCK_REALTIME | CLOCK_MONOTONIC, false) => 0,
        (CLOCK_MONOTONIC, true) => clocks.monotonic_now(),
        (CLOCK_REALTIME, true) => clocks.wall_now(),
        _ => return Err(Errno::Inval.into()),
    };
    Ok(Kind::Clock(timeout.saturating_sub(now)))
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
            return Err(Failure::Trap(wasmtime::Error::new(TimeoutTrap)));
        }
    }
    thread::sleep(wanted);
    Ok(())
}

fn write_event(mem: &mut GuestMemory, event: u32, sub: &Subscription, nbytes: u64) -> Answer {
    let (kind, flags) = match sub.kind {
        Kind::Sleep(_) | Kind::Clock(_) => (TAG_CLOCK, 0),
        Kind::FdRead { hangup } => (TAG_FD_READ, if hangup { EVENTRWFLAGS_HANGUP } else { 0 }),
        Kind::FdWrite => (TAG_FD_WRITE, 0),
    };
    mem.write_u64(event, sub.userdata)?;
    mem.write_u16(GuestMemory::field(event, 8)?, 0)?;
    mem.write_u8(GuestMemory::field(event, 10)?, kind)?;
    mem.write_u64(GuestMemory::field(event, 16)?, nbytes)?;
    mem.write_u16(GuestMemory::field(event, 24)?, flags)
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
