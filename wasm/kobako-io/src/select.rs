//! `IO.select`, which waits on any IO the guest holds, a file's and a
//! socket's alike, as CRuby's `select(2)` takes every descriptor. It reaches
//! the host through wasi-libc's `poll`, which preview1's `poll_oneoff`
//! answers, so a socket's wait sleeps on the host until its peer moves it.

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, Integer, Mrb, RArray, RClass, Symbol, TryConvert, Value};
use core::ffi::{c_int, c_short};

use crate::io::argument_error;
use crate::{errno, socket};

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
    Ok(())
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
/// third set waits for nothing, since WASI reports no exceptional condition.
fn io_s_select(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args =
        scan_args::<(Value,), (Option<Value>, Option<Value>, Option<Value>), (), (), (), ()>(mrb)?;
    let (read,) = args.required;
    let (write, _error, timeout) = args.optional;
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
    // SAFETY: `fds` holds `fds.len()` initialised entries poll writes back.
    let ready = unsafe { poll(fds.as_mut_ptr(), fds.len(), timeout) };
    if ready < 0 {
        return Err(errno::from_io(
            mrb,
            &std::io::Error::last_os_error(),
            "select(2)",
        ));
    }
    let (readable, writable) = (mrb.ary_new(), mrb.ary_new());
    let mut any = false;
    for (w, fd) in watched.iter().zip(&fds) {
        let pending = w.events == POLLRDNORM && socket::holds_buffered(mrb, w.io);
        if pending || fd.revents & (w.events | POLLERR | POLLHUP) != 0 {
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
