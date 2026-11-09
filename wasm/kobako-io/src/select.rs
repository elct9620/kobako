//! `IO.select`, which waits on any IO the guest holds, a file's and a
//! socket's alike, as CRuby's `select(2)` takes every descriptor. It reaches
//! the host through wasi-libc's `poll`, which preview1's `poll_oneoff`
//! answers, so a socket's wait sleeps on the host until its peer moves it.

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, Integer, IntoValue, Mrb, RArray, RClass, Symbol, TryConvert, Value};
use core::ffi::{c_int, c_short};

use crate::io::{argument_error, Direction};
use crate::{descriptors, errno, socket};

/// wasi-libc's `struct pollfd`.
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

/// Wait on `fds` at most `timeout` milliseconds, `-1` without end, as
/// `select(2)` and every one-descriptor wait CRuby makes share one `poll`.
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
    let fd = waitable_fd(mrb, self_, events)?;
    if events == POLLRDNORM && socket::holds_buffered(mrb, self_) {
        return Ok(true.into_value(mrb));
    }
    let mut fds = [PollFd {
        fd,
        events,
        revents: 0,
    }];
    poll_fds(mrb, &mut fds, to_millis(mrb, timeout)?)?;
    Ok(match is_ready(&fds[0]) {
        true => self_,
        false => qnil().as_value(),
    })
}

/// The descriptor a wait on `io` polls, refusing a stream not opened for the
/// way it waits, as CRuby's `IOError` does: a captured stream only writes.
fn waitable_fd(mrb: &Mrb, io: Value, events: c_short) -> Result<c_int, Error> {
    let direction = match events {
        POLLRDNORM => Direction::Read,
        _ => Direction::Write,
    };
    if let Some(handle) = socket::waitable_handle(mrb, io, direction)? {
        return Ok(handle as c_int);
    }
    let fd = crate::io::checked_fd(mrb, io, crate::stream::REACH_REFUSAL)?;
    let (opened, refusal) = match events {
        POLLRDNORM => (
            !descriptors::is_channel(fd) && crate::io::opened_to_read(fd),
            "not opened for reading",
        ),
        _ => (
            descriptors::is_channel(fd) || crate::io::opened_to_write(fd),
            "not opened for writing",
        ),
    };
    match opened {
        true => Ok(fd),
        false => Err(errno::io_error(mrb, refusal)),
    }
}

/// One object a select names, the IO it stands for, and which of its sets it
/// stands in.
struct Watched {
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
    if let Some(error) = error.filter(|set| !set.is_nil()) {
        let error = RArray::try_convert(error, mrb)?;
        for i in 0..error.len() {
            to_io(mrb, error.entry(mrb, i as isize)?)?;
        }
    }
    let mut watched = Vec::new();
    for (set, events) in [(Some(read), POLLRDNORM), (write, POLLWRNORM)] {
        let Some(set) = set.filter(|set| !set.is_nil()) else {
            continue;
        };
        let set = RArray::try_convert(set, mrb)?;
        for i in 0..set.len() {
            let given = set.entry(mrb, i as isize)?;
            watched.push(Watched {
                given,
                io: to_io(mrb, given)?,
                events,
            });
        }
    }
    let buffered = watched
        .iter()
        .any(|w| w.events == POLLRDNORM && socket::holds_buffered(mrb, w.io));
    let timeout = match buffered {
        true => 0,
        false => to_millis(mrb, timeout)?,
    };
    let mut fds = watched
        .iter()
        .map(|w| {
            let fd = w.io.funcall::<_, _, Value>(mrb, c"fileno", ())?;
            Ok(PollFd {
                fd: Integer::try_convert(fd, mrb)?.to_i64(mrb)? as c_int,
                events: w.events,
                revents: 0,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    poll_fds(mrb, &mut fds, timeout)?;
    let (readable, writable) = (mrb.ary_new(), mrb.ary_new());
    let mut any = false;
    for (w, fd) in watched.iter().zip(&fds) {
        let pending = w.events == POLLRDNORM && socket::holds_buffered(mrb, w.io);
        if pending || is_ready(fd) {
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
    let Some(timeout) = timeout.filter(|t| !t.is_nil()) else {
        return Ok(-1);
    };
    let seconds = f64::try_convert(timeout, mrb)?;
    if seconds < 0.0 {
        return Err(argument_error(mrb, "time interval must not be negative"));
    }
    Ok((seconds * 1000.0).ceil().min(c_int::MAX as f64) as c_int)
}
