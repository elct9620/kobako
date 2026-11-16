//! Windows answers a lookup through `GetAddrInfoExW` with a completion
//! routine, the asynchronous form its own `getaddrinfo` stands on, and
//! `GetAddrInfoExCancel` cancels it. The routine runs on a system thread even
//! after a cancel, so it holds its own reference to what it writes.

use std::cell::{Cell, UnsafeCell};
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Networking::WinSock::{
    FreeAddrInfoExW, GetAddrInfoExCancel, GetAddrInfoExW, WSAStartup, ADDRINFOEXW, AF_INET,
    AF_INET6, AF_UNSPEC, IPPROTO_TCP, NS_ALL, SOCKADDR_IN, SOCKADDR_IN6, SOCK_STREAM, WSADATA,
    WSAEINVAL, WSAHOST_NOT_FOUND, WSANO_DATA, WSANO_RECOVERY, WSA_IO_PENDING,
};
use windows_sys::Win32::System::IO::OVERLAPPED;

use crate::net::lookup::{LookupError, Resolving};

/// `name`'s addresses; Winsock that cannot start is `Again`.
pub(super) fn resolve(name: &str) -> Resolving {
    match Query::start(name) {
        Ok(query) => Box::pin(query),
        Err(_) => Box::pin(std::future::ready(Err(LookupError::Again))),
    }
}

/// The kind of `error`, a Winsock code, as Windows' `getaddrinfo` pairs
/// `WSAHOST_NOT_FOUND` with `EAI_NONAME`, `WSANO_RECOVERY` with `EAI_FAIL`,
/// and `WSATRY_AGAIN` with `EAI_AGAIN`. A name no query can carry is
/// `NoName`; any other failure leaves a lookup asking again may answer.
fn to_lookup_error(error: i32) -> LookupError {
    match error {
        WSAHOST_NOT_FOUND | WSANO_DATA | WSAEINVAL => LookupError::NoName,
        WSANO_RECOVERY => LookupError::Fail,
        _ => LookupError::Again,
    }
}

/// What the call and its completion routine share, as Microsoft's example
/// calls its `QUERY_CONTEXT`. `overlapped` comes first, so the routine finds
/// the whole from the pointer it is handed; Windows writes it, `result`, and
/// `handle` through that pointer, so each sits in an `UnsafeCell`.
#[repr(C)]
struct QueryContext {
    overlapped: UnsafeCell<OVERLAPPED>,
    result: UnsafeCell<*mut ADDRINFOEXW>,
    handle: UnsafeCell<HANDLE>,
    /// Whether the query is pending, being cancelled, or done. A cancel and
    /// the routine's end exclude each other, so no cancel follows a routine
    /// that marked the query done; one racing Windows' own completion,
    /// before the routine marks it, answers `WSA_INVALID_HANDLE` as
    /// `GetAddrInfoExCancel` documents.
    phase: AtomicU8,
    state: Mutex<State>,
}

// Windows writes the cells before the routine runs, and the routine alone
// reads them after; everything else is atomic or behind the mutex.
unsafe impl Send for QueryContext {}
unsafe impl Sync for QueryContext {}

const PENDING: u8 = 0;
const CANCELLING: u8 = 1;
const DONE: u8 = 2;

thread_local! {
    /// Whether this thread is inside `GetAddrInfoExCancel`, which may run the
    /// routine on it before returning.
    static IN_CANCEL: Cell<bool> = const { Cell::new(false) };
}

#[derive(Default)]
struct State {
    answer: Option<Result<Vec<IpAddr>, LookupError>>,
    waker: Option<Waker>,
}

impl QueryContext {
    fn new() -> Self {
        Self {
            overlapped: UnsafeCell::new(unsafe { std::mem::zeroed() }),
            result: UnsafeCell::new(null_mut()),
            handle: UnsafeCell::new(null_mut()),
            phase: AtomicU8::new(PENDING),
            state: Mutex::default(),
        }
    }

    /// Mark the query done, waiting out a cancel another thread is making,
    /// so the handle stays valid for as long as that cancel names it.
    fn end(&self) {
        loop {
            match self
                .phase
                .compare_exchange(PENDING, DONE, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) | Err(DONE) => return,
                Err(_) if IN_CANCEL.get() => return,
                Err(_) => std::thread::yield_now(),
            }
        }
    }

    /// Cancel the query unless it already ended.
    fn cancel(&self) {
        let claimed = self
            .phase
            .compare_exchange(PENDING, CANCELLING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if claimed {
            IN_CANCEL.set(true);
            unsafe { GetAddrInfoExCancel(self.handle.get()) };
            IN_CANCEL.set(false);
            self.phase.store(DONE, Ordering::Release);
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Take the query's outcome, `error` being its Winsock code, and wake
    /// whoever waits for it.
    fn finish(&self, error: u32) {
        self.end();
        let result = unsafe { std::mem::replace(&mut *self.result.get(), null_mut()) };
        let answer = match error {
            0 => Ok(unsafe { addresses_of(result) }),
            code => Err(to_lookup_error(code as i32)),
        };
        if !result.is_null() {
            unsafe { FreeAddrInfoExW(result) };
        }
        let mut state = self.state();
        state.answer = Some(answer);
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }
}

/// Each address in `result`, in the order Windows sorted them.
unsafe fn addresses_of(mut result: *const ADDRINFOEXW) -> Vec<IpAddr> {
    let mut addresses = Vec::new();
    while let Some(info) = result.as_ref() {
        let ip = match i32::from(info.ai_family as u16) {
            family if family == i32::from(AF_INET) => {
                let addr = &*info.ai_addr.cast::<SOCKADDR_IN>();
                Some(IpAddr::from(Ipv4Addr::from(u32::from_be(
                    addr.sin_addr.S_un.S_addr,
                ))))
            }
            family if family == i32::from(AF_INET6) => {
                let addr = &*info.ai_addr.cast::<SOCKADDR_IN6>();
                Some(IpAddr::from(Ipv6Addr::from(addr.sin6_addr.u.Byte)))
            }
            _ => None,
        };
        addresses.extend(ip);
        result = info.ai_next;
    }
    addresses
}

unsafe extern "system" fn on_complete(error: u32, _bytes: u32, overlapped: *const OVERLAPPED) {
    let context = Arc::from_raw(overlapped.cast::<QueryContext>());
    context.finish(error);
}

/// Start Winsock once for the process, as a call before any socket needs.
fn start_winsock() -> io::Result<()> {
    static STARTED: OnceLock<i32> = OnceLock::new();
    let code = *STARTED.get_or_init(|| {
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        unsafe { WSAStartup(0x202, &mut data) }
    });
    match code {
        0 => Ok(()),
        code => Err(io::Error::from_raw_os_error(code)),
    }
}

/// One query, cancelled as it drops unanswered.
struct Query {
    context: Arc<QueryContext>,
}

impl Query {
    fn start(name: &str) -> io::Result<Self> {
        start_winsock()?;
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut hints: ADDRINFOEXW = unsafe { std::mem::zeroed() };
        hints.ai_family = i32::from(AF_UNSPEC);
        hints.ai_socktype = SOCK_STREAM;
        hints.ai_protocol = IPPROTO_TCP;
        let context = Arc::new(QueryContext::new());
        // The routine's own reference, which it takes back as it runs.
        let routine = Arc::into_raw(context.clone());
        let error = unsafe {
            GetAddrInfoExW(
                wide.as_ptr(),
                null(),
                NS_ALL,
                null(),
                &hints,
                context.result.get(),
                null(),
                routine.cast::<OVERLAPPED>(),
                Some(on_complete),
                context.handle.get(),
            )
        };
        if error != WSA_IO_PENDING {
            // Answered at once, so the routine never runs.
            drop(unsafe { Arc::from_raw(routine) });
            context.finish(error as u32);
        }
        Ok(Self { context })
    }
}

impl Future for Query {
    type Output = Result<Vec<IpAddr>, LookupError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.context.state();
        match state.answer.take() {
            Some(answer) => Poll::Ready(answer),
            None => {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        self.context.cancel();
    }
}
