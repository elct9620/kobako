//! Caller-based guest linear-memory I/O shared by the host-import paths.
//!
//! Both directions of a host↔guest buffer handoff that run *inside* a wasm
//! callback frame go through here: writing the transport Response back
//! (`super::dispatch`) and shipping block-yield args into the guest
//! (`super::instance::drive_yield`) performed the same `__kobako_alloc` +
//! bounds-check + `memory.write` dance with only the diagnostic strings
//! differing. The Store-based write path (`Runtime::write_envelope`) is a
//! separate beast — it holds the cached `Store`, not a `Caller` — and stays
//! in `instance.rs`.

use wasmtime::{Caller, Extern};

use super::invocation::Invocation;

/// Allocate `bytes.len()` bytes in guest memory via `__kobako_alloc` and
/// copy `bytes` in. Returns the guest pointer. Every failure path carries a
/// `&'static str` reason so the caller can surface a diagnostic rather than
/// a silent fault.
pub(super) fn alloc_and_write(
    caller: &mut Caller<'_, Invocation>,
    bytes: &[u8],
) -> Result<u32, &'static str> {
    let alloc = match caller.get_export("__kobako_alloc") {
        Some(Extern::Func(f)) => f
            .typed::<i32, i32>(&*caller)
            .map_err(|_| "Sandbox runtime's allocation hook has the wrong signature")?,
        _ => return Err("Sandbox runtime is missing the allocation hook"),
    };
    let len = i32::try_from(bytes.len()).map_err(|_| "guest buffer exceeds 2 GiB")?;
    let ptr = alloc
        .call(&mut *caller, len)
        .map_err(|_| "Sandbox allocation trapped while preparing a guest buffer")?;
    if ptr == 0 {
        return Err("Sandbox is out of memory while preparing a guest buffer");
    }

    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return Err("Sandbox runtime does not export linear memory"),
    };
    mem.write(&mut *caller, ptr as usize, bytes)
        .map_err(|_| "could not write into Sandbox memory (range invalid)")?;
    Ok(ptr as u32)
}

/// Copy `[ptr, ptr + len)` out of the guest's linear memory as seen from
/// `caller`. Returns `None` when `memory` is not exported or the slice
/// falls outside the live memory range.
pub(super) fn read(caller: &mut Caller<'_, Invocation>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return None,
    };
    let data = mem.data(&caller);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    data.get(start..end).map(|s| s.to_vec())
}
