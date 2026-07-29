//! The `#run` entrypoint's arguments.
//!
//! `#run` names a top-level constant and calls its `call`, so the schema
//! key here is the entrypoint the host asked for. The codec is handed
//! the payload without it — as with a dispatch, the name is in the same
//! frame and not passed down — so a guest with one entrypoint convention
//! reads one request message, and a guest with several argument shapes
//! would need a schema that says which it is.
//!
//! This one takes `App.call(body, env)`: the request itself, and the
//! capability the host issued for this run. That is the whole of what an
//! entrypoint gets, which is the point — nothing ambient, nothing left
//! over from a previous run.

use beni::{Mrb, Value};
use prost::Message;

use crate::schema::EntryRequest;
use crate::session;

/// Read a `#run` payload into the arguments `App.call` receives.
///
/// `None` means this schema cannot read those bytes. Minting the
/// capability can only fail if the gem never installed its class, which
/// would have failed the boot, so it folds into the same answer rather
/// than growing a second error the caller has no separate use for.
pub fn decode(mrb: &Mrb, bytes: &[u8]) -> Option<Vec<Value>> {
    let request = EntryRequest::decode(bytes).ok()?;
    let body = mrb.str_new(&request.body).as_value();
    let env = session::mint(mrb, request.env).ok()?;
    Some(vec![body, env])
}
