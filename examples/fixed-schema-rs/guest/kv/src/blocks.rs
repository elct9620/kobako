//! The gem's block support.
//!
//! A block is not the built-in proxy's alone: a method this gem defines
//! takes one the same way, by handing it to the dispatch it makes. The
//! harness parks it for that call's duration, which is what the host's
//! yield finds — the yield re-enters the guest through a separate export
//! while the dispatch frame that took the block is still parked on the
//! wasm stack.
//!
//! What the yield carries is this schema's, like every other position —
//! one message per yield, read back into the arguments the block
//! receives. And the block's answer travels the outcome's way, so it is a
//! String or the invocation fails: one schema, one rule, wherever a value
//! leaves the guest.

use beni::{Error, Mrb, Value};
use kobako_mruby::Target;
use prost::Message;

use crate::dispatch::{call, type_error, wire_error, KV_PATH};
use crate::schema::{EachResponse, YieldKey};

/// `MyService::KV.each_key { |key| … }` — the host yields every stored
/// key to the block and answers with how many it yielded.
///
/// Any-arity because the block is read off the call frame rather than the
/// argument list.
pub(crate) fn each_key(mrb: &Mrb, _self: Value) -> Result<i32, Error> {
    let (_rest, block) = mrb.get_args::<beni::format::RestBlock>();
    if block.is_nil() {
        return Err(type_error(mrb, "MyService::KV.each_key needs a block"));
    }

    // The block is parked for the length of this call and no longer — it
    // outlives neither the dispatch that carried it nor the frame that
    // held it.
    let body = call(mrb, Target::Path(KV_PATH), "each_key", Vec::new(), block)?;
    EachResponse::decode(&body[..])
        .map(|answer| answer.keys as i32)
        .map_err(|err| wire_error(mrb, &err.to_string()))
}

/// Read one yield's arguments — the shell's codec hands this position
/// here, since the schema is the gem's.
pub fn decode_yield(mrb: &Mrb, bytes: &[u8]) -> Option<Vec<Value>> {
    let yielded = YieldKey::decode(bytes).ok()?;
    Some(vec![mrb.str_new(&yielded.key).as_value()])
}
