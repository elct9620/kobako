//! `random_get`, answered a page at a time into the guest's own buffer, as
//! Linux's `get_random_bytes_user` copies a page at a time: the buffer is
//! checked before any entropy is drawn, the host holds one page, and a run
//! past its deadline or interrupted stops between pages, as `getrandom`
//! looks for a pending signal at each page boundary.

use wasmtime_wasi::p2::bindings::random::random::Host as _;
use wiggle::GuestPtr;

use super::errno::Answer;
use super::memory::GuestMemory;
use super::types;
use crate::trap;
use crate::wasi::Wasi;

/// What one step draws and copies, Linux's `PAGE_SIZE`.
const PAGE: usize = 4096;

/// Fill the `len` bytes at `buf` with the entropy `wasi`'s profile reads.
pub(super) fn get(
    memory: &mut GuestMemory<'_, '_>,
    wasi: &mut Wasi,
    buf: GuestPtr<u8>,
    len: u32,
) -> Answer {
    let bytes = memory.bytes_mut(buf.as_array(len))?;
    let random = wasmtime_wasi::WasiView::ctx(wasi.ctx_mut()).ctx.random();
    for (index, page) in bytes.chunks_mut(PAGE).enumerate() {
        if index > 0 {
            if let Some(fatal) = trap::fatal_signal_trap() {
                return Err(types::Error::trap(fatal));
            }
        }
        let drawn = random
            .get_random_bytes(page.len() as u64)
            .map_err(types::Error::trap)?;
        page.copy_from_slice(&drawn);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use wasmtime::{Module, Store};

    use crate::cache::load_engine;
    use crate::config::Config;
    use crate::imports;
    use crate::invocation::Invocation;
    use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
    use crate::wasi::Wasi;
    use kobako_runtime::profile::Profile;
    use kobako_runtime::sched::run_task;

    /// Two pages of entropy asked for at `at` of a one-page memory.
    const READ_WAT: &str = r#"
        (module
          (import "wasi_snapshot_preview1" "random_get"
            (func $random (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "read_two_pages") (param $at i32) (result i32)
            (call $random (local.get $at) (i32.const 8192))))
    "#;

    /// Ask a permissive guest for two pages of entropy at `at`, its run's
    /// deadline already passed when `past_deadline`, answering how the call
    /// ended and what its memory then holds.
    fn read_two_pages(at: i32, past_deadline: bool) -> (wasmtime::Result<i32>, Vec<u8>) {
        let engine = load_engine().expect("shared engine must be constructible");
        let config = Config::for_profile(Profile::Permissive);
        let wasi = Wasi::for_invocation(&config, &[]).expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(wasi));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let module = Module::new(engine, READ_WAT).expect("read module must compile");
        let instance = imports::linker()
            .expect("host imports must link")
            .instantiate(&mut store, &module)
            .expect("read module must instantiate");
        let read = instance
            .get_typed_func::<i32, i32>(&mut store, "read_two_pages")
            .expect("read export must resolve");
        let deadline = past_deadline.then(Instant::now);
        store.data_mut().set_deadline(deadline);
        let ended = run_task(deadline, || read.call(&mut store, at));
        let memory = instance
            .get_memory(&mut store, "memory")
            .expect("the module exports its memory");
        (ended, memory.data(&store).to_vec())
    }

    // The first page lies inside the memory and the second crosses its end,
    // so a buffer checked a page at a time would draw into the first.
    // @behavior RT-416
    #[test]
    fn entropy_asked_for_past_the_memory_traps_before_any_is_drawn() {
        let at = 65536 - 4096 - 16;
        let (ended, memory) = read_two_pages(at, false);
        assert!(
            ended.is_err() && memory[at as usize..].iter().all(|&byte| byte == 0),
            "random_get into a buffer that ends past a permissive guest's memory must trap \
             before drawing any entropy into the part inside it; got {:?}",
            ended.map_err(|trap| trap.to_string())
        );
    }

    // @behavior RT-417
    #[test]
    fn a_run_reading_entropy_stops_at_its_deadline_between_pages() {
        let (ended, memory) = read_two_pages(0, true);
        let (first, second) = memory[..8192].split_at(4096);
        assert!(
            ended
                .as_ref()
                .err()
                .is_some_and(|trap| trap.is::<crate::trap::TimeoutTrap>())
                && first.iter().any(|&byte| byte != 0)
                && second.iter().all(|&byte| byte == 0),
            "random_get of two pages by a permissive guest whose run's deadline passed must \
             draw the first page and end the run as timed out before the second; got {:?}",
            ended.map_err(|trap| trap.to_string())
        );
    }
}
