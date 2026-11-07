//! What the Wasm Driver answers each preview2 import. A guest holds no socket
//! until the network is granted, so a call needing one answers as WASI says a
//! host without it does; the clock subscriptions and `poll` wait on the
//! runtime's wait primitive.

use std::time::{Duration, Instant};

use kobako_runtime::wait::do_poll;

use super::bindings::wasi_clocks_monotonic_clock_0_2_12 as monotonic_clock;
use super::bindings::wasi_io_poll_0_2_12::Pollable;
use super::bindings::wasi_sockets_network_0_2_12::{ErrorCode, IpAddressFamily, Network};
use super::bindings::wasi_sockets_tcp_0_2_12::TcpSocket;
use super::bindings::Host;
use super::table::{self, Resource};
use crate::invocation::Invocation;
use crate::trap::TimeoutTrap;

impl Invocation {
    /// A pollable on the guest's own clocks: ready at once on frozen ones,
    /// and `nanos` of host time from now on live ones.
    fn subscribe_clock(&mut self, nanos: u64) -> wasmtime::Result<Pollable> {
        let at = match self.wasi().clocks().advances() {
            true => Some(Instant::now() + Duration::from_nanos(nanos)),
            false => None,
        };
        let pollable = table::Pollable::Clock(at);
        Ok(Pollable(
            self.wasi_mut()
                .table_mut()
                .push(Resource::Pollable(pollable))?,
        ))
    }

    /// Wait until `ready` answers, or trap as the deadline or an interrupt
    /// ends the run; the wait sleeps until the soonest pollable is due.
    fn wait_for<T>(
        &self,
        due: Option<Instant>,
        mut ready: impl FnMut() -> Option<T>,
    ) -> wasmtime::Result<T> {
        if let Some(answer) = ready() {
            return Ok(answer);
        }
        loop {
            let timeout = due.map(|at| at.saturating_duration_since(Instant::now()));
            match do_poll(timeout, |_| ready()) {
                Ok(Some(answer)) => return Ok(answer),
                Ok(None) => {
                    if let Some(answer) = ready() {
                        return Ok(answer);
                    }
                }
                Err(_) if self.deadline().is_some_and(|at| Instant::now() >= at) => {
                    return Err(wasmtime::Error::new(TimeoutTrap));
                }
                Err(_) => return Err(wasmtime::Error::msg("the guest's wait was interrupted")),
            }
        }
    }
}

impl Host for Invocation {
    fn instance_network_instance_network(&mut self) -> wasmtime::Result<Network> {
        Ok(Network(
            self.wasi_mut().table_mut().push(Resource::Network)?,
        ))
    }

    fn network_network_drop(&mut self, handle: Network) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }

    /// No network is granted, so no socket is made, as WASI's
    /// `not-supported` says of a host without one.
    fn tcp_create_socket_create_tcp_socket(
        &mut self,
        _address_family: IpAddressFamily,
    ) -> wasmtime::Result<Result<TcpSocket, ErrorCode>> {
        Ok(Err(ErrorCode::NotSupported))
    }

    fn monotonic_clock_subscribe_duration(
        &mut self,
        when: monotonic_clock::Duration,
    ) -> wasmtime::Result<Pollable> {
        self.subscribe_clock(when)
    }

    /// An instant counts from preview1's `clock_time_get` origin, so both
    /// encodings name one clock.
    fn monotonic_clock_subscribe_instant(
        &mut self,
        when: monotonic_clock::Instant,
    ) -> wasmtime::Result<Pollable> {
        let now = self.wasi().clocks().monotonic_now();
        self.subscribe_clock(when.saturating_sub(now))
    }

    fn poll_pollable_ready(&mut self, this: Pollable) -> wasmtime::Result<bool> {
        Ok(self.wasi().table().pollable(this.0)?.ready())
    }

    fn poll_pollable_block(&mut self, this: Pollable) -> wasmtime::Result<()> {
        let pollable = self.wasi().table().pollable(this.0)?;
        self.wait_for(pollable.due(), || pollable.ready().then_some(()))
    }

    /// The indices of the pollables ready, once one is, as WASI's `poll`
    /// answers; an empty list traps, as WASI says it does.
    fn poll_poll(&mut self, r#in: Vec<Pollable>) -> wasmtime::Result<Vec<u32>> {
        if r#in.is_empty() {
            return Err(wasmtime::Error::msg("poll was handed no pollable"));
        }
        let pollables = r#in
            .iter()
            .map(|handle| self.wasi().table().pollable(handle.0))
            .collect::<wasmtime::Result<Vec<_>>>()?;
        let due = pollables.iter().filter_map(table::Pollable::due).min();
        self.wait_for(due, || {
            let ready = (0..pollables.len() as u32)
                .filter(|i| pollables[*i as usize].ready())
                .collect::<Vec<_>>();
            (!ready.is_empty()).then_some(ready)
        })
    }

    fn poll_pollable_drop(&mut self, handle: Pollable) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }
}

#[cfg(test)]
mod tests {
    //! Witness each preview2 import through a core module that links it by its
    //! WASI name, as a Guest Binary does.
    use std::time::{Duration, Instant};

    use wasmtime::{Instance, Module, Store};

    use crate::cache::load_engine;
    use crate::config::Config;
    use crate::imports;
    use crate::invocation::Invocation;
    use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
    use crate::wasi::Wasi;
    use kobako_runtime::profile::Profile;
    use kobako_runtime::sched::run_task;

    /// Imports one function of each preview2 interface kobako answers, with a
    /// bump `cabi_realloc` from 4096.
    const GUEST_WAT: &str = r#"
        (module
          (import "wasi:sockets/tcp-create-socket@0.2.12" "create-tcp-socket"
            (func $create (param i32 i32)))
          (import "wasi:sockets/instance-network@0.2.12" "instance-network"
            (func $network (result i32)))
          (import "wasi:sockets/network@0.2.12" "[resource-drop]network"
            (func $drop_network (param i32)))
          (import "wasi:clocks/monotonic-clock@0.2.12" "subscribe-duration"
            (func $subscribe (param i64) (result i32)))
          (import "wasi:io/poll@0.2.12" "[method]pollable.block" (func $block (param i32)))
          (import "wasi:io/poll@0.2.12" "poll" (func $poll (param i32 i32 i32)))
          (memory (export "memory") 1)
          (global $bump (mut i32) (i32.const 4096))
          (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
            (global.get $bump)
            (global.set $bump (i32.add (global.get $bump) (local.get 3))))
          (func (export "create_ipv4") (call $create (i32.const 0) (i32.const 16)))
          (func (export "network") (result i32) (call $network))
          (func (export "drop_network") (param i32) (call $drop_network (local.get 0)))
          (func (export "subscribe") (param i64) (result i32) (call $subscribe (local.get 0)))
          (func (export "block") (param i32) (call $block (local.get 0)))
          (func (export "poll") (param i32 i32)
            (i32.store (i32.const 64) (local.get 0))
            (call $poll (i32.const 64) (local.get 1) (i32.const 32))))
    "#;

    /// A guest at `profile`, holding at most `open_file_limit` files.
    fn guest(profile: Profile, open_file_limit: Option<usize>) -> (Store<Invocation>, Instance) {
        let engine = load_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit,
            profile,
            security: Vec::new(),
        };
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[]).expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(None, wasi));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let module = Module::new(engine, GUEST_WAT).expect("the guest module must compile");
        let instance = imports::linker()
            .expect("host imports must link")
            .instantiate(&mut store, &module)
            .expect("every preview2 import the module names must link");
        (store, instance)
    }

    fn call<P: wasmtime::WasmParams, R: wasmtime::WasmResults>(
        store: &mut Store<Invocation>,
        instance: &Instance,
        name: &str,
        params: P,
    ) -> wasmtime::Result<R> {
        instance
            .get_typed_func::<P, R>(&mut *store, name)
            .expect("the guest module exports each call")
            .call(&mut *store, params)
    }

    fn read(store: &mut Store<Invocation>, instance: &Instance, at: usize, len: usize) -> Vec<u8> {
        let memory = instance.get_memory(&mut *store, "memory").expect("memory");
        memory.data(&*store)[at..at + len].to_vec()
    }

    // @behavior NW-009
    #[test]
    fn without_the_network_creating_a_socket_answers_not_supported() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        call::<(), ()>(&mut store, &instance, "create_ipv4", ()).expect("the call answers");
        let answer = read(&mut store, &instance, 16, 5);
        assert_eq!(
            (answer[0], answer[4]),
            (1, 2),
            "create-tcp-socket with no network entry must answer err(not-supported)"
        );
    }

    // @behavior NW-010
    #[test]
    fn the_network_is_one_handle_the_guest_may_drop() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let network = call::<(), i32>(&mut store, &instance, "network", ()).expect("a handle");
        let first = call::<i32, ()>(&mut store, &instance, "drop_network", network);
        let second = call::<i32, ()>(&mut store, &instance, "drop_network", network);
        assert!(
            first.is_ok() && second.is_err(),
            "[resource-drop]network of the handle instance-network answered must succeed once and \
             then trap"
        );
    }

    // @behavior RT-338
    #[test]
    fn a_clock_wait_on_the_strongest_posture_ends_at_once() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let started = Instant::now();
        let pollable = call::<i64, i32>(&mut store, &instance, "subscribe", 10_000_000_000)
            .expect("a pollable");
        call::<i32, ()>(&mut store, &instance, "block", pollable).expect("the wait answers");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "pollable.block on ten seconds of subscribe-duration at the hermetic posture must \
             return at once, not after {:?}",
            started.elapsed()
        );
    }

    // @behavior RT-339
    #[test]
    fn a_poll_on_a_clock_waits_until_it_is_due() {
        let (mut store, instance) = guest(Profile::Permissive, None);
        let started = Instant::now();
        let pollable =
            call::<i64, i32>(&mut store, &instance, "subscribe", 50_000_000).expect("a pollable");
        run_task(Some(started + Duration::from_secs(10)), || {
            call::<(i32, i32), ()>(&mut store, &instance, "poll", (pollable, 1))
        })
        .expect("the poll answers");
        let spent = started.elapsed();
        let answer = read(&mut store, &instance, 32, 8);
        let (at, len) = (
            u32::from_le_bytes(answer[..4].try_into().unwrap()) as usize,
            u32::from_le_bytes(answer[4..].try_into().unwrap()),
        );
        let index = u32::from_le_bytes(read(&mut store, &instance, at, 4).try_into().unwrap());
        assert!(
            (len, index) == (1, 0)
                && spent >= Duration::from_millis(50)
                && spent < Duration::from_secs(5),
            "poll of one pollable on fifty milliseconds of subscribe-duration at the permissive \
             posture must answer [0] once they pass, got {len} index {index} after {spent:?}"
        );
    }

    // @behavior RT-340
    #[test]
    fn a_poll_of_no_pollable_traps() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        assert!(
            call::<(i32, i32), ()>(&mut store, &instance, "poll", (0, 0)).is_err(),
            "poll of an empty list must trap"
        );
    }

    // @behavior RT-341
    #[test]
    fn a_resource_counts_against_the_open_file_limit() {
        let (mut store, instance) = guest(Profile::Hermetic, Some(3));
        assert!(
            call::<i64, i32>(&mut store, &instance, "subscribe", 0).is_err(),
            "subscribe-duration with the open file limit filled by the standard streams must trap"
        );
    }
}
