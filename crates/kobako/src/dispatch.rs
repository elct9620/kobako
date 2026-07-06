//! The SDK's guest→host dispatch handler.
//!
//! The twin of the Ruby gem's `Transport::Dispatcher` contract: it
//! **never fails** — every refusal, decode fault, and unencodable
//! response folds into a `Response::Err` fault envelope the guest
//! re-raises as a rescuable exception, so a Service misuse can never
//! become a wasm trap. Capability-Handle targets and block yields are
//! seams of a later build and fold into `runtime` faults for now.

use std::sync::Arc;

use kobako_codec::codec::{Decode, Encode, Encoder, Value};
use kobako_codec::transport::{Request, Response, Target};
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::yielder::Yielder;

use crate::catalog::Catalog;
use crate::member::{Fault, FaultKind};

/// `DispatchHandler` over a sealed Catalog: route each Request to its
/// bound Member and fold every failure into a fault envelope.
pub(crate) struct CatalogHandler {
    catalog: Arc<Catalog>,
}

impl CatalogHandler {
    pub(crate) fn new(catalog: Arc<Catalog>) -> Self {
        CatalogHandler { catalog }
    }

    fn handle(&self, request: &Request) -> Response {
        let Target::Path(path) = &request.target else {
            return fault_response(&Fault::new(
                FaultKind::Runtime,
                "capability Handle dispatch is not yet implemented in this SDK build",
            ));
        };
        let Some(member) = self.catalog.lookup(path) else {
            return fault_response(&Fault::new(
                FaultKind::Undefined,
                format!("unknown constant {path}"),
            ));
        };
        match member.call(&request.method, &request.args, &request.kwargs) {
            Ok(value) => Response::Ok(value),
            Err(fault) => fault_response(&fault),
        }
    }
}

impl DispatchHandler for CatalogHandler {
    /// `None` is reserved for "the handler itself failed"; this
    /// handler reifies every failure as an envelope instead.
    fn dispatch(&self, request: &[u8], _yielder: &mut dyn Yielder) -> Option<Vec<u8>> {
        let response = match Request::decode(request) {
            Ok(request) => self.handle(&request),
            Err(err) => fault_response(&Fault::new(
                FaultKind::Runtime,
                format!("Sandbox received a malformed request: {err}"),
            )),
        };
        let bytes = response.encode().unwrap_or_else(|err| {
            // A value the wire cannot carry back folds like every
            // other failure; the flat fault map itself always encodes.
            fault_response(&Fault::new(
                FaultKind::Runtime,
                format!("response not encodable: {err}"),
            ))
            .encode()
            .expect("a flat fault map always encodes")
        });
        Some(bytes)
    }
}

/// A `Response::Err` carrying the ext 0x02 fault payload — a msgpack
/// map of `type` (which proxy-side error the guest raises) and
/// `message`.
fn fault_response(fault: &Fault) -> Response {
    let mut encoder = Encoder::new();
    encoder
        .write_value(&Value::Map(vec![
            (
                Value::Str("type".into()),
                Value::Str(fault.kind.wire_name().into()),
            ),
            (
                Value::Str("message".into()),
                Value::Str(fault.message.clone()),
            ),
        ]))
        .expect("a str/str fault map always encodes");
    Response::Err(encoder.into_bytes())
}

#[cfg(test)]
mod tests {
    use crate::member::Member;

    use super::*;

    /// A yielder for tests: the handler under test never yields.
    struct NoYield;

    impl Yielder for NoYield {
        fn yield_block(&mut self, _args: &[u8]) -> Result<Vec<u8>, kobako_runtime::error::Trap> {
            panic!("dispatch under test must not yield");
        }
    }

    struct Echo;

    impl Member for Echo {
        fn call(
            &self,
            method: &str,
            args: &[Value],
            kwargs: &[(String, Value)],
        ) -> Result<Value, Fault> {
            match method {
                "echo" => Ok(args.first().cloned().unwrap_or(Value::Nil)),
                "first_kwarg" => Ok(kwargs
                    .first()
                    .map(|(_, value)| value.clone())
                    .unwrap_or(Value::Nil)),
                "explode" => Err(Fault::new(FaultKind::Runtime, "boom")),
                _ => Err(Fault::new(FaultKind::Undefined, "no such method")),
            }
        }
    }

    fn handler() -> CatalogHandler {
        let mut catalog = Catalog::default();
        catalog.bind("MyService", "KV", Arc::new(Echo));
        CatalogHandler::new(Arc::new(catalog))
    }

    fn roundtrip(request: &Request) -> Response {
        let bytes = handler()
            .dispatch(&request.encode().unwrap(), &mut NoYield)
            .expect("this handler never returns None");
        Response::decode(&bytes).unwrap()
    }

    fn request(target: Target, method: &str, args: Vec<Value>) -> Request {
        Request {
            target,
            method: method.into(),
            args,
            kwargs: vec![],
            block_given: false,
        }
    }

    #[test]
    fn routed_call_returns_the_member_value() {
        let req = request(
            Target::Path("MyService::KV".into()),
            "echo",
            vec![Value::Int(7)],
        );
        assert_eq!(roundtrip(&req), Response::Ok(Value::Int(7)));
    }

    #[test]
    fn kwargs_reach_the_member_intact() {
        let mut req = request(Target::Path("MyService::KV".into()), "first_kwarg", vec![]);
        req.kwargs = vec![("limit".into(), Value::Int(9))];
        assert_eq!(roundtrip(&req), Response::Ok(Value::Int(9)));
    }

    #[test]
    fn member_fault_folds_into_an_err_envelope() {
        let req = request(Target::Path("MyService::KV".into()), "explode", vec![]);
        assert!(matches!(roundtrip(&req), Response::Err(_)));
    }

    #[test]
    fn unknown_path_folds_into_an_undefined_fault() {
        let req = request(Target::Path("Nope::Nada".into()), "echo", vec![]);
        assert!(matches!(roundtrip(&req), Response::Err(_)));
    }

    #[test]
    fn handle_target_folds_into_a_fault_not_a_panic() {
        let req = request(Target::Handle(1), "echo", vec![]);
        assert!(matches!(roundtrip(&req), Response::Err(_)));
    }

    #[test]
    fn malformed_request_bytes_fold_into_a_fault_envelope() {
        let bytes = handler().dispatch(&[0xd9], &mut NoYield).unwrap();
        assert!(matches!(
            Response::decode(&bytes).unwrap(),
            Response::Err(_)
        ));
    }
}
