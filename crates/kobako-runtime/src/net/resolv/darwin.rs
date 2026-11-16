//! macOS answers every lookup through mDNSResponder, which its own
//! `getaddrinfo` asks too: `DNSServiceGetAddrInfo` hands back a socket the
//! reactor waits on, and `DNSServiceRefDeallocate` cancels the query. One
//! query asks for IPv6 and one for IPv4, so each knows when it is answered,
//! and the answer lists IPv6 first, as RFC 6724's default policy table ranks
//! it above IPv4 and the system's `getaddrinfo` sorts.

use std::ffi::{c_char, c_int, c_void, CString};
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::RawFd;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

use mio::unix::SourceFd;

use super::ReadyWaker;
use crate::net::lookup::Resolving;
use crate::net::reactor::{Reactor, Ready, Registration};

/// `DNSServiceRef`, an opaque handle to one query.
type ServiceRef = *mut c_void;

/// `DNSServiceGetAddrInfoReply`.
type Reply = extern "C" fn(
    ServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const libc::sockaddr,
    u32,
    *mut c_void,
);

// The values `dns_sd.h` gives each name.
const K_DNS_SERVICE_FLAGS_MORE_COMING: u32 = 0x1;
const K_DNS_SERVICE_FLAGS_ADD: u32 = 0x2;
const K_DNS_SERVICE_FLAGS_RETURN_INTERMEDIATES: u32 = 0x1000;
const K_DNS_SERVICE_FLAGS_TIMEOUT: u32 = 0x10000;
const K_DNS_SERVICE_PROTOCOL_IPV4: u32 = 0x1;
const K_DNS_SERVICE_PROTOCOL_IPV6: u32 = 0x2;
const K_DNS_SERVICE_ERR_NO_ERROR: i32 = 0;
const K_DNS_SERVICE_ERR_NO_SUCH_NAME: i32 = -65538;
const K_DNS_SERVICE_ERR_NO_SUCH_RECORD: i32 = -65554;

extern "C" {
    fn DNSServiceGetAddrInfo(
        sd_ref: *mut ServiceRef,
        flags: u32,
        interface_index: u32,
        protocol: u32,
        hostname: *const c_char,
        callback: Reply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(sd_ref: ServiceRef) -> c_int;
    fn DNSServiceProcessResult(sd_ref: ServiceRef) -> i32;
    fn DNSServiceRefDeallocate(sd_ref: ServiceRef);
}

pub(super) fn resolve(name: &str) -> Resolving {
    match Lookup::start(name) {
        Ok(lookup) => Box::pin(lookup),
        Err(err) => Box::pin(std::future::ready(Err(err))),
    }
}

/// What one query's replies brought: its addresses, and whether it is answered.
#[derive(Default)]
struct Replies {
    addresses: Vec<IpAddr>,
    answered: bool,
    failed: bool,
}

/// One family's query, cancelled as it drops.
struct Query {
    sd_ref: ServiceRef,
    fd: RawFd,
    registration: Registration,
    /// Where `on_reply` writes, owned by the query and freed after `sd_ref`
    /// closes, so no reply lands in freed memory.
    replies: NonNull<Replies>,
}

// The handle is used only by the task that owns the query, one thread at a
// time, as the reactor runs a task.
unsafe impl Send for Query {}

impl Query {
    fn start(
        name: &CString,
        protocol: u32,
        reactor: &Arc<Reactor>,
        waker: Weak<dyn Ready>,
    ) -> io::Result<Self> {
        let replies = NonNull::from(Box::leak(Box::<Replies>::default()));
        let mut sd_ref: ServiceRef = std::ptr::null_mut();
        let flags = K_DNS_SERVICE_FLAGS_RETURN_INTERMEDIATES | K_DNS_SERVICE_FLAGS_TIMEOUT;
        let context = replies.as_ptr().cast::<c_void>();
        let err = unsafe {
            DNSServiceGetAddrInfo(
                &mut sd_ref,
                flags,
                0,
                protocol,
                name.as_ptr(),
                on_reply,
                context,
            )
        };
        if err != K_DNS_SERVICE_ERR_NO_ERROR {
            return Err(io::Error::other(format!(
                "DNSServiceGetAddrInfo failed: {err}"
            )));
        }
        let fd = unsafe { DNSServiceRefSockFD(sd_ref) };
        match reactor.register(&mut SourceFd(&fd), waker) {
            Ok(registration) => Ok(Self {
                sd_ref,
                fd,
                registration,
                replies,
            }),
            Err(err) => {
                unsafe { DNSServiceRefDeallocate(sd_ref) };
                drop(unsafe { Box::from_raw(replies.as_ptr()) });
                Err(err)
            }
        }
    }

    /// What the query's replies brought so far.
    fn replies(&self) -> &Replies {
        unsafe { self.replies.as_ref() }
    }

    /// Hand each reply waiting on the socket to `on_reply`, never blocking.
    fn take_replies(&mut self) {
        while !self.replies().answered && is_readable(self.fd) {
            if unsafe { DNSServiceProcessResult(self.sd_ref) } != K_DNS_SERVICE_ERR_NO_ERROR {
                let replies = unsafe { self.replies.as_mut() };
                replies.failed = true;
                replies.answered = true;
            }
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        // The socket leaves the reactor before the query closes it.
        self.registration.deregister(&mut SourceFd(&self.fd));
        unsafe { DNSServiceRefDeallocate(self.sd_ref) };
        drop(unsafe { Box::from_raw(self.replies.as_ptr()) });
    }
}

/// Whether `fd` has a reply to read, or has closed, without waiting.
fn is_readable(fd: RawFd) -> bool {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut pollfd, 1, 0) > 0 }
}

extern "C" fn on_reply(
    _sd_ref: ServiceRef,
    flags: u32,
    _interface_index: u32,
    error: i32,
    _hostname: *const c_char,
    address: *const libc::sockaddr,
    _ttl: u32,
    context: *mut c_void,
) {
    let replies = unsafe { &mut *context.cast::<Replies>() };
    match error {
        K_DNS_SERVICE_ERR_NO_ERROR if flags & K_DNS_SERVICE_FLAGS_ADD != 0 => {
            if let Some(ip) = unsafe { ip_of(address) } {
                if !replies.addresses.contains(&ip) {
                    replies.addresses.push(ip);
                }
            }
        }
        K_DNS_SERVICE_ERR_NO_ERROR
        | K_DNS_SERVICE_ERR_NO_SUCH_NAME
        | K_DNS_SERVICE_ERR_NO_SUCH_RECORD => {}
        _ => replies.failed = true,
    }
    if flags & K_DNS_SERVICE_FLAGS_MORE_COMING == 0 {
        replies.answered = true;
    }
}

/// The address a reply carries.
unsafe fn ip_of(address: *const libc::sockaddr) -> Option<IpAddr> {
    if address.is_null() {
        return None;
    }
    match c_int::from((*address).sa_family) {
        libc::AF_INET => {
            let addr = &*address.cast::<libc::sockaddr_in>();
            Some(Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr)).into())
        }
        libc::AF_INET6 => {
            let addr = &*address.cast::<libc::sockaddr_in6>();
            Some(Ipv6Addr::from(addr.sin6_addr.s6_addr).into())
        }
        _ => None,
    }
}

/// An IPv6 and an IPv4 query for one name, answered once both are.
struct Lookup {
    queries: [Query; 2],
    waker: Arc<ReadyWaker>,
}

impl Lookup {
    fn start(name: &str) -> io::Result<Self> {
        let name = CString::new(name).map_err(|_| io::ErrorKind::InvalidInput)?;
        let reactor = Reactor::current()?;
        let waker = Arc::new(ReadyWaker::default());
        let weak = Arc::downgrade(&waker) as Weak<dyn Ready>;
        let ipv6 = Query::start(&name, K_DNS_SERVICE_PROTOCOL_IPV6, &reactor, weak.clone())?;
        let ipv4 = Query::start(&name, K_DNS_SERVICE_PROTOCOL_IPV4, &reactor, weak)?;
        Ok(Self {
            queries: [ipv6, ipv4],
            waker,
        })
    }
}

impl Future for Lookup {
    type Output = io::Result<Vec<IpAddr>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // The waker is in place before the sockets are read, so a reply
        // landing after the read wakes the task; one landing before it is
        // read again at once.
        loop {
            let changed = self.waker.poll_changed(cx);
            self.queries.iter_mut().for_each(Query::take_replies);
            if self.queries.iter().all(|query| query.replies().answered) {
                break;
            }
            if changed.is_pending() {
                return Poll::Pending;
            }
        }
        let addresses: Vec<IpAddr> = self
            .queries
            .iter()
            .flat_map(|query| query.replies().addresses.iter().copied())
            .collect();
        match addresses.is_empty() && self.queries.iter().any(|query| query.replies().failed) {
            true => Poll::Ready(Err(io::Error::other("the system resolver failed"))),
            false => Poll::Ready(Ok(addresses)),
        }
    }
}
