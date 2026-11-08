//! A `Net::HTTP` subset over WASI 0.2's `wasi:http`, which the guest imports
//! as a core module, as it imports `wasi:sockets` for `TCPSocket`. A request
//! with no block is answered whole, its body read to the end before the call
//! returns; one with a block yields the response first, so `read_body` streams
//! the body as CRuby's does. A refusal raises what CRuby's would, wrapped as
//! CRuby wraps a failed open.

use std::cell::RefCell;
use std::time::Duration;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{
    Error, ExceptionClass, ForEach, IntoValue, Module, Mrb, Proc, RArray, RClass, RHash, RModule,
    RObject, Symbol, TryConvert, Value,
};

use crate::errno;
use crate::io::argument_error;
use crate::socket;
use crate::wasi::wasi::http::outgoing_handler::handle;
use crate::wasi::wasi::http::types::{
    ErrorCode, Fields, IncomingBody, Method, OutgoingBody, OutgoingRequest, Scheme,
};
use crate::wasi::wasi::io::streams::{InputStream, StreamError};
use crate::wasi::{wait_within, write_within};

/// The most one read of a response's body asks, the host's buffer.
const READ_CHUNK: u64 = 64 * 1024;

/// How much of a streamed body passes between collections.
const COLLECT_EVERY: usize = 1 << 20;

/// A response's head as `wasi:http` answers it, and its body still to read.
struct Answer {
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Unread,
}

/// A response's body the guest has not read yet, its stream dropped before
/// the body it belongs to, as WASI asks of a child resource.
struct Unread {
    stream: Option<InputStream>,
    body: Option<IncomingBody>,
    /// How long one chunk may take, as `read_timeout` bounds each read.
    read: Option<Duration>,
}

impl Unread {
    /// The next chunk the body holds, `None` at its end; one slower than the
    /// read limit raises `Net::ReadTimeout`.
    fn chunk(&mut self, mrb: &Mrb) -> Result<Option<Vec<u8>>, Error> {
        let Some(stream) = &self.stream else {
            return Ok(None);
        };
        let read = loop {
            match stream.read(READ_CHUNK) {
                Ok(bytes) if bytes.is_empty() => {
                    if !wait_within(&stream.subscribe(), self.read) {
                        return Err(net_error(mrb, c"ReadTimeout", "Net::ReadTimeout"));
                    }
                }
                read => break read,
            }
        };
        match read {
            Ok(bytes) => Ok(Some(bytes)),
            Err(StreamError::Closed) => {
                self.finish();
                Ok(None)
            }
            Err(StreamError::LastOperationFailed(_)) => Err(errno::error_with_message(
                mrb,
                c"ECONNRESET",
                "Connection reset by peer",
            )),
        }
    }

    /// Every byte left, to the end.
    fn rest(&mut self, mrb: &Mrb) -> Result<Vec<u8>, Error> {
        let mut rest = Vec::new();
        while let Some(chunk) = self.chunk(mrb)? {
            rest.extend(chunk);
        }
        Ok(rest)
    }

    fn finish(&mut self) {
        drop(self.stream.take());
        if let Some(body) = self.body.take() {
            drop(IncomingBody::finish(body));
        }
    }
}

impl Drop for Unread {
    fn drop(&mut self) {
        self.finish();
    }
}

thread_local! {
    /// The bodies a request's block may still stream, each beside the object
    /// id of the response it belongs to, innermost last.
    static STREAMING: RefCell<Vec<(i64, Unread)>> = const { RefCell::new(Vec::new()) };
}

/// One request the guest sends: where, how, and what.
struct Outgoing<'a> {
    ssl: bool,
    host: &'a str,
    port: u16,
    method: &'a str,
    path: &'a str,
    headers: Vec<(String, Vec<u8>)>,
    body: Option<Vec<u8>>,
    limits: Limits,
}

pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let object = mrb.class_object();
    let net = mrb.define_module(c"Net")?;
    let standard = mrb.exception_standard_error();
    // As CRuby's timeout library and net-protocol define them.
    let runtime = mrb.exc_get(c"RuntimeError")?;
    let timeout = mrb
        .define_module(c"Timeout")?
        .define_error(mrb, c"Error", runtime)?;
    for name in [c"OpenTimeout", c"ReadTimeout", c"WriteTimeout"] {
        net.define_error(mrb, name, timeout)?;
    }
    net.define_error(mrb, c"HTTPBadResponse", standard)?;
    net.define_error(mrb, c"HTTPHeaderSyntaxError", standard)?;
    define_http_exceptions(mrb, net, standard)?;
    let openssl = mrb.define_module(c"OpenSSL")?;
    let openssl_error = openssl.define_error(mrb, c"OpenSSLError", standard)?;
    openssl
        .define_module(mrb, c"SSL")?
        .define_error(mrb, c"SSLError", openssl_error)?;

    let http = net.define_class(mrb, c"HTTP", object)?;
    http.define_singleton_method(mrb, c"start", beni::method!(http_s_start, -1))?;
    http.define_singleton_method(mrb, c"get", beni::method!(http_s_get, -1))?;
    http.define_singleton_method(mrb, c"get_response", beni::method!(http_s_get_response, -1))?;
    http.define_method(mrb, c"initialize", beni::method!(http_initialize, -1))?;
    http.define_method(mrb, c"address", beni::method!(http_address, 0))?;
    http.define_method(mrb, c"port", beni::method!(http_port, 0))?;
    http.define_method(mrb, c"use_ssl?", beni::method!(http_use_ssl_p, 0))?;
    http.define_method(mrb, c"use_ssl=", beni::method!(http_set_use_ssl, 1))?;
    http.define_method(mrb, c"open_timeout", beni::method!(http_open_timeout, 0))?;
    http.define_method(
        mrb,
        c"open_timeout=",
        beni::method!(http_set_open_timeout, 1),
    )?;
    http.define_method(mrb, c"read_timeout", beni::method!(http_read_timeout, 0))?;
    http.define_method(
        mrb,
        c"read_timeout=",
        beni::method!(http_set_read_timeout, 1),
    )?;
    http.define_method(mrb, c"write_timeout", beni::method!(http_write_timeout, 0))?;
    http.define_method(
        mrb,
        c"write_timeout=",
        beni::method!(http_set_write_timeout, 1),
    )?;
    http.define_method(mrb, c"start", beni::method!(http_start, 0))?;
    http.define_method(mrb, c"finish", beni::method!(http_finish, 0))?;
    http.define_method(mrb, c"started?", beni::method!(http_started_p, 0))?;
    http.define_method(mrb, c"request", beni::method!(http_request, -1))?;
    for (name, func) in [
        (c"get", beni::method!(http_get, -1)),
        (c"head", beni::method!(http_head, -1)),
        (c"delete", beni::method!(http_delete, -1)),
        (c"options", beni::method!(http_options, -1)),
        (c"post", beni::method!(http_post, -1)),
        (c"put", beni::method!(http_put, -1)),
        (c"patch", beni::method!(http_patch, -1)),
    ] {
        http.define_method(mrb, name, func)?;
    }

    let generic = net.define_class(mrb, c"HTTPGenericRequest", object)?;
    generic.define_method(mrb, c"initialize", beni::method!(request_initialize, -1))?;
    generic.define_method(mrb, c"method", beni::method!(request_method, 0))?;
    generic.define_method(mrb, c"path", beni::method!(request_path, 0))?;
    generic.define_method(mrb, c"body", beni::method!(request_body, 0))?;
    generic.define_method(mrb, c"body=", beni::method!(request_set_body, 1))?;
    generic.define_method(mrb, c"[]", beni::method!(header_aref, 1))?;
    generic.define_method(mrb, c"[]=", beni::method!(request_aset, 2))?;
    generic.define_method(mrb, c"each_header", beni::method!(header_each, 0))?;
    generic.define_method(mrb, c"key?", beni::method!(header_key_p, 1))?;
    generic.define_method(mrb, c"delete", beni::method!(header_delete, 1))?;
    generic.define_method(mrb, c"content_type", beni::method!(header_content_type, 0))?;
    generic.define_method(
        mrb,
        c"content_type=",
        beni::method!(request_set_content_type, 1),
    )?;
    generic.define_method(mrb, c"basic_auth", beni::method!(request_basic_auth, 2))?;
    generic.define_method(
        mrb,
        c"set_form_data",
        beni::method!(request_set_form_data, -1),
    )?;
    generic.define_method(mrb, c"form_data=", beni::method!(request_set_form_data, -1))?;
    let request = net.define_class(mrb, c"HTTPRequest", generic)?;
    for (name, verb, has_body) in [
        (c"Get", "GET", false),
        (c"Head", "HEAD", false),
        (c"Delete", "DELETE", false),
        (c"Options", "OPTIONS", false),
        (c"Post", "POST", true),
        (c"Put", "PUT", true),
        (c"Patch", "PATCH", true),
    ] {
        let class = http.define_class(mrb, name, request)?;
        class.const_set(mrb, c"METHOD", mrb.str_new(verb.as_bytes()))?;
        class.const_set(mrb, c"REQUEST_HAS_BODY", has_body)?;
    }

    let response = net.define_class(mrb, c"HTTPResponse", object)?;
    response.define_method(mrb, c"code", beni::method!(response_code, 0))?;
    response.define_method(mrb, c"message", beni::method!(response_message, 0))?;
    response.define_method(mrb, c"body", beni::method!(response_read_body, 0))?;
    response.define_method(mrb, c"read_body", beni::method!(response_read_body, 0))?;
    response.define_method(mrb, c"value", beni::method!(response_value, 0))?;
    response.define_method(mrb, c"[]", beni::method!(header_aref, 1))?;
    response.define_method(mrb, c"each_header", beni::method!(header_each, 0))?;
    response.define_method(mrb, c"to_hash", beni::method!(header_to_hash, 0))?;
    response.define_method(mrb, c"key?", beni::method!(header_key_p, 1))?;
    response.define_method(mrb, c"content_type", beni::method!(header_content_type, 0))?;
    response.define_method(
        mrb,
        c"content_length",
        beni::method!(response_content_length, 0),
    )?;
    response.define_method(mrb, c"header", beni::method!(response_header, 0))?;
    response.define_method(
        mrb,
        c"http_version",
        beni::method!(response_http_version, 0),
    )?;
    response.define_method(mrb, c"code_type", beni::method!(response_code_type, 0))?;
    for name in [
        c"HTTPInformation",
        c"HTTPSuccess",
        c"HTTPRedirection",
        c"HTTPClientError",
        c"HTTPServerError",
        c"HTTPUnknownResponse",
    ] {
        net.define_class(mrb, name, response)?;
    }
    for (_, name, family) in STATUS_CLASSES {
        let family: RClass = net.const_get(mrb, *family)?;
        net.define_class(mrb, *name, family)?;
    }
    Ok(())
}

/// The errors `value` raises, each beneath its `Net::Proto*` family and
/// carrying `response` through `Net::HTTPExceptions`, as CRuby's are.
fn define_http_exceptions(mrb: &Mrb, net: RModule, standard: ExceptionClass) -> Result<(), Error> {
    let protocol = net.define_error(mrb, c"ProtocolError", standard)?;
    let exceptions = net.define_module(mrb, c"HTTPExceptions")?;
    exceptions.define_method(mrb, c"response", beni::method!(exception_response, 0))?;
    for (name, family) in [
        (c"HTTPError", None),
        (c"HTTPRetriableError", Some(c"ProtoRetriableError")),
        (c"HTTPClientException", Some(c"ProtoServerError")),
        (c"HTTPFatalError", Some(c"ProtoFatalError")),
    ] {
        let parent = match family {
            Some(family) => net.define_error(mrb, family, protocol)?,
            None => protocol,
        };
        net.define_error(mrb, name, parent)?
            .include_module(mrb, exceptions)?;
    }
    Ok(())
}

fn object(mrb: &Mrb, value: Value) -> Result<RObject, Error> {
    RObject::from_value(value).ok_or_else(|| argument_error(mrb, "not a Net object"))
}

fn ivar(mrb: &Mrb, self_: Value, name: &core::ffi::CStr) -> Result<Value, Error> {
    object(mrb, self_)?.ivar_get(mrb, name)
}

fn set_ivar(mrb: &Mrb, self_: Value, name: &core::ffi::CStr, value: Value) -> Result<(), Error> {
    object(mrb, self_)?.ivar_set(mrb, name, value)
}

fn to_text(mrb: &Mrb, value: Value) -> Result<String, Error> {
    value.to_r_string(mrb)?.to_string(mrb)
}

fn to_bytes(mrb: &Mrb, value: Value) -> Result<Vec<u8>, Error> {
    let string = value.to_r_string(mrb)?;
    // SAFETY: the bytes are copied out before the next mruby call.
    Ok(unsafe { string.as_bytes(mrb) }.to_vec())
}

/// `Net::HTTP.new(address, port = nil)`: HTTP on port 80 unless written.
fn http_initialize(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (address, port) = match args {
        [address] => (*address, qnil().as_value()),
        [address, port, ..] => (*address, *port),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    set_ivar(
        mrb,
        self_,
        c"@address",
        to_text(mrb, address)?.into_value(mrb),
    )?;
    set_ivar(mrb, self_, c"@port", port)?;
    set_ivar(mrb, self_, c"@use_ssl", false.into_value(mrb))?;
    for name in [c"@open_timeout", c"@read_timeout", c"@write_timeout"] {
        set_ivar(mrb, self_, name, DEFAULT_TIMEOUT.into_value(mrb))?;
    }
    Ok(qnil().as_value())
}

/// The seconds each of `Net::HTTP`'s timeouts starts at, as CRuby's.
const DEFAULT_TIMEOUT: i64 = 60;

fn http_open_timeout(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@open_timeout")
}

fn http_set_open_timeout(mrb: &Mrb, self_: Value, sec: Value) -> Result<Value, Error> {
    set_ivar(mrb, self_, c"@open_timeout", sec)?;
    Ok(sec)
}

fn http_read_timeout(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@read_timeout")
}

fn http_set_read_timeout(mrb: &Mrb, self_: Value, sec: Value) -> Result<Value, Error> {
    set_ivar(mrb, self_, c"@read_timeout", sec)?;
    Ok(sec)
}

fn http_write_timeout(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@write_timeout")
}

fn http_set_write_timeout(mrb: &Mrb, self_: Value, sec: Value) -> Result<Value, Error> {
    set_ivar(mrb, self_, c"@write_timeout", sec)?;
    Ok(sec)
}

/// The limit a timeout `ivar` sets, `None` for `nil`, which waits on.
fn limit_of(mrb: &Mrb, self_: Value, name: &core::ffi::CStr) -> Result<Option<Duration>, Error> {
    let sec = ivar(mrb, self_, name)?;
    if sec.is_nil() {
        return Ok(None);
    }
    let sec = f64::try_convert(sec, mrb)?;
    Duration::try_from_secs_f64(sec)
        .map(Some)
        .map_err(|_| argument_error(mrb, "time interval must not be negative"))
}

/// How long a request may wait to read and to write. The wait for a response's
/// head takes the connect too, which `wasi:http` shows the guest no apart from
/// it, so `read_timeout` bounds the whole wait and `open_timeout` none of it.
#[derive(Clone, Copy)]
struct Limits {
    read: Option<Duration>,
    write: Option<Duration>,
}

impl Limits {
    fn of(mrb: &Mrb, self_: Value) -> Result<Self, Error> {
        Ok(Self {
            read: limit_of(mrb, self_, c"@read_timeout")?,
            write: limit_of(mrb, self_, c"@write_timeout")?,
        })
    }
}

fn http_address(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@address")
}

/// The port given, or the scheme's own.
fn http_port(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(i64::from(port_of(mrb, self_)?).into_value(mrb))
}

fn port_of(mrb: &Mrb, self_: Value) -> Result<u16, Error> {
    let port = ivar(mrb, self_, c"@port")?;
    if port.is_nil() {
        return Ok(if use_ssl(mrb, self_)? { 443 } else { 80 });
    }
    u16::try_from(i64::try_convert(port, mrb)?)
        .map_err(|_| argument_error(mrb, "port out of range"))
}

fn use_ssl(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(ivar(mrb, self_, c"@use_ssl")?.to_bool())
}

fn http_use_ssl_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    use_ssl(mrb, self_)
}

fn http_set_use_ssl(mrb: &Mrb, self_: Value, flag: Value) -> Result<Value, Error> {
    set_ivar(mrb, self_, c"@use_ssl", flag.to_bool().into_value(mrb))?;
    Ok(flag)
}

/// Each request opens its own connection, so a session holds nothing open.
fn http_start(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let block = current_block(mrb)?;
    session(mrb, self_, block)
}

/// The block the method running now was handed, whatever its arguments.
fn current_block(mrb: &Mrb) -> Result<Option<Proc>, Error> {
    Ok(scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?.block)
}

/// Start a session, yielding to `block` and finishing once it returns.
fn session(mrb: &Mrb, self_: Value, block: Option<Proc>) -> Result<Value, Error> {
    set_ivar(mrb, self_, c"@started", true.into_value(mrb))?;
    let Some(block) = block else {
        return Ok(self_);
    };
    let answer = block.call(mrb, [self_]);
    set_ivar(mrb, self_, c"@started", false.into_value(mrb))?;
    answer
}

fn http_finish(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    set_ivar(mrb, self_, c"@started", false.into_value(mrb))?;
    Ok(qnil().as_value())
}

fn http_started_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(ivar(mrb, self_, c"@started")?.to_bool())
}

/// `Net::HTTP.start(address, port = nil, use_ssl: false, open_timeout: 60,
/// read_timeout: 60, write_timeout: 60) { |http| ... }`.
fn http_s_start(mrb: &Mrb, class: RClass, args: &[Value]) -> Result<Value, Error> {
    let block = current_block(mrb)?;
    let mut positional = args.to_vec();
    let options = positional.last().copied().and_then(RHash::from_value);
    if options.is_some() {
        positional.pop();
    }
    let http: Value = class.funcall(mrb, c"new", positional.as_slice())?;
    if let Some(options) = options {
        set_options(mrb, http, options)?;
    }
    session(mrb, http, block)
}

/// Set what each option `Net::HTTP.start` takes names, as its writer would.
fn set_options(mrb: &Mrb, http: Value, options: RHash) -> Result<(), Error> {
    for (key, name) in [
        ("use_ssl", c"@use_ssl"),
        ("open_timeout", c"@open_timeout"),
        ("read_timeout", c"@read_timeout"),
        ("write_timeout", c"@write_timeout"),
    ] {
        let key = Symbol::new(mrb, key)?;
        if options.funcall::<_, _, bool>(mrb, c"key?", [key.as_value()])? {
            let value = options.aref::<_, Value>(mrb, key)?;
            match name == c"@use_ssl" {
                true => set_ivar(mrb, http, name, value.to_bool().into_value(mrb))?,
                false => set_ivar(mrb, http, name, value)?,
            }
        }
    }
    Ok(())
}

/// `Net::HTTP.get(host, path, port = nil)`: the body of a GET.
fn http_s_get(mrb: &Mrb, class: RClass, args: &[Value]) -> Result<Value, Error> {
    let response = get_response(mrb, class, args, None)?;
    ivar(mrb, response, c"@body")
}

/// `Net::HTTP.get_response(host, path, port = nil) { |response| }`: the
/// response to a GET, handed to a block before its body is read.
fn http_s_get_response(mrb: &Mrb, class: RClass, args: &[Value]) -> Result<Value, Error> {
    let given = Given::current(mrb, Given::Response)?;
    get_response(mrb, class, args, given)
}

fn get_response(
    mrb: &Mrb,
    class: RClass,
    args: &[Value],
    given: Option<Given>,
) -> Result<Value, Error> {
    let (host, path, port) = match args {
        [host, path] => (*host, *path, qnil().as_value()),
        [host, path, port] => (*host, *path, *port),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 3)),
    };
    let http: Value = class.funcall(mrb, c"new", [host, port])?;
    let path = to_text(mrb, path)?;
    send_from(mrb, http, "GET", &path, Vec::new(), None, given)
}

fn http_get(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    bodyless(mrb, self_, "GET", args)
}

fn http_head(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    bodyless(mrb, self_, "HEAD", args)
}

fn http_delete(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    bodyless(mrb, self_, "DELETE", args)
}

fn http_options(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    bodyless(mrb, self_, "OPTIONS", args)
}

fn http_post(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    bodied(mrb, self_, "POST", args)
}

fn http_put(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    bodied(mrb, self_, "PUT", args)
}

fn http_patch(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    bodied(mrb, self_, "PATCH", args)
}

/// `get(path, initheader = nil) { |chunk| }` and its kin with no body; a block
/// is handed the body as it arrives.
fn bodyless(mrb: &Mrb, self_: Value, method: &str, args: &[Value]) -> Result<Value, Error> {
    let (path, header) = match args {
        [path] => (*path, qnil().as_value()),
        [path, header] => (*path, *header),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let path = to_text(mrb, path)?;
    let headers = headers_given(mrb, header)?;
    let given = Given::current(mrb, Given::Chunks)?;
    send_from(mrb, self_, method, &path, headers, None, given)
}

/// `post(path, data, initheader = nil) { |chunk| }` and its kin with a body;
/// a block is handed the response's body as it arrives.
fn bodied(mrb: &Mrb, self_: Value, method: &str, args: &[Value]) -> Result<Value, Error> {
    let (path, data, header) = match args {
        [path, data] => (*path, *data, qnil().as_value()),
        [path, data, header] => (*path, *data, *header),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 3)),
    };
    let path = to_text(mrb, path)?;
    let body = to_bytes(mrb, data)?;
    let headers = headers_given(mrb, header)?;
    let given = Given::current(mrb, Given::Chunks)?;
    send_from(mrb, self_, method, &path, headers, Some(body), given)
}

/// `request(req, body = nil) { |response| }`: send a `Net::HTTP::Get` or one
/// of its kin; a block is handed the response before its body is read.
fn http_request(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (request, body) = match args {
        [request] => (*request, qnil().as_value()),
        [request, body] => (*request, *body),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let method = to_text(mrb, ivar(mrb, request, c"@method")?)?;
    let path = to_text(mrb, ivar(mrb, request, c"@path")?)?;
    let headers = headers_of(mrb, ivar(mrb, request, c"@header")?)?;
    let body = match (body.is_nil(), ivar(mrb, request, c"@body")?) {
        (false, _) => Some(to_bytes(mrb, body)?),
        (true, held) if !held.is_nil() => Some(to_bytes(mrb, held)?),
        _ => None,
    };
    let given = Given::current(mrb, Given::Response)?;
    send_from(mrb, self_, &method, &path, headers, body, given)
}

/// Headers handed as a Hash, or none.
fn headers_given(mrb: &Mrb, header: Value) -> Result<Vec<(String, Vec<u8>)>, Error> {
    if header.is_nil() {
        return Ok(Vec::new());
    }
    let hash = RHash::from_value(header)
        .ok_or_else(|| argument_error(mrb, "initheader must be a Hash"))?;
    let mut headers = Vec::new();
    hash.foreach(mrb, |name: Value, value: Value| {
        headers.push((
            to_text(mrb, name)?.to_ascii_lowercase(),
            to_bytes(mrb, value)?,
        ));
        Ok(ForEach::Continue)
    })?;
    Ok(headers)
}

/// A request's `@header`, each name to its list of values.
fn headers_of(mrb: &Mrb, header: Value) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let Some(hash) = RHash::from_value(header) else {
        return Ok(Vec::new());
    };
    let mut headers = Vec::new();
    hash.foreach(mrb, |name: Value, values: RArray| {
        let name = to_text(mrb, name)?;
        for index in 0..values.len() {
            headers.push((
                name.clone(),
                to_bytes(mrb, values.entry(mrb, index as isize)?)?,
            ));
        }
        Ok(ForEach::Continue)
    })?;
    Ok(headers)
}

/// What a request's block is handed: the response, as `request` yields it, or
/// each chunk of its body, as `get` and its kin yield them.
enum Given {
    Response(Proc),
    Chunks(Proc),
}

impl Given {
    /// The block the method running now was handed, as `kind` takes it.
    fn current(mrb: &Mrb, kind: fn(Proc) -> Given) -> Result<Option<Given>, Error> {
        Ok(current_block(mrb)?.map(kind))
    }
}

fn send_from(
    mrb: &Mrb,
    self_: Value,
    method: &str,
    path: &str,
    headers: Vec<(String, Vec<u8>)>,
    body: Option<Vec<u8>>,
    given: Option<Given>,
) -> Result<Value, Error> {
    let host = to_text(mrb, ivar(mrb, self_, c"@address")?)?;
    let outgoing = Outgoing {
        ssl: use_ssl(mrb, self_)?,
        host: &host,
        port: port_of(mrb, self_)?,
        method,
        path,
        headers,
        body,
        limits: Limits::of(mrb, self_)?,
    };
    let Answer {
        status,
        headers,
        body: mut unread,
    } = send(mrb, &outgoing)?;
    let Some(given) = given else {
        let body = unread.rest(mrb)?;
        return to_response(mrb, status, headers, Some(&body));
    };
    let response = to_response(mrb, status, headers, None)?;
    let streaming = Streaming::begin(response.object_id(), unread);
    match given {
        Given::Response(block) => block.call::<_, Value>(mrb, [response])?,
        Given::Chunks(block) => read_body_into(mrb, response, Some(block))?,
    };
    // What the block left unread is read once it returns, so `body` answers
    // after it, as CRuby reads the rest.
    if let Some(mut unread) = streaming.end() {
        let rest = unread.rest(mrb)?;
        set_ivar(mrb, response, c"@body", mrb.str_new(&rest).as_value())?;
    }
    Ok(response)
}

/// A response's body open to its request's block, taken back as the block
/// returns or fails.
struct Streaming(i64);

impl Streaming {
    fn begin(id: i64, unread: Unread) -> Self {
        STREAMING.with_borrow_mut(|open| open.push((id, unread)));
        Self(id)
    }

    /// The body, unless `read_body` took it.
    fn end(self) -> Option<Unread> {
        take_streaming(self.0)
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        drop(take_streaming(self.0));
    }
}

/// The unread body of the response `id` names, if a block still streams it.
fn take_streaming(id: i64) -> Option<Unread> {
    STREAMING.with_borrow_mut(|open| {
        let at = open.iter().position(|(held, _)| *held == id)?;
        Some(open.remove(at).1)
    })
}

fn method_of(method: &str) -> Method {
    match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "OPTIONS" => Method::Options,
        "PATCH" => Method::Patch,
        "TRACE" => Method::Trace,
        "CONNECT" => Method::Connect,
        other => Method::Other(other.to_string()),
    }
}

/// Send `outgoing` and answer its response's head, its body left to read. A
/// header the seam frames itself, such as `content-length` or `host`, is left
/// to it.
fn send(mrb: &Mrb, outgoing: &Outgoing<'_>) -> Result<Answer, Error> {
    let fields = Fields::new();
    for (name, value) in &outgoing.headers {
        let _ = fields.append(name, value);
    }
    let request = OutgoingRequest::new(fields);
    let failed = |what: &str| errno::io_error(mrb, what);
    request
        .set_method(&method_of(outgoing.method))
        .map_err(|()| argument_error(mrb, "invalid HTTP method"))?;
    let scheme = if outgoing.ssl {
        Scheme::Https
    } else {
        Scheme::Http
    };
    request
        .set_scheme(Some(&scheme))
        .map_err(|()| failed("invalid scheme"))?;
    request
        .set_authority(Some(&format!("{}:{}", outgoing.host, outgoing.port)))
        .map_err(|()| argument_error(mrb, "invalid host"))?;
    request
        .set_path_with_query(Some(outgoing.path))
        .map_err(|()| argument_error(mrb, "invalid path"))?;
    let out = request.body().map_err(|()| failed("request body taken"))?;
    if let Some(body) = &outgoing.body {
        let stream = out.write().map_err(|()| failed("request body taken"))?;
        let written = write_within(&stream, body, outgoing.limits.write)
            .map_err(|_| errno::error_by_name(mrb, c"ENOMEM", "request body"))?;
        if !written {
            return Err(net_error(mrb, c"WriteTimeout", "Net::WriteTimeout"));
        }
    }
    OutgoingBody::finish(out, None).map_err(|code| to_error(mrb, outgoing, code))?;
    let future = handle(request, None).map_err(|code| to_error(mrb, outgoing, code))?;
    let response = loop {
        match future.get() {
            Some(answer) => break answer,
            None => {
                if !wait_within(&future.subscribe(), outgoing.limits.read) {
                    return Err(net_error(mrb, c"ReadTimeout", "Net::ReadTimeout"));
                }
            }
        }
    };
    let response = response
        .map_err(|()| failed("response taken"))?
        .map_err(|code| to_error(mrb, outgoing, code))?;
    let status = response.status();
    let headers = response.headers().entries();
    let incoming = response
        .consume()
        .map_err(|()| failed("response body taken"))?;
    let stream = incoming
        .stream()
        .map_err(|()| failed("response body taken"))?;
    Ok(Answer {
        status,
        headers,
        body: Unread {
            stream: Some(stream),
            body: Some(incoming),
            read: outgoing.limits.read,
        },
    })
}

/// What CRuby raises for a request that found no response, wrapped as its
/// `Net::HTTP#connect` wraps a failed open.
fn to_error(mrb: &Mrb, outgoing: &Outgoing<'_>, code: ErrorCode) -> Error {
    let (host, port) = (outgoing.host, outgoing.port);
    let opening =
        |detail: &str| format!("Failed to open TCP connection to {host}:{port} ({detail})");
    let connect = |name: &core::ffi::CStr| {
        let detail = errno::message_of(name, &format!("connect(2) for \"{host}\" port {port}"));
        errno::error_with_message(mrb, name, &opening(&detail))
    };
    match code {
        ErrorCode::HttpRequestDenied if !socket::network_given() => errno::not_implemented(
            mrb,
            "Net::HTTP is unimplemented: this Sandbox gives the guest no network",
        ),
        ErrorCode::HttpRequestDenied => connect(c"EACCES"),
        ErrorCode::ConnectionRefused => connect(c"ECONNREFUSED"),
        ErrorCode::ConnectionLimitReached => connect(c"EAGAIN"),
        ErrorCode::DestinationNotFound | ErrorCode::DnsError(_) => {
            let (code, words) = socket::EAI_NONAME;
            socket::resolution_error_with(mrb, code, &opening(&format!("getaddrinfo: {words}")))
        }
        ErrorCode::ConnectionTimeout => {
            net_error(mrb, c"OpenTimeout", &opening("execution expired"))
        }
        ErrorCode::ConnectionReadTimeout => net_error(mrb, c"ReadTimeout", "Net::ReadTimeout"),
        ErrorCode::ConnectionTerminated | ErrorCode::HttpResponseIncomplete => Error::new(
            mrb,
            mrb.exc_get(c"EOFError")
                .unwrap_or_else(|_| mrb.exception_standard_error()),
            "end of file reached",
        ),
        ErrorCode::HttpProtocolError => net_error(mrb, c"HTTPBadResponse", "wrong status line"),
        ErrorCode::TlsCertificateError => ssl_error(mrb, "certificate verify failed"),
        ErrorCode::TlsProtocolError | ErrorCode::TlsAlertReceived(_) => {
            ssl_error(mrb, "SSL_connect returned=1 errno=0 state=error")
        }
        _ => errno::io_error(mrb, "the HTTP request failed"),
    }
}

/// The `OpenSSL::SSL::SSLError` CRuby raises for TLS that failed, here ended on
/// the host.
fn ssl_error(mrb: &Mrb, message: &str) -> Error {
    match mrb
        .module_get(c"OpenSSL")
        .and_then(|openssl| openssl.const_get::<_, RModule>(mrb, c"SSL"))
        .and_then(|ssl| ssl.const_get::<_, ExceptionClass>(mrb, c"SSLError"))
    {
        Ok(class) => Error::new(mrb, class, message),
        Err(err) => err,
    }
}

fn net_error(mrb: &Mrb, name: &core::ffi::CStr, message: &str) -> Error {
    match mrb
        .module_get(c"Net")
        .and_then(|net| net.const_get::<_, ExceptionClass>(mrb, name))
    {
        Ok(class) => Error::new(mrb, class, message),
        Err(err) => err,
    }
}

/// The reason phrase HTTP/1.1 gives `status`, as CRuby reads it from the
/// status line; WASI carries only the code.
fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// The statuses CRuby gives a class of their own, each beneath its family, as
/// `Net::HTTPOK` sits beneath `Net::HTTPSuccess`.
const STATUS_CLASSES: &[(u16, &core::ffi::CStr, &core::ffi::CStr)] = &[
    (200, c"HTTPOK", c"HTTPSuccess"),
    (201, c"HTTPCreated", c"HTTPSuccess"),
    (202, c"HTTPAccepted", c"HTTPSuccess"),
    (204, c"HTTPNoContent", c"HTTPSuccess"),
    (206, c"HTTPPartialContent", c"HTTPSuccess"),
    (301, c"HTTPMovedPermanently", c"HTTPRedirection"),
    (302, c"HTTPFound", c"HTTPRedirection"),
    (303, c"HTTPSeeOther", c"HTTPRedirection"),
    (304, c"HTTPNotModified", c"HTTPRedirection"),
    (307, c"HTTPTemporaryRedirect", c"HTTPRedirection"),
    (308, c"HTTPPermanentRedirect", c"HTTPRedirection"),
    (400, c"HTTPBadRequest", c"HTTPClientError"),
    (401, c"HTTPUnauthorized", c"HTTPClientError"),
    (403, c"HTTPForbidden", c"HTTPClientError"),
    (404, c"HTTPNotFound", c"HTTPClientError"),
    (405, c"HTTPMethodNotAllowed", c"HTTPClientError"),
    (409, c"HTTPConflict", c"HTTPClientError"),
    (422, c"HTTPUnprocessableEntity", c"HTTPClientError"),
    (429, c"HTTPTooManyRequests", c"HTTPClientError"),
    (500, c"HTTPInternalServerError", c"HTTPServerError"),
    (502, c"HTTPBadGateway", c"HTTPServerError"),
    (503, c"HTTPServiceUnavailable", c"HTTPServerError"),
    (504, c"HTTPGatewayTimeout", c"HTTPServerError"),
];

/// The `Net::HTTPResponse` subclass a status stands for: its own, or its
/// family's.
fn response_class(mrb: &Mrb, status: u16) -> Result<RClass, Error> {
    if let Some((_, name, _)) = STATUS_CLASSES.iter().find(|(code, ..)| *code == status) {
        return mrb.module_get(c"Net")?.const_get(mrb, *name);
    }
    let name: &core::ffi::CStr = match status {
        100..=199 => c"HTTPInformation",
        200..=299 => c"HTTPSuccess",
        300..=399 => c"HTTPRedirection",
        400..=499 => c"HTTPClientError",
        500..=599 => c"HTTPServerError",
        _ => c"HTTPUnknownResponse",
    };
    mrb.module_get(c"Net")?.const_get(mrb, name)
}

/// The `Net::HTTPResponse` for `status` and `headers`, holding `body` once it
/// was read; one a block still streams holds none yet.
fn to_response(
    mrb: &Mrb,
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Option<&[u8]>,
) -> Result<Value, Error> {
    let response: Value = response_class(mrb, status)?.funcall(mrb, c"new", ())?;
    let header = mrb.hash_new();
    for (name, value) in headers {
        let key = mrb.str_new(name.to_ascii_lowercase().as_bytes());
        let values = match header.aref::<_, Value>(mrb, key)? {
            held if held.is_nil() => {
                let values = mrb.ary_new();
                header.aset(mrb, key, values)?;
                values
            }
            held => RArray::try_convert(held, mrb)?,
        };
        values.push(mrb, mrb.str_new(&value).as_value())?;
    }
    set_ivar(mrb, response, c"@code", status.to_string().into_value(mrb))?;
    set_ivar(mrb, response, c"@message", reason(status).into_value(mrb))?;
    set_ivar(mrb, response, c"@header", header.as_value())?;
    let body = body.map_or_else(|| qnil().as_value(), |body| mrb.str_new(body).as_value());
    set_ivar(mrb, response, c"@body", body)?;
    Ok(response)
}

fn response_code(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@code")
}

fn response_message(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@message")
}

/// `body` and `read_body { |chunk| }`: the body, read to the end, or each
/// chunk of it handed to a block as it arrives, as CRuby streams it.
fn response_read_body(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let block = current_block(mrb)?;
    read_body_into(mrb, self_, block)
}

/// The body of `response`, or each chunk of it handed to `block`. Once the
/// body is read, a block raises `IOError`, as CRuby's second `read_body` does.
fn read_body_into(mrb: &Mrb, response: Value, block: Option<Proc>) -> Result<Value, Error> {
    let Some(mut unread) = take_streaming(response.object_id()) else {
        if block.is_some() {
            let class: Value = response.funcall(mrb, c"class", ())?;
            let message = format!("{}#read_body called twice", to_text(mrb, class)?);
            return Err(errno::io_error(mrb, &message));
        }
        return ivar(mrb, response, c"@body");
    };
    let Some(block) = block else {
        let body = mrb.str_new(&unread.rest(mrb)?).as_value();
        set_ivar(mrb, response, c"@body", body)?;
        return Ok(body);
    };
    let mut since_collected = 0;
    while let Some(chunk) = unread.chunk(mrb)? {
        // Each chunk is released once the block returns, and collected as the
        // stream goes on, since mruby counts objects rather than bytes; a body
        // larger than the guest's memory streams through the block.
        let _scope = mrb.arena_scope();
        since_collected += chunk.len();
        block.call::<_, Value>(mrb, [mrb.str_new(&chunk).as_value()])?;
        if since_collected >= COLLECT_EVERY {
            mrb.gc_start();
            since_collected = 0;
        }
    }
    Ok(qnil().as_value())
}

/// `value`: `nil` for a success, else the error CRuby raises for the
/// response's family, carrying it as `response`.
fn response_value(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let code = to_text(mrb, ivar(mrb, self_, c"@code")?)?;
    let name: &core::ffi::CStr = match code.as_bytes().first() {
        Some(b'2') => return Ok(qnil().as_value()),
        Some(b'3') => c"HTTPRetriableError",
        Some(b'4') => c"HTTPClientException",
        Some(b'5') => c"HTTPFatalError",
        _ => c"HTTPError",
    };
    let message = to_text(mrb, ivar(mrb, self_, c"@message")?)?;
    let class: ExceptionClass = mrb.module_get(c"Net")?.const_get(mrb, name)?;
    let exception = class.new_str(mrb, mrb.str_new(format!("{code} {message:?}").as_bytes()));
    exception.ivar_set(mrb, c"@response", self_)?;
    Err(Error::Exception(exception.as_value()))
}

/// `response` on an error `value` raised: the response it was raised for.
fn exception_response(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    self_.funcall(
        mrb,
        c"instance_variable_get",
        [Symbol::new(mrb, "@response")?.as_value()],
    )
}

/// `content_length`: the `content-length` header as an Integer, or `nil`.
fn response_content_length(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let length = header_aref(mrb, self_, mrb.str_new(b"content-length").as_value())?;
    if length.is_nil() {
        return Ok(length);
    }
    let text = to_text(mrb, length)?;
    match text.trim().parse::<i64>() {
        Ok(length) => Ok(length.into_value(mrb)),
        Err(_) => Err(net_error(
            mrb,
            c"HTTPHeaderSyntaxError",
            "wrong Content-Length format",
        )),
    }
}

/// `header`: the response itself, as CRuby answers it.
fn response_header(_mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(self_)
}

/// `http_version`: the HTTP/1.1 the host speaks, as WASI carries none.
fn response_http_version(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    Ok("1.1".into_value(mrb))
}

/// `code_type`: the response's own class, as CRuby's maps its code.
fn response_code_type(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    self_.funcall(mrb, c"class", ())
}

/// `Net::HTTP::Get.new(path, initheader = nil)` and its kin.
fn request_initialize(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (path, header) = match args {
        [path] => (*path, qnil().as_value()),
        [path, header] => (*path, *header),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let class: Value = self_.funcall(mrb, c"class", ())?;
    let method: Value =
        class.funcall(mrb, c"const_get", [Symbol::new(mrb, "METHOD")?.as_value()])?;
    set_ivar(mrb, self_, c"@method", method)?;
    set_ivar(mrb, self_, c"@path", to_text(mrb, path)?.into_value(mrb))?;
    let hash = mrb.hash_new();
    for (name, value) in headers_given(mrb, header)? {
        let values = mrb.ary_new();
        values.push(mrb, mrb.str_new(&value).as_value())?;
        hash.aset(mrb, mrb.str_new(name.as_bytes()), values)?;
    }
    set_ivar(mrb, self_, c"@header", hash.as_value())?;
    Ok(qnil().as_value())
}

fn request_method(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@method")
}

fn request_path(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@path")
}

fn request_body(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    ivar(mrb, self_, c"@body")
}

fn request_set_body(mrb: &Mrb, self_: Value, body: Value) -> Result<Value, Error> {
    set_ivar(mrb, self_, c"@body", body)?;
    Ok(body)
}

/// `req[name] = value` sets the header, case-insensitively; `nil` removes it.
fn request_aset(mrb: &Mrb, self_: Value, name: Value, value: Value) -> Result<Value, Error> {
    let header = RHash::try_convert(ivar(mrb, self_, c"@header")?, mrb)?;
    let key = mrb.str_new(to_text(mrb, name)?.to_ascii_lowercase().as_bytes());
    if value.is_nil() {
        header.delete::<_, Value>(mrb, key)?;
        return Ok(value);
    }
    let values = mrb.ary_new();
    values.push(mrb, value.to_r_string(mrb)?.as_value())?;
    header.aset(mrb, key, values)?;
    Ok(value)
}

/// `[name]`: the header's values joined, as CRuby joins them, or `nil`.
fn header_aref(mrb: &Mrb, self_: Value, name: Value) -> Result<Value, Error> {
    let header = RHash::try_convert(ivar(mrb, self_, c"@header")?, mrb)?;
    let key = mrb.str_new(to_text(mrb, name)?.to_ascii_lowercase().as_bytes());
    let values = header.aref::<_, Value>(mrb, key)?;
    if values.is_nil() {
        return Ok(values);
    }
    RArray::try_convert(values, mrb)?.funcall(mrb, c"join", [mrb.str_new(b", ").as_value()])
}

/// `each_header { |name, value| }` over each header, its values joined.
fn header_each(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let block =
        current_block(mrb)?.ok_or_else(|| argument_error(mrb, "each_header needs a block"))?;
    let header = RHash::try_convert(ivar(mrb, self_, c"@header")?, mrb)?;
    let mut pairs = Vec::new();
    header.foreach(mrb, |name: Value, values: RArray| {
        pairs.push((
            name,
            values.funcall::<_, _, Value>(mrb, c"join", [mrb.str_new(b", ").as_value()])?,
        ));
        Ok(ForEach::Continue)
    })?;
    for (name, value) in pairs {
        block.call::<_, Value>(mrb, [name, value])?;
    }
    Ok(header.as_value())
}

/// `to_hash`: each header's name to its values.
fn header_to_hash(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let header = RHash::try_convert(ivar(mrb, self_, c"@header")?, mrb)?;
    Ok(header.dup(mrb).as_value())
}

/// The header hash and the lowercase key `name` stands for in it.
fn header_key(mrb: &Mrb, self_: Value, name: Value) -> Result<(RHash, Value), Error> {
    let header = RHash::try_convert(ivar(mrb, self_, c"@header")?, mrb)?;
    let key = mrb.str_new(to_text(mrb, name)?.to_ascii_lowercase().as_bytes());
    Ok((header, key.as_value()))
}

/// `key?(name)`: whether the header is present, case-insensitively.
fn header_key_p(mrb: &Mrb, self_: Value, name: Value) -> Result<bool, Error> {
    let (header, key) = header_key(mrb, self_, name)?;
    Ok(!header.aref::<_, Value>(mrb, key)?.is_nil())
}

/// `delete(name)`: remove the header, answering its values, or `nil`.
fn header_delete(mrb: &Mrb, self_: Value, name: Value) -> Result<Value, Error> {
    let (header, key) = header_key(mrb, self_, name)?;
    header.delete(mrb, key)
}

/// `content_type`: the media type without its parameters, or `nil`.
fn header_content_type(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let value = header_aref(mrb, self_, mrb.str_new(b"content-type").as_value())?;
    if value.is_nil() {
        return Ok(value);
    }
    let value = to_text(mrb, value)?;
    let media = value.split(';').next().unwrap_or_default().trim();
    Ok(media.into_value(mrb))
}

/// `content_type = type`: set the `content-type` header.
fn request_set_content_type(mrb: &Mrb, self_: Value, media: Value) -> Result<Value, Error> {
    request_aset(mrb, self_, mrb.str_new(b"content-type").as_value(), media)
}

/// `basic_auth(user, password)`: an `authorization` header of the Basic
/// scheme, as CRuby writes it.
fn request_basic_auth(
    mrb: &Mrb,
    self_: Value,
    user: Value,
    password: Value,
) -> Result<Value, Error> {
    let mut credentials = to_bytes(mrb, user)?;
    credentials.push(b':');
    credentials.extend(to_bytes(mrb, password)?);
    let value = format!("Basic {}", base64(&credentials));
    request_aset(
        mrb,
        self_,
        mrb.str_new(b"authorization").as_value(),
        value.into_value(mrb),
    )
}

/// `set_form_data(params, sep = "&")` and `form_data = params`: the params as
/// `application/x-www-form-urlencoded`, encoded as CRuby's
/// `URI.encode_www_form` encodes them.
fn request_set_form_data(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (params, separator) = match args {
        [params] => (*params, "&".to_string()),
        [params, separator] => (*params, to_text(mrb, *separator)?),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let params = RHash::try_convert(params, mrb)?;
    let mut pairs = Vec::new();
    params.foreach(mrb, |name: Value, value: Value| {
        pairs.push(format!(
            "{}={}",
            www_form_escape(&to_bytes(mrb, name)?),
            www_form_escape(&to_bytes(mrb, value)?)
        ));
        Ok(ForEach::Continue)
    })?;
    let body = pairs.join(&separator);
    request_set_body(mrb, self_, body.into_value(mrb))?;
    request_set_content_type(
        mrb,
        self_,
        "application/x-www-form-urlencoded".into_value(mrb),
    )?;
    Ok(params.as_value())
}

/// One form component as `URI.encode_www_form_component` writes it: `*-._`
/// and alphanumerics kept, a space as `+`, every other byte as `%XX`.
fn www_form_escape(bytes: &[u8]) -> String {
    let mut escaped = String::with_capacity(bytes.len());
    for &byte in bytes {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                escaped.push(char::from(byte))
            }
            b' ' => escaped.push('+'),
            _ => escaped.push_str(&format!("%{byte:02X}")),
        }
    }
    escaped
}

/// `bytes` in standard Base64 with padding, as `pack("m0")` writes it.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let triple = group
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= group.len() {
                encoded.push(char::from(ALPHABET[(triple >> (18 - 6 * i) & 63) as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}
