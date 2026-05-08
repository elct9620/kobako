//! kobako-wasm — Guest Binary crate root.
//!
//! This crate is the source of `kobako.wasm`, the Guest Binary artifact
//! described in SPEC.md "Core Abstractions". It hosts:
//!
//! * `codec` — the hand-written MessagePack wire codec (SPEC.md "Wire Codec").
//! * (future) ABI exports `__kobako_run`, `__kobako_alloc`,
//!   `__kobako_take_outcome` — added by item #9.
//! * `boot` — Rust-side mruby C API registrations that REFERENCE
//!   Ch.5 §Boot Script 預載 specifies (Kobako module / Kobako::RPC
//!   class / Kobako.__rpc_call__ module function). No Ruby boot text.
//! * `mruby_sys` — hand-rolled FFI declarations for the mruby C API
//!   subset the boot mechanism calls.
//!
//! This is the **skeleton** delivered by item #4: module layout, error type,
//! and the `Value` enum covering the 11 wire types per SPEC.md "Type
//! Mapping". Encode/decode bodies are placeholders (`unimplemented!()`) and
//! will be filled in by item #6.
//!
//! The crate uses `std` on every target. `wasm32-wasip1` (the production
//! target — see SPEC.md "Implementation Standards" §Architecture) ships a
//! working `std`, including allocator and panic handler. A `no_std` codec
//! is not required by SPEC; switching adds friction (custom allocator,
//! custom panic handler) without buying anything for the Guest Binary,
//! which already pays for `std` through the embedded mruby interpreter.

pub mod abi;
pub mod boot;
pub mod codec;
pub mod envelope;
pub mod mruby_sys;
pub mod rpc_client;

pub use abi::{pack_u64, unpack_u64, EXPORT_NAMES, IMPORT_MODULE, IMPORT_NAME};
pub use boot::mrb_kobako_init;
pub use codec::{Decoder, Encoder, Value, WireError};
pub use envelope::{
    decode_outcome, decode_panic, decode_request, decode_response, decode_result, encode_outcome,
    encode_panic, encode_request, encode_response, encode_result, EnvelopeError, Outcome, Panic,
    Request, Response, ResultEnv, Target, STATUS_ERROR, STATUS_OK,
};
pub use rpc_client::{build_request_bytes, invoke_rpc, ExceptionPayload, InvokeError};
