//! The SDK's guest→host dispatch handler.
//!
//! The twin of the Ruby gem's `Transport::Dispatcher` contract: it
//! **never fails** — every refusal, decode fault, and unencodable
//! response folds into a `Response::Err` fault envelope the guest
//! re-raises as a rescuable exception, so a Service misuse can never
//! become a wasm trap.

use std::sync::{Arc, Mutex};

use kobako_codec::codec::{Decode, Encode, Encoder, Value};
use kobako_codec::transport::{Request, Response, Target};
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::yielder::Yielder;

use crate::block::Block;
use crate::catalog::Catalog;
use crate::handles::{HandleTable, Handles};
use crate::member::{Fault, FaultKind, Member};

/// `DispatchHandler` over a sealed Catalog and the invocation's Handle
/// table: route each Request to its bound Member or live Handle entry
/// and fold every failure into a fault envelope.
pub(crate) struct CatalogHandler {
    catalog: Arc<Catalog>,
    handles: Arc<Mutex<HandleTable>>,
}

impl CatalogHandler {
    pub(crate) fn new(catalog: Arc<Catalog>, handles: Arc<Mutex<HandleTable>>) -> Self {
        CatalogHandler { catalog, handles }
    }

    fn handle(&self, request: &Request, yielder: &mut dyn Yielder) -> Response {
        let member = match self.resolve_target(&request.target) {
            Ok(member) => member,
            Err(fault) => return fault_response(&fault),
        };
        // The target's own narrowing predicate answers before any
        // method runs; the rejection shares the `undefined` fault kind
        // of an unresolved target and the Ruby frontend's wording.
        if !member.respond_to_guest(&request.method) {
            return fault_response(&Fault::new(
                FaultKind::Undefined,
                format!("method :{} is not exposed to the guest", request.method),
            ));
        }
        let handles = Handles::new(&self.handles);
        let mut block = request.block_given.then(|| Block::new(yielder));
        let result = member.call(
            &request.method,
            &request.args,
            &request.kwargs,
            block.as_mut(),
            &handles,
        );
        // A break unwinds the member transparently: the guest receives
        // the break value no matter what the member returned, and the
        // value rides back verbatim rather than through host code.
        if let Some(value) = block.and_then(Block::into_break) {
            return Response::Ok(value);
        }
        match result {
            Ok(value) => Response::Ok(value),
            Err(fault) => fault_response(&fault),
        }
    }

    /// Resolve the Request target: a path against the sealed Catalog,
    /// a Handle id against the invocation's table. Either miss is the
    /// `undefined` fault the guest re-raises.
    fn resolve_target(&self, target: &Target) -> Result<Arc<dyn Member>, Fault> {
        match target {
            Target::Path(path) => self.catalog.lookup(path).ok_or_else(|| {
                Fault::new(FaultKind::Undefined, format!("unknown constant {path}"))
            }),
            Target::Handle(id) => self
                .handles
                .lock()
                .expect("the Handle table mutex is never poisoned")
                .get(*id)
                .ok_or_else(|| {
                    Fault::new(FaultKind::Undefined, format!("unknown Handle id: {id}"))
                }),
        }
    }
}

impl DispatchHandler for CatalogHandler {
    /// `None` is reserved for "the handler itself failed"; this
    /// handler reifies every failure as an envelope instead.
    fn dispatch(&self, request: &[u8], yielder: &mut dyn Yielder) -> Option<Vec<u8>> {
        let response = match Request::decode(request) {
            Ok(request) => self.handle(&request, yielder),
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
    use kobako_codec::transport::{Yield, TAG_BREAK, TAG_ERROR, TAG_OK};

    use crate::member::Member;

    use super::*;

    /// A yielder for tests: the handler under test never yields.
    struct NoYield;

    impl Yielder for NoYield {
        fn yield_block(&mut self, _args: &[u8]) -> Result<Vec<u8>, kobako_runtime::error::Trap> {
            panic!("dispatch under test must not yield");
        }
    }

    /// A yielder answering from a canned script of YieldResponse bytes.
    struct Scripted(std::collections::VecDeque<Vec<u8>>);

    impl Scripted {
        fn new(responses: Vec<(u8, Value)>) -> Self {
            Scripted(
                responses
                    .into_iter()
                    .map(|(tag, value)| Yield { tag, value }.encode().unwrap())
                    .collect(),
            )
        }
    }

    impl Yielder for Scripted {
        fn yield_block(&mut self, _args: &[u8]) -> Result<Vec<u8>, kobako_runtime::error::Trap> {
            Ok(self.0.pop_front().expect("script exhausted"))
        }
    }

    /// A Handle-table entry for the chaining tests: answers `label`
    /// with its tag.
    struct Tagged(&'static str);

    impl Member for Tagged {
        fn call(
            &self,
            method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut Block<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            match method {
                "label" => Ok(Value::Str(self.0.into())),
                _ => Err(Fault::new(FaultKind::Undefined, "no such method")),
            }
        }
    }

    struct Echo;

    impl Member for Echo {
        fn call(
            &self,
            method: &str,
            args: &[Value],
            kwargs: &[(String, Value)],
            block: Option<&mut Block<'_>>,
            handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            match method {
                "echo" => Ok(args.first().cloned().unwrap_or(Value::Nil)),
                "first_kwarg" => Ok(kwargs
                    .first()
                    .map(|(_, value)| value.clone())
                    .unwrap_or(Value::Nil)),
                "explode" => Err(Fault::new(FaultKind::Runtime, "boom")),
                "yield_each" => {
                    let block = block.expect("scenario always supplies a block here");
                    let mut out = Vec::with_capacity(args.len());
                    for arg in args {
                        out.push(block.call(std::slice::from_ref(arg))?);
                    }
                    Ok(Value::Array(out))
                }
                "ignores_block" => Ok(Value::Sym("ok".into())),
                "swallow_break" => {
                    let block = block.expect("scenario always supplies a block here");
                    let _ = block.call(&[Value::Int(0)]);
                    Ok(Value::Sym("swallowed".into()))
                }
                "make" => handles.alloc(Arc::new(Tagged("bob"))),
                "read_label" => {
                    let object = args
                        .first()
                        .and_then(|arg| handles.resolve(arg))
                        .ok_or_else(|| Fault::new(FaultKind::Runtime, "not a live Handle"))?;
                    object.call("label", &[], &[], None, handles)
                }
                _ => Err(Fault::new(FaultKind::Undefined, "no such method")),
            }
        }
    }

    fn handler() -> CatalogHandler {
        let mut catalog = Catalog::default();
        catalog.bind("MyService", "KV", Arc::new(Echo));
        CatalogHandler::new(Arc::new(catalog), Arc::default())
    }

    fn roundtrip(request: &Request) -> Response {
        roundtrip_with(request, &mut NoYield)
    }

    fn roundtrip_with(request: &Request, yielder: &mut dyn Yielder) -> Response {
        roundtrip_on(&handler(), request, yielder)
    }

    fn roundtrip_on(
        handler: &CatalogHandler,
        request: &Request,
        yielder: &mut dyn Yielder,
    ) -> Response {
        let bytes = handler
            .dispatch(&request.encode().unwrap(), yielder)
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
    fn unknown_handle_target_folds_into_an_undefined_fault() {
        let req = request(Target::Handle(1), "echo", vec![]);
        assert!(matches!(roundtrip(&req), Response::Err(_)));
    }

    #[test]
    fn allocated_handle_routes_the_next_dispatch_to_its_object() {
        let handler = handler();
        let make = request(Target::Path("MyService::KV".into()), "make", vec![]);
        let Response::Ok(token) = roundtrip_on(&handler, &make, &mut NoYield) else {
            panic!("make must answer with a Handle token");
        };
        assert_eq!(
            token,
            Value::Handle(1),
            "the first id of an invocation is 1"
        );

        let Value::Handle(id) = token else {
            unreachable!("asserted above");
        };
        let chained = request(Target::Handle(id), "label", vec![]);
        assert_eq!(
            roundtrip_on(&handler, &chained, &mut NoYield),
            Response::Ok(Value::Str("bob".into())),
            "a Handle target must route to the very object the allocation bound"
        );
    }

    /// An Echo narrowed to its `echo` method by the opt-in predicate.
    struct Narrowed;

    impl Member for Narrowed {
        fn call(
            &self,
            method: &str,
            args: &[Value],
            kwargs: &[(String, Value)],
            block: Option<&mut Block<'_>>,
            handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Echo.call(method, args, kwargs, block, handles)
        }

        fn respond_to_guest(&self, method: &str) -> bool {
            method == "echo"
        }
    }

    #[test]
    fn narrowing_predicate_rejects_an_unexposed_method_before_it_runs() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService", "Narrow", Arc::new(Narrowed));
        let handler = CatalogHandler::new(Arc::new(catalog), Arc::default());
        let visible = request(
            Target::Path("MyService::Narrow".into()),
            "echo",
            vec![Value::Int(7)],
        );
        assert_eq!(
            roundtrip_on(&handler, &visible, &mut NoYield),
            Response::Ok(Value::Int(7)),
            "a truthy predicate answer leaves the call unchanged"
        );
        let hidden = request(Target::Path("MyService::Narrow".into()), "explode", vec![]);
        assert!(
            matches!(
                roundtrip_on(&handler, &hidden, &mut NoYield),
                Response::Err(_)
            ),
            "a falsy predicate answer must reject the dispatch before the method runs"
        );
    }

    #[test]
    fn narrowing_predicate_applies_to_a_handle_target() {
        let handles: Arc<Mutex<HandleTable>> = Arc::default();
        let id = handles.lock().unwrap().alloc(Arc::new(Narrowed)).unwrap();
        let handler = CatalogHandler::new(Arc::new(Catalog::default()), handles);
        let visible = request(Target::Handle(id), "echo", vec![Value::Int(7)]);
        assert_eq!(
            roundtrip_on(&handler, &visible, &mut NoYield),
            Response::Ok(Value::Int(7))
        );
        let hidden = request(Target::Handle(id), "explode", vec![]);
        assert!(
            matches!(
                roundtrip_on(&handler, &hidden, &mut NoYield),
                Response::Err(_)
            ),
            "a Handle-table entry's narrowing predicate must reject the dispatch like a bound Service's"
        );
    }

    #[test]
    fn handle_argument_resolves_to_the_live_object() {
        let handler = handler();
        let make = request(Target::Path("MyService::KV".into()), "make", vec![]);
        let Response::Ok(token) = roundtrip_on(&handler, &make, &mut NoYield) else {
            panic!("make must answer with a Handle token");
        };
        let read = request(
            Target::Path("MyService::KV".into()),
            "read_label",
            vec![token],
        );
        assert_eq!(
            roundtrip_on(&handler, &read, &mut NoYield),
            Response::Ok(Value::Str("bob".into())),
            "a Handle passed back as an argument must resolve to the bound object"
        );
    }

    #[test]
    fn malformed_request_bytes_fold_into_a_fault_envelope() {
        let bytes = handler().dispatch(&[0xd9], &mut NoYield).unwrap();
        assert!(matches!(
            Response::decode(&bytes).unwrap(),
            Response::Err(_)
        ));
    }

    fn block_request(method: &str, args: Vec<Value>) -> Request {
        let mut req = request(Target::Path("MyService::KV".into()), method, args);
        req.block_given = true;
        req
    }

    #[test]
    fn yield_results_flow_back_through_the_member_value() {
        let req = block_request("yield_each", vec![Value::Int(1), Value::Int(2)]);
        let mut yielder = Scripted::new(vec![(TAG_OK, Value::Int(10)), (TAG_OK, Value::Int(20))]);
        assert_eq!(
            roundtrip_with(&req, &mut yielder),
            Response::Ok(Value::Array(vec![Value::Int(10), Value::Int(20)]))
        );
    }

    #[test]
    fn break_answers_the_guest_with_the_break_value() {
        let req = block_request(
            "yield_each",
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
        );
        let mut yielder = Scripted::new(vec![
            (TAG_OK, Value::Int(10)),
            (TAG_BREAK, Value::Sym("stop".into())),
        ]);
        assert_eq!(
            roundtrip_with(&req, &mut yielder),
            Response::Ok(Value::Sym("stop".into()))
        );
    }

    #[test]
    fn break_overrides_even_a_member_that_swallows_it() {
        let req = block_request("swallow_break", vec![]);
        let mut yielder = Scripted::new(vec![(TAG_BREAK, Value::Sym("stop".into()))]);
        assert_eq!(
            roundtrip_with(&req, &mut yielder),
            Response::Ok(Value::Sym("stop".into())),
            "the guest must receive the break value even when the member discards BlockError::Break"
        );
    }

    #[test]
    fn member_that_never_yields_discards_the_block() {
        let req = block_request("ignores_block", vec![]);
        assert_eq!(
            roundtrip_with(&req, &mut NoYield),
            Response::Ok(Value::Sym("ok".into()))
        );
    }

    #[test]
    fn propagated_block_failure_folds_into_an_err_envelope() {
        let req = block_request("yield_each", vec![Value::Int(1)]);
        let mut yielder = Scripted::new(vec![(
            TAG_ERROR,
            Value::Map(vec![
                (
                    Value::Str("class".into()),
                    Value::Str("LocalJumpError".into()),
                ),
                (Value::Str("message".into()), Value::Str("crossed".into())),
            ]),
        )]);
        assert!(matches!(
            roundtrip_with(&req, &mut yielder),
            Response::Err(_)
        ));
    }
}
