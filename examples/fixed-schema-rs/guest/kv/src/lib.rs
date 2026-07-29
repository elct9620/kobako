//! kv — a fixed-schema KV surface for a kobako mruby guest.
//!
//! Installs `MyService::KV` with `get` and `put` as real methods that
//! encode a protobuf schema and drive the guest transport directly. The
//! gem is where a fixed schema attaches: not at `MrbGuest::Codec`, which
//! is handed arguments with no method name, but at a call site that
//! already knows its own `(target, method)` — the pair the Call envelope
//! carries as the schema key on both sides.
//!
//! The bound path is fixed here because a schema is fixed to it. The
//! object behind that path is not: a host declares the path at setup and
//! fills it per invocation, so a static schema does not force a static
//! receiver.

pub mod blocks;
pub mod entry;

mod dispatch;
mod kv;
mod schema;
mod session;

use beni::{Error, Gem, Mrb};

/// The KV surface as an installable gem.
pub struct KvGem;

impl Gem for KvGem {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        kv::init(mrb)
    }
}
