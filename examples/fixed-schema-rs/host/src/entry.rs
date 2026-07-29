//! Entering the guest at a preloaded entrypoint rather than with a
//! script.
//!
//! `#run` names a top-level constant and calls its `call`, so the guest
//! side of this is an ordinary Ruby class the host preloaded. What makes
//! it worth showing is that its capabilities arrive two different ways,
//! and the difference is who they belong to.
//!
//! The `env` Handle belongs to *this request*: it is built here, issued
//! by this invocation's table, and rides in the payload, so the
//! entrypoint is handed it and nothing else can name it. The bound
//! `MyService::KV` belongs to *this invocation*: `ctx.bind` fills the
//! declared path before the guest drives, and the gem's methods reach it
//! under the name they were compiled against.
//!
//! A host serving many requests from one Sandbox wants the first for
//! anything request-scoped — a name that outlives the request is a name
//! the next request could reach.

use std::sync::Arc;

use kobako::{Error, RunPayload, Sandbox};
use prost::Message;

use crate::schema::EntryRequest;
use crate::session::Session;
use crate::store::{ProtoKv, Store};
use crate::KV_PATH;

/// The constant `#run` resolves, and the snippet name it is preloaded
/// under.
pub const NAME: &str = "App";

/// The entrypoint, as the guest sees it. It touches one capability it
/// was handed and one it was bound, and nothing else is in scope.
pub const SOURCE: &str = r#"
class App
  def self.call(body, env)
    env.put("last", body)
    MyService::KV.put("seen", body)
    "handled=#{body} scoped=#{env.get("last")} shared=#{MyService::KV.get("seen")}"
  end
end
"#;

/// Serve one request through the preloaded entrypoint and print what it
/// answered.
pub fn serve(sandbox: &Sandbox, body: &[u8]) -> Result<(), String> {
    let answer = invoke(sandbox, body)?;
    println!("{NAME}.call returned: {}", String::from_utf8_lossy(&answer));
    Ok(())
}

/// Run one request through the entrypoint and hand back what it ended
/// on. The store is built here and dropped with the request, so neither
/// capability outlives it.
pub fn invoke(sandbox: &Sandbox, body: &[u8]) -> Result<Vec<u8>, String> {
    let store = Arc::new(Store::default());
    let scoped = store.clone();
    let execution = sandbox
        .run_with(
            NAME,
            // Built rather than final: the id standing for this request's
            // capability is issued by the table the verb is about to
            // create, so the payload cannot be finished before then.
            RunPayload::build(move |handles| {
                let id = handles
                    .alloc(Arc::new(Session::new(scoped, b"req/".to_vec())))
                    .map_err(|fault| Error::Argument(fault.message))?;
                Ok(EntryRequest {
                    body: body.to_vec(),
                    env: id,
                }
                .encode_to_vec())
            }),
            |ctx| ctx.bind(KV_PATH, Arc::new(ProtoKv(store.clone()))),
        )
        .map_err(|err| format!("the invocation did not start: {err}"))?;

    execution
        .payload()
        .map(Vec::from)
        .map_err(|failure| format!("the entrypoint failed: {failure}"))
}
