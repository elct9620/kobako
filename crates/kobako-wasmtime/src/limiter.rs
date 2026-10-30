//! Hands wasmtime's growth hook to the runtime's per-invocation memory cap.

use kobako_runtime::endpoint::MemoryCap;
use wasmtime::ResourceLimiter;

/// wasmtime asks this as `memory.grow` runs; an `Err` becomes the trap
/// surfaced to the host as a guest invocation failure.
pub(crate) struct MemoryLimiter(pub(crate) MemoryCap);

impl ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.0.growing(desired).map_err(wasmtime::Error::new)?;
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(true)
    }
}
