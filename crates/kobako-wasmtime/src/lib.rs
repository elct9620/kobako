//! kobako-wasmtime — the wasmtime implementation of the kobako runtime
//! contract.
//!
//! `Driver` implements `kobako_runtime::runtime::Runtime` over wasmtime:
//! every invocation instantiates a fresh instance from a pre-linked
//! template and discards the whole Store afterwards — the
//! per-invocation instance discipline (ABI v2). Everything engine-bound
//! lives behind the contract surface, so a frontend shell (the Ruby
//! ext's `Kobako::Runtime`) sees no wasmtime type.
//!
//! Module layout (one responsibility per file):
//!
//! * `driver` — `Driver` + `impl kobako_runtime::runtime::Runtime`
//!   (the run mechanics).
//! * `cache` — process-wide Engine + per-path Module cache and the
//!   process-singleton epoch ticker thread.
//! * `config` — per-Driver caps (timeout / stdout / stderr limits).
//! * `exports` — per-invocation `__kobako_eval` / `_run` /
//!   `_take_outcome` / `_alloc` / `memory` handles.
//! * `instance_pre` — host-import Linker wiring + per-path
//!   `InstancePre` cache.
//! * `invocation` — Invocation (per-Store context), the
//!   `MemoryLimiter` memory cap, and the trap marker types
//!   (`TimeoutTrap` / `MemoryLimitTrap`).
//! * `dispatch` — `__kobako_dispatch` host-import dispatch helpers.
//! * `frames` — stdin frame stream + WASI context assembly, `#run`
//!   envelope write, OUTCOME_BUFFER readout.
//! * `guest_mem` — Caller-based guest linear-memory alloc / write / read.
//! * `capture` — stdout / stderr pipe sizing + clip helpers.
//! * `ambient` — frozen WASI clocks + constant RNG (ambient denial).
//! * `trap` — wasmtime-error → neutral `Trap` classification.

mod ambient;
mod cache;
mod capture;
mod config;
mod dispatch;
mod driver;
mod exports;
mod frames;
mod guest_mem;
mod instance_pre;
mod invocation;
mod trap;

pub use config::Config;
pub use driver::Driver;
