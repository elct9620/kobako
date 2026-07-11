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
use kobako_runtime::yielder::Yielder as RawYielder;

use crate::catalog::Catalog;
use crate::handles::{HandleTable, Handles};
use crate::receiver::{Fault, FaultKind, Receiver};
use crate::yielder::Yielder;

/// `DispatchHandler` over a sealed Catalog and the invocation's Handle
/// table: resolve each Request's target to its Receiver and fold every
/// failure into a fault envelope.
pub(crate) struct CatalogHandler {
    catalog: Arc<Catalog>,
    handles: Arc<Mutex<HandleTable>>,
}

impl CatalogHandler {
    pub(crate) fn new(catalog: Arc<Catalog>, handles: Arc<Mutex<HandleTable>>) -> Self {
        CatalogHandler { catalog, handles }
    }

    fn handle(&self, request: &Request, channel: &mut dyn RawYielder) -> Response {
        let object = match self.resolve_target(&request.target) {
            Ok(object) => object,
            Err(fault) => return fault_response(&fault),
        };
        // The target's own narrowing predicate answers before any
        // method runs; the rejection shares the `undefined` fault kind
        // of an unresolved target and the Ruby frontend's wording.
        if !object.respond_to_guest(&request.method) {
            return fault_response(&Fault::new(
                FaultKind::Undefined,
                format!("method :{} is not exposed to the guest", request.method),
            ));
        }
        let handles = Handles::new(&self.handles);
        let mut block = request.block_given.then(|| Yielder::new(channel));
        let result = object.call(
            &request.method,
            &request.args,
            &request.kwargs,
            block.as_mut(),
            &handles,
        );
        // A break unwinds the receiver transparently: the guest receives
        // the break value no matter what the receiver returned, and the
        // value rides back verbatim rather than through host code.
        if let Some(value) = block.and_then(Yielder::into_break) {
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
    fn resolve_target(&self, target: &Target) -> Result<Arc<dyn Receiver>, Fault> {
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
    fn dispatch(&self, request: &[u8], channel: &mut dyn RawYielder) -> Option<Vec<u8>> {
        let response = match Request::decode(request) {
            Ok(request) => self.handle(&request, channel),
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
    use kobako_codec::codec::Decoder;
    use kobako_codec::transport::{Yield, TAG_BREAK, TAG_ERROR, TAG_OK};

    use crate::receiver::Receiver;

    use super::*;

    /// A yield channel for tests: the handler under test never yields.
    struct NoYield;

    impl RawYielder for NoYield {
        fn yield_block(&mut self, _args: &[u8]) -> Result<Vec<u8>, kobako_runtime::error::Trap> {
            panic!("dispatch under test must not yield");
        }
    }

    /// A yield channel answering from a canned script of YieldResponse
    /// bytes.
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

    impl RawYielder for Scripted {
        fn yield_block(&mut self, _args: &[u8]) -> Result<Vec<u8>, kobako_runtime::error::Trap> {
            Ok(self.0.pop_front().expect("script exhausted"))
        }
    }

    /// A Handle-table entry for the chaining tests: answers `label`
    /// with its tag.
    struct Tagged(&'static str);

    impl Receiver for Tagged {
        fn call(
            &self,
            method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut Yielder<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            match method {
                "label" => Ok(Value::Str(self.0.into())),
                _ => Err(Fault::new(FaultKind::Undefined, "no such method")),
            }
        }
    }

    struct Echo;

    impl Receiver for Echo {
        fn call(
            &self,
            method: &str,
            args: &[Value],
            kwargs: &[(String, Value)],
            block: Option<&mut Yielder<'_>>,
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
        catalog.bind("MyService::KV", Arc::new(Echo));
        CatalogHandler::new(Arc::new(catalog), Arc::default())
    }

    fn roundtrip(request: &Request) -> Response {
        roundtrip_with(request, &mut NoYield)
    }

    fn roundtrip_with(request: &Request, channel: &mut dyn RawYielder) -> Response {
        roundtrip_on(&handler(), request, channel)
    }

    fn roundtrip_on(
        handler: &CatalogHandler,
        request: &Request,
        channel: &mut dyn RawYielder,
    ) -> Response {
        let bytes = handler
            .dispatch(&request.encode().unwrap(), channel)
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
    fn routed_call_returns_the_receiver_value() {
        let req = request(
            Target::Path("MyService::KV".into()),
            "echo",
            vec![Value::Int(7)],
        );
        assert_eq!(roundtrip(&req), Response::Ok(Value::Int(7)));
    }

    #[test]
    fn kwargs_reach_the_receiver_intact() {
        let mut req = request(Target::Path("MyService::KV".into()), "first_kwarg", vec![]);
        req.kwargs = vec![("limit".into(), Value::Int(9))];
        assert_eq!(roundtrip(&req), Response::Ok(Value::Int(9)));
    }

    /// The fault payload's `type` field — the discriminator the guest
    /// uses to pick the proxy-side error, so a test can tell a
    /// rejection kind apart from a receiver that ran and failed.
    fn fault_type(response: &Response) -> String {
        let Response::Err(bytes) = response else {
            panic!("expected a fault envelope, got a success response");
        };
        let Ok(Value::Map(pairs)) = Decoder::new(bytes).read_only_value() else {
            panic!("a fault payload is always a msgpack map");
        };
        pairs
            .into_iter()
            .find_map(|(key, value)| match (key, value) {
                (Value::Str(key), Value::Str(text)) if key == "type" => Some(text),
                _ => None,
            })
            .expect("a fault payload always carries a type field")
    }

    #[test]
    fn receiver_fault_folds_into_a_runtime_fault() {
        let req = request(Target::Path("MyService::KV".into()), "explode", vec![]);
        assert_eq!(
            fault_type(&roundtrip(&req)),
            "runtime",
            "a receiver failure through dispatch must fold into the runtime fault envelope"
        );
    }

    #[test]
    fn unknown_path_folds_into_an_undefined_fault() {
        let req = request(Target::Path("Nope::Nada".into()), "echo", vec![]);
        assert_eq!(
            fault_type(&roundtrip(&req)),
            "undefined",
            "an unbound path target through dispatch must fold into the undefined fault"
        );
    }

    #[test]
    fn unknown_handle_target_folds_into_an_undefined_fault() {
        let req = request(Target::Handle(1), "echo", vec![]);
        assert_eq!(
            fault_type(&roundtrip(&req)),
            "undefined",
            "an unissued Handle target through dispatch must fold into the undefined fault"
        );
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

    impl Receiver for Narrowed {
        fn call(
            &self,
            method: &str,
            args: &[Value],
            kwargs: &[(String, Value)],
            block: Option<&mut Yielder<'_>>,
            handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Echo.call(method, args, kwargs, block, handles)
        }

        fn respond_to_guest(&self, method: &str) -> bool {
            method == "echo"
        }
    }

    // The hidden method in both narrowing tests is `explode`, whose
    // body fails as a *runtime* fault when it runs: only the fault
    // type can tell "rejected before running" (undefined) apart from
    // "ran and failed" (runtime).
    #[test]
    fn narrowing_predicate_rejects_an_unexposed_method_before_it_runs() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService::Narrow", Arc::new(Narrowed));
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
        assert_eq!(
            fault_type(&roundtrip_on(&handler, &hidden, &mut NoYield)),
            "undefined",
            "a falsy predicate answer must reject the dispatch as undefined before the method runs"
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
        assert_eq!(
            fault_type(&roundtrip_on(&handler, &hidden, &mut NoYield)),
            "undefined",
            "a Handle-table entry's narrowing predicate must reject the dispatch as undefined like a bound Service's"
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
    fn malformed_request_bytes_fold_into_a_runtime_fault() {
        let bytes = handler().dispatch(&[0xd9], &mut NoYield).unwrap();
        assert_eq!(
            fault_type(&Response::decode(&bytes).unwrap()),
            "runtime",
            "undecodable request bytes must fold into the runtime fault envelope"
        );
    }

    fn block_request(method: &str, args: Vec<Value>) -> Request {
        let mut req = request(Target::Path("MyService::KV".into()), method, args);
        req.block_given = true;
        req
    }

    #[test]
    fn yield_results_flow_back_through_the_receiver_value() {
        let req = block_request("yield_each", vec![Value::Int(1), Value::Int(2)]);
        let mut channel = Scripted::new(vec![(TAG_OK, Value::Int(10)), (TAG_OK, Value::Int(20))]);
        assert_eq!(
            roundtrip_with(&req, &mut channel),
            Response::Ok(Value::Array(vec![Value::Int(10), Value::Int(20)]))
        );
    }

    #[test]
    fn break_answers_the_guest_with_the_break_value() {
        let req = block_request(
            "yield_each",
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
        );
        let mut channel = Scripted::new(vec![
            (TAG_OK, Value::Int(10)),
            (TAG_BREAK, Value::Sym("stop".into())),
        ]);
        assert_eq!(
            roundtrip_with(&req, &mut channel),
            Response::Ok(Value::Sym("stop".into()))
        );
    }

    #[test]
    fn break_overrides_even_a_receiver_that_swallows_it() {
        let req = block_request("swallow_break", vec![]);
        let mut channel = Scripted::new(vec![(TAG_BREAK, Value::Sym("stop".into()))]);
        assert_eq!(
            roundtrip_with(&req, &mut channel),
            Response::Ok(Value::Sym("stop".into())),
            "the guest must receive the break value even when the receiver discards YieldError::Break"
        );
    }

    #[test]
    fn receiver_that_never_yields_discards_the_block() {
        let req = block_request("ignores_block", vec![]);
        assert_eq!(
            roundtrip_with(&req, &mut NoYield),
            Response::Ok(Value::Sym("ok".into()))
        );
    }

    #[test]
    fn propagated_block_failure_folds_into_a_runtime_fault() {
        let req = block_request("yield_each", vec![Value::Int(1)]);
        let mut channel = Scripted::new(vec![(
            TAG_ERROR,
            Value::Map(vec![
                (
                    Value::Str("class".into()),
                    Value::Str("LocalJumpError".into()),
                ),
                (Value::Str("message".into()), Value::Str("crossed".into())),
            ]),
        )]);
        assert_eq!(
            fault_type(&roundtrip_with(&req, &mut channel)),
            "runtime",
            "a propagated block failure must fold into the runtime fault envelope"
        );
    }
}
