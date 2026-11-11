//! `IO.select`, which waits on any IO the guest holds, a file's and a
//! socket's alike, as CRuby's `select(2)` takes every descriptor. A file is
//! asked through wasi-libc's `poll`, which preview1's `poll_oneoff` answers;
//! a socket is waited on through preview2's `poll`, so its wait sleeps on the
//! host until its peer moves it.

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, Integer, IntoValue, Mrb, RArray, RClass, Symbol, TryConvert, Value};
use core::ffi::{c_int, c_short};

use crate::io::{time_interval_of, Direction};
use crate::socket::Readiness;
use crate::wasi::wasi::clocks::monotonic_clock::subscribe_duration;
use crate::wasi::wasi::io::poll::{self as io_poll, Pollable};
use crate::{descriptors, errno, socket};

/// wasi-libc's `struct pollfd`.
#[derive(Clone, Copy)]
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

/// wasi-libc's `poll.h`, which takes only these two to ask for, and reports
/// the other two whatever was asked.
const POLLRDNORM: c_short = 0x1;
const POLLWRNORM: c_short = 0x2;
const POLLERR: c_short = 0x1000;
const POLLHUP: c_short = 0x2000;

unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
}

pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<(), Error> {
    io.define_singleton_method(mrb, c"select", beni::method!(io_s_select, -1))?;
    io.define_method(mrb, c"wait_readable", beni::method!(io_wait_readable, -1))?;
    io.define_method(mrb, c"wait_writable", beni::method!(io_wait_writable, -1))?;
    Ok(())
}

/// Ask files through preview1's `poll` at most `timeout` milliseconds, `-1`
/// without end.
fn poll_fds(mrb: &Mrb, fds: &mut [PollFd], timeout: c_int) -> Result<(), Error> {
    // SAFETY: `fds` holds `fds.len()` initialised entries poll writes back.
    if unsafe { poll(fds.as_mut_ptr(), fds.len(), timeout) } < 0 {
        return Err(errno::from_io(
            mrb,
            &std::io::Error::last_os_error(),
            "select(2)",
        ));
    }
    Ok(())
}

/// Whether `fd` came back ready for what it asked.
fn is_ready(fd: &PollFd) -> bool {
    fd.revents & (fd.events | POLLERR | POLLHUP) != 0
}

/// What a wait on one IO watches: a file's descriptor, or a socket's
/// readiness.
enum WaitTarget {
    File(c_int),
    Socket(Readiness),
}

/// Wait at most `timeout` milliseconds, `-1` without end, until any of
/// `targets` is ready, answering which are. A file never waits on the host,
/// so files alone are asked through preview1's `poll`; beside a socket they
/// are asked at once and the wait goes through preview2's `poll`, as an older
/// WASI adapts to a newer one. A socket that can never become ready waits as
/// a clock with no end, so only the deadline ends it, as Linux's blocks.
fn wait_any(
    mrb: &Mrb,
    targets: &[(WaitTarget, c_short)],
    timeout: c_int,
) -> Result<Vec<bool>, Error> {
    let mut ready = vec![false; targets.len()];
    let mut fds = Vec::new();
    let mut pollables = Vec::new();
    let mut has_socket = false;
    for (i, (target, events)) in targets.iter().enumerate() {
        has_socket |= matches!(target, WaitTarget::Socket(_));
        match target {
            WaitTarget::File(fd) => fds.push((
                i,
                PollFd {
                    fd: *fd,
                    events: *events,
                    revents: 0,
                },
            )),
            WaitTarget::Socket(Readiness::Ready) => ready[i] = true,
            WaitTarget::Socket(Readiness::Pending(pollable)) => pollables.push((i, pollable)),
            WaitTarget::Socket(Readiness::Inert) => {}
        }
    }
    if !fds.is_empty() || !has_socket {
        let mut asked: Vec<PollFd> = fds.iter().map(|(_, fd)| *fd).collect();
        poll_fds(mrb, &mut asked, if has_socket { 0 } else { timeout })?;
        for ((i, _), fd) in fds.iter().zip(&asked) {
            ready[*i] |= is_ready(fd);
        }
    }
    let any_ready = ready.contains(&true);
    if !has_socket || (any_ready && pollables.is_empty()) {
        return Ok(ready);
    }
    let clock = match (any_ready, timeout) {
        (true, _) => Some(0),
        (false, -1) if pollables.is_empty() => Some(u64::MAX),
        (false, -1) => None,
        (false, millis) => Some(u64::try_from(millis).unwrap_or(0) * 1_000_000),
    }
    .map(subscribe_duration);
    let mut list: Vec<&Pollable> = pollables.iter().map(|(_, pollable)| *pollable).collect();
    list.extend(clock.as_ref());
    for fired in io_poll::poll(&list) {
        if let Some((i, _)) = pollables.get(fired as usize) {
            ready[*i] = true;
        }
    }
    Ok(ready)
}

/// `wait_readable(timeout = nil)` answers the IO once a read would not wait,
/// `true` when its buffer already holds bytes, or `nil` at the timeout, as
/// CRuby's `rb_io_wait` polls the one descriptor `IO.select` would.
fn io_wait_readable(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    wait_for(mrb, self_, args, POLLRDNORM)
}

/// `wait_writable(timeout = nil)` answers the IO once a write would not wait,
/// or `nil` at the timeout.
fn io_wait_writable(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    wait_for(mrb, self_, args, POLLWRNORM)
}

fn wait_for(mrb: &Mrb, self_: Value, args: &[Value], events: c_short) -> Result<Value, Error> {
    let timeout = match args {
        [] => None,
        [timeout] => Some(*timeout),
        _ => return Err(Error::argnum(mrb, args.len(), 0, 1)),
    };
    let target = wait_target_of(mrb, self_, events)?;
    if events == POLLRDNORM && socket::holds_buffered(mrb, self_) {
        return Ok(true.into_value(mrb));
    }
    let ready = wait_any(mrb, &[(target, events)], to_millis(mrb, timeout)?)?;
    Ok(match ready[0] {
        true => self_,
        false => qnil().as_value(),
    })
}

/// The way a wait for `events` watches a socket.
fn direction_of(events: c_short) -> Direction {
    match events {
        POLLRDNORM => Direction::Read,
        _ => Direction::Write,
    }
}

/// What a wait on `io` watches, refusing a stream not opened for the way it
/// waits, as CRuby's `IOError` does: a captured stream only writes.
fn wait_target_of(mrb: &Mrb, io: Value, events: c_short) -> Result<WaitTarget, Error> {
    if let Some(readiness) = socket::readiness_of(mrb, io, direction_of(events))? {
        return Ok(WaitTarget::Socket(readiness));
    }
    let fd = crate::io::checked_fd(mrb, io, crate::stream::REACH_REFUSAL)?;
    let (opened, refusal) = match events {
        POLLRDNORM => (
            !descriptors::is_channel(fd) && crate::io::is_opened_to_read(fd),
            "not opened for reading",
        ),
        _ => (
            descriptors::is_channel(fd) || crate::io::is_opened_to_write(fd),
            "not opened for writing",
        ),
    };
    match opened {
        true => Ok(WaitTarget::File(fd)),
        false => Err(errno::io_error(mrb, refusal)),
    }
}

/// One object a select names, the IO it stands for, and which of its sets it
/// stands in.
struct WatchedIo {
    given: Value,
    io: Value,
    events: c_short,
}

/// The IO `object` stands for, as CRuby's `rb_io_get_io` converts it: itself
/// when an IO, else what its `to_io` answers; anything else raises
/// `TypeError`.
fn to_io(mrb: &Mrb, object: Value) -> Result<Value, Error> {
    let io_class = mrb.class_get(c"IO")?.as_value();
    let is_io = |value: Value| value.funcall::<_, _, bool>(mrb, c"is_a?", [io_class]);
    if is_io(object)? {
        return Ok(object);
    }
    let to_io = Symbol::new(mrb, "to_io")?.as_value();
    if !object.funcall::<_, _, bool>(mrb, c"respond_to?", [to_io])? {
        let message = format!(
            "no implicit conversion of {} into IO",
            object.classname(mrb)
        );
        return Err(Error::new(mrb, mrb.exception_type_error()?, &message));
    }
    let io = object.funcall::<_, _, Value>(mrb, c"to_io", ())?;
    if !is_io(io)? {
        let class = object.classname(mrb);
        let message = format!(
            "can't convert {class} to IO ({class}#to_io gives {})",
            io.classname(mrb)
        );
        return Err(Error::new(mrb, mrb.exception_type_error()?, &message));
    }
    Ok(io)
}

/// `IO.select(read, write = nil, error = nil, timeout = nil)` answers
/// `[readable, writable, []]` once one is ready, or `nil` at the timeout. The
/// third set is converted as CRuby's is but waits for nothing, since WASI
/// reports no exceptional condition. A select of nothing with no timeout
/// raises `Errno::ENOTSUP`, where CRuby's waits forever.
fn io_s_select(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args =
        scan_args::<(Value,), (Option<Value>, Option<Value>, Option<Value>), (), (), (), ()>(mrb)?;
    let (read,) = args.required;
    let (write, error, timeout) = args.optional;
    // The timeout is read first, and then the read, write, and error sets in
    // turn, as CRuby's `rb_f_select` reads it before `select_internal`.
    let millis = to_millis(mrb, timeout)?;
    let mut watched = Vec::new();
    for (set, events) in [(Some(read), POLLRDNORM), (write, POLLWRNORM)] {
        let Some(set) = set.filter(|set| !set.is_nil()) else {
            continue;
        };
        let set = RArray::try_convert(set, mrb)?;
        for i in 0..set.len() {
            let given = set.entry(mrb, i as isize)?;
            watched.push(WatchedIo {
                given,
                io: to_io(mrb, given)?,
                events,
            });
        }
    }
    if let Some(error) = error.filter(|set| !set.is_nil()) {
        let error = RArray::try_convert(error, mrb)?;
        for i in 0..error.len() {
            to_io(mrb, error.entry(mrb, i as isize)?)?;
        }
    }
    let buffered = watched
        .iter()
        .any(|w| w.events == POLLRDNORM && socket::holds_buffered(mrb, w.io));
    let timeout = match buffered {
        true => 0,
        false => millis,
    };
    let targets = watched
        .iter()
        .map(|w| {
            if let Some(readiness) = socket::select_readiness_of(mrb, w.io, direction_of(w.events))?
            {
                return Ok((WaitTarget::Socket(readiness), w.events));
            }
            let fd = w.io.funcall::<_, _, Value>(mrb, c"fileno", ())?;
            let fd = Integer::try_convert(fd, mrb)?.to_i64(mrb)? as c_int;
            Ok((WaitTarget::File(fd), w.events))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let ready = wait_any(mrb, &targets, timeout)?;
    let (readable, writable) = (mrb.ary_new(), mrb.ary_new());
    let mut any = false;
    for (w, is_ready) in watched.iter().zip(ready) {
        let pending = w.events == POLLRDNORM && socket::holds_buffered(mrb, w.io);
        if pending || is_ready {
            any = true;
            match w.events {
                POLLRDNORM => readable.push(mrb, w.given)?,
                _ => writable.push(mrb, w.given)?,
            }
        }
    }
    if !any {
        return Ok(qnil().as_value());
    }
    let answer = mrb.ary_new();
    answer.push(mrb, readable.as_value())?;
    answer.push(mrb, writable.as_value())?;
    answer.push(mrb, mrb.ary_new().as_value())?;
    Ok(answer.as_value())
}

/// A timeout in seconds as poll's milliseconds, rounded up so a short one
/// still waits; `nil` waits without end, as `-1` asks.
fn to_millis(mrb: &Mrb, timeout: Option<Value>) -> Result<c_int, Error> {
    let Some(timeout) = timeout else {
        return Ok(-1);
    };
    Ok(match time_interval_of(mrb, timeout)? {
        Some(wait) => (wait.as_secs_f64() * 1000.0).ceil().min(c_int::MAX as f64) as c_int,
        None => -1,
    })
}
