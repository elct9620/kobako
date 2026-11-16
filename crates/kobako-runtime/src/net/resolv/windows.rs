//! Windows answers a lookup through `GetAddrInfoExW` with a completion
//! routine, the asynchronous form its own `getaddrinfo` stands on, and
//! `GetAddrInfoExCancel` cancels it. The routine runs on a system thread even
//! after a cancel, so it holds its own reference to what it writes.

use std::cell::UnsafeCell;
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Networking::WinSock::{
    FreeAddrInfoExW, GetAddrInfoExCancel, GetAddrInfoExW, WSAStartup, ADDRINFOEXW, AF_INET,
    AF_INET6, AF_UNSPEC, IPPROTO_TCP, NS_ALL, SOCKADDR_IN, SOCKADDR_IN6, SOCK_STREAM, WSADATA,
    WSA_IO_PENDING,
};
use windows_sys::Win32::System::IO::OVERLAPPED;

use crate::net::lookup::Resolving;

pub(super) fn resolve(name: &str) -> Resolving {
    match Query::start(name) {
        Ok(query) => Box::pin(query),
        Err(err) => Box::pin(std::future::ready(Err(err))),
    }
}

/// What the call and its completion routine share. `overlapped` comes first,
/// so the routine finds the whole from the pointer it is handed.
#[repr(C)]
struct Shared {
    overlapped: OVERLAPPED,
    result: UnsafeCell<*mut ADDRINFOEXW>,
    handle: UnsafeCell<HANDLE>,
    state: Mutex<State>,
}

// Windows writes `result` and `handle` before the routine runs, and the
// routine alone reads them after; everything else sits behind the mutex.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

#[derive(Default)]
struct State {
    /// Whether the query ended, after which its handle is no longer valid.
    finished: bool,
    answer: Option<io::Result<Vec<IpAddr>>>,
    waker: Option<Waker>,
}

impl Shared {
    fn new() -> Self {
        Self {
            overlapped: unsafe { std::mem::zeroed() },
            result: UnsafeCell::new(null_mut()),
            handle: UnsafeCell::new(null_mut()),
            state: Mutex::default(),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Take the query's outcome, `error` being its Winsock code, and wake
    /// whoever waits for it.
    fn finish(&self, error: u32) {
        let result = unsafe { std::mem::replace(&mut *self.result.get(), null_mut()) };
        let answer = match error {
            0 => Ok(unsafe { addresses_of(result) }),
            code => Err(io::Error::from_raw_os_error(code as i32)),
        };
        if !result.is_null() {
            unsafe { FreeAddrInfoExW(result) };
        }
        let mut state = self.state();
        state.finished = true;
        state.answer = Some(answer);
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }
}

/// Each address in `result`, in the order Windows sorted them, once.
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
        if let Some(ip) = ip.filter(|ip| !addresses.contains(ip)) {
            addresses.push(ip);
        }
        result = info.ai_next;
    }
    addresses
}

unsafe extern "system" fn on_complete(error: u32, _bytes: u32, overlapped: *const OVERLAPPED) {
    let shared = Arc::from_raw(overlapped.cast::<Shared>());
    shared.finish(error);
}

/// Winsock started once for the process, as a call before any socket needs.
fn startup() -> io::Result<()> {
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
    shared: Arc<Shared>,
}

impl Query {
    fn start(name: &str) -> io::Result<Self> {
        startup()?;
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut hints: ADDRINFOEXW = unsafe { std::mem::zeroed() };
        hints.ai_family = i32::from(AF_UNSPEC);
        hints.ai_socktype = SOCK_STREAM;
        hints.ai_protocol = IPPROTO_TCP;
        let shared = Arc::new(Shared::new());
        // The routine's own reference, which it takes back as it runs.
        let routine = Arc::into_raw(shared.clone());
        let error = unsafe {
            GetAddrInfoExW(
                wide.as_ptr(),
                null(),
                NS_ALL,
                null(),
                &hints,
                shared.result.get(),
                null(),
                routine.cast::<OVERLAPPED>(),
                Some(on_complete),
                shared.handle.get(),
            )
        };
        if error != WSA_IO_PENDING {
            // Answered at once, so the routine never runs.
            drop(unsafe { Arc::from_raw(routine) });
            shared.finish(error as u32);
        }
        Ok(Self { shared })
    }
}

impl Future for Query {
    type Output = io::Result<Vec<IpAddr>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.shared.state();
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
        // The state is read apart from the cancel, since a cancel may run the
        // routine, which takes the state, before it returns.
        let finished = self.shared.state().finished;
        if !finished {
            unsafe { GetAddrInfoExCancel(self.shared.handle.get()) };
        }
    }
}
