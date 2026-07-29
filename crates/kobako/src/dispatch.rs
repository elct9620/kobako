//! The SDK's guest→host dispatch handler.
//!
//! The twin of the Ruby gem's `Transport::Dispatcher` contract: it
//! **never fails** — every refusal, decode fault, and unencodable
//! response folds into the Reply's fault arm, which the guest re-raises
//! as a rescuable exception, so a Service misuse can never become a
//! wasm trap.

use std::sync::{Arc, Mutex};

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::yielder::Yielder as RawYielder;
use kobako_transport::envelope::{Call, Reply, Target};

use crate::catalog::Catalog;
use crate::handles::{HandleTable, Handles};
use crate::receiver::{Fault, FaultKind, Receiver};
use crate::yielder::Yielder;

/// `DispatchHandler` over a sealed Catalog and the invocation's Handle
/// table: resolve each Call's target to its Receiver and fold every
/// failure into a fault envelope.
pub(crate) struct CatalogHandler {
    catalog: Arc<Catalog>,
    handles: Arc<Mutex<HandleTable>>,
    /// The paths this invocation resolves ahead of the sealed Catalog — its
    /// `ctx.bind` overrides first, then each `PerInvocation` provider's fresh
    /// object — so an override or a fresh backend serves its path while
    /// Frame 1 stays fixed.
    resolved: Vec<(String, Arc<dyn Receiver>)>,
}

impl CatalogHandler {
    pub(crate) fn new(
        catalog: Arc<Catalog>,
        handles: Arc<Mutex<HandleTable>>,
        resolved: Vec<(String, Arc<dyn Receiver>)>,
    ) -> Self {
        CatalogHandler {
            catalog,
            handles,
            resolved,
        }
    }

    /// Routes a Call to its Receiver and hands the payload through
    /// untouched: what the bytes mean is the Receiver's own schema, so
    /// this layer never decodes them.
    fn handle(&self, call: &Call<'_>, channel: &mut dyn RawYielder) -> Reply {
        let object = match self.resolve_target(&call.target) {
            Ok(object) => object,
            Err(fault) => return fault_reply(&fault),
        };
        // The target's own narrowing predicate answers before any
        // method runs; the rejection shares the `undefined` fault kind
        // of an unresolved target and the Ruby frontend's wording.
        if !object.respond_to_guest(call.method) {
            return fault_reply(&Fault::new(
                FaultKind::Undefined,
                format!("method :{} is not exposed to the guest", call.method),
            ));
        }
        let handles = Handles::new(&self.handles);
        let mut block = call.block_given.then(|| Yielder::new(channel));
        let result = object.call(call.method, call.payload, block.as_mut(), &handles);
        // A break unwinds the receiver transparently: the guest receives
        // the break value no matter what the receiver returned, and the
        // bytes ride back exactly as the block wrote them.
        if let Some(body) = block.and_then(Yielder::into_break) {
            return Reply::Ok(body);
        }
        match result {
            Ok(body) => Reply::Ok(body),
            Err(fault) => fault_reply(&fault),
        }
    }

    /// Resolve the Call target: a path against the sealed Catalog,
    /// a Handle id against the invocation's table. Either miss is the
    /// `undefined` fault the guest re-raises.
    fn resolve_target(&self, target: &Target) -> Result<Arc<dyn Receiver>, Fault> {
        match target {
            Target::Path(path) => self
                .resolved
                .iter()
                .find(|(bound, _)| bound == path)
                .map(|(_, object)| object.clone())
                .or_else(|| self.catalog.lookup(path))
                .ok_or_else(|| {
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
    fn dispatch(&self, call: Call<'_>, channel: &mut dyn RawYielder) -> Option<Reply> {
        Some(self.handle(&call, channel))
    }
}

/// The fault arm carries the Fault itself: it is an envelope shape, so
/// this frontend hands it on rather than encoding it — which is what
/// leaves a host free to answer every other position in its own schema.
fn fault_reply(fault: &Fault) -> Reply {
    Reply::Fault(fault.clone())
}

// The handler routes bytes, but a test needs a Service with behaviour to
// route to, and writing one against a value tree is how a reader follows
// what each case asserts. That is the overlay's spelling, so these cases
// stand with it; the byte-level path they share is walked end-to-end
// against the real guest in `tests/byte_surface.rs`.
#[cfg(all(test, feature = "msgpack"))]
mod tests {
    use kobako_codec::msgpack::codec::{Decoder, Encode, Encoder, Value};
    use kobako_codec::msgpack::payload::Arguments;
    use kobako_transport::envelope::{ErrorRecord, YieldReply};

    use crate::msgpack::ValueReceiver;

    use super::*;

    /// A yield channel for tests: the handler under test never yields.
    struct NoYield;

    impl RawYielder for NoYield {
        fn yield_to_block(&mut self, _args: &[u8]) -> Result<Vec<u8>, kobako_runtime::error::Trap> {
            panic!("dispatch under test must not yield");
        }
    }

    /// A yield channel answering from a canned script of Yield Reply
    /// bytes.
    struct Scripted(std::collections::VecDeque<Vec<u8>>);

    impl Scripted {
        fn new(replies: Vec<YieldReply>) -> Self {
            Scripted(replies.into_iter().map(|reply| reply.encode()).collect())
        }
    }

    /// A value-carrying Yield Reply arm over a codec-encoded payload.
    fn arm(make: fn(Vec<u8>) -> YieldReply, value: Value) -> YieldReply {
        make(Encoder::encode(&value).unwrap())
    }

    impl RawYielder for Scripted {
        fn yield_to_block(&mut self, _args: &[u8]) -> Result<Vec<u8>, kobako_runtime::error::Trap> {
            Ok(self.0.pop_front().expect("script exhausted"))
        }
    }

    /// A Handle-table entry for the chaining tests: answers `label`
    /// with its tag.
    struct Tagged(&'static str);

    impl ValueReceiver for Tagged {
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

    impl ValueReceiver for Echo {
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
                        out.push(block.call_values(std::slice::from_ref(arg))?);
                    }
                    Ok(Value::Array(out))
                }
                "ignores_block" => Ok(Value::Sym("ok".into())),
                "swallow_break" => {
                    let block = block.expect("scenario always supplies a block here");
                    let _ = block.call_values(&[Value::Int(0)]);
                    Ok(Value::Sym("swallowed".into()))
                }
                "make" => handles
                    .alloc(Tagged("bob").into_receiver())
                    .map(Value::Handle),
                "read_label" => {
                    // A Handle is an id wherever it travels; this schema
                    // spells one as `Value::Handle`, so reading it out is
                    // a destructure and the table takes it from there.
                    let object = match args.first() {
                        Some(Value::Handle(id)) => handles.resolve(*id),
                        _ => None,
                    }
                    .ok_or_else(|| Fault::new(FaultKind::Runtime, "not a live Handle"))?;
                    // Reaching another Receiver means speaking its schema:
                    // this one stands at the value seam, so encode the empty
                    // argument payload and decode what it answers with.
                    let payload = Arguments::default()
                        .encode()
                        .map_err(|err| Fault::new(FaultKind::Runtime, err.to_string()))?;
                    let body = object.call("label", &payload, None, handles)?;
                    Decoder::new(&body)
                        .read_only_value()
                        .map_err(|err| Fault::new(FaultKind::Runtime, err.to_string()))
                }
                _ => Err(Fault::new(FaultKind::Undefined, "no such method")),
            }
        }
    }

    fn handler() -> CatalogHandler {
        let mut catalog = Catalog::default();
        catalog.bind("MyService::KV", Echo.into_receiver());
        CatalogHandler::new(Arc::new(catalog), Arc::default(), Vec::new())
    }

    /// One call in the owned form a `Call` envelope borrows from. The
    /// wire carries an opaque payload; these tests stay written in the
    /// `Value` vocabulary a Receiver actually sees.
    struct Sent {
        target: Target<'static>,
        method: String,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
        block_given: bool,
    }

    impl Sent {
        fn encode(&self) -> Vec<u8> {
            let payload = Arguments::new(self.args.clone(), self.kwargs.clone())
                .encode()
                .unwrap();
            Call {
                target: self.target,
                method: &self.method,
                block_given: self.block_given,
                payload: &payload,
            }
            .encode()
        }
    }

    /// A Reply read back in the same vocabulary: the arm the envelope
    /// tagged, plus the body the codec decoded.
    #[derive(Debug, PartialEq)]
    enum Answer {
        Ok(Value),
        Fault(Fault),
    }

    fn answer(reply: Reply) -> Answer {
        match reply {
            Reply::Ok(body) => Answer::Ok(Decoder::new(&body).read_only_value().unwrap()),
            Reply::Fault(fault) => Answer::Fault(fault),
        }
    }

    fn roundtrip(request: &Sent) -> Answer {
        roundtrip_with(request, &mut NoYield)
    }

    fn roundtrip_with(request: &Sent, channel: &mut dyn RawYielder) -> Answer {
        roundtrip_on(&handler(), request, channel)
    }

    fn roundtrip_on(
        handler: &CatalogHandler,
        request: &Sent,
        channel: &mut dyn RawYielder,
    ) -> Answer {
        let bytes = request.encode();
        let reply = handler
            .dispatch(Call::decode(&bytes).unwrap(), channel)
            .expect("this handler never returns None");
        answer(reply)
    }

    fn request(target: Target<'static>, method: &str, args: Vec<Value>) -> Sent {
        Sent {
            target,
            method: method.into(),
            args,
            kwargs: vec![],
            block_given: false,
        }
    }

    #[test]
    fn routed_call_returns_the_receiver_value() {
        let req = request(Target::Path("MyService::KV"), "echo", vec![Value::Int(7)]);
        assert_eq!(roundtrip(&req), Answer::Ok(Value::Int(7)));
    }

    #[test]
    fn kwargs_reach_the_receiver_intact() {
        let mut req = request(Target::Path("MyService::KV"), "first_kwarg", vec![]);
        req.kwargs = vec![("limit".into(), Value::Int(9))];
        assert_eq!(roundtrip(&req), Answer::Ok(Value::Int(9)));
    }

    /// The Fault's category — the discriminator the guest uses to pick
    /// the proxy-side error, so a test can tell a rejection kind apart
    /// from a receiver that ran and failed.
    fn fault_type(response: &Answer) -> &'static str {
        let Answer::Fault(fault) = response else {
            panic!("expected a fault envelope, got a success response");
        };
        fault.kind.name()
    }

    #[test]
    fn receiver_fault_folds_into_a_runtime_fault() {
        let req = request(Target::Path("MyService::KV"), "explode", vec![]);
        assert_eq!(
            fault_type(&roundtrip(&req)),
            "runtime",
            "a receiver failure through dispatch must fold into the runtime fault envelope"
        );
    }

    #[test]
    fn unknown_path_folds_into_an_undefined_fault() {
        let req = request(Target::Path("Nope::Nada"), "echo", vec![]);
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

    // Per-invocation resolution wins over the sealed Catalog: the placeholder
    // bound at install (Echo, no `label`) is shadowed by the fresh object.
    #[test]
    fn resolution_wins_over_the_sealed_catalog() {
        let mut catalog = Catalog::default();
        catalog.bind("File", Echo.into_receiver());
        let handler = CatalogHandler::new(
            Arc::new(catalog),
            Arc::default(),
            vec![(
                "File".to_string(),
                Tagged("fresh").into_receiver() as Arc<dyn Receiver>,
            )],
        );
        let req = request(Target::Path("File"), "label", vec![]);
        assert_eq!(
            roundtrip_on(&handler, &req, &mut NoYield),
            Answer::Ok(Value::Str("fresh".into())),
            "a path resolved for this invocation must win over the sealed Catalog"
        );
    }

    #[test]
    fn allocated_handle_routes_the_next_dispatch_to_its_object() {
        let handler = handler();
        let make = request(Target::Path("MyService::KV"), "make", vec![]);
        let Answer::Ok(token) = roundtrip_on(&handler, &make, &mut NoYield) else {
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
            Answer::Ok(Value::Str("bob".into())),
            "a Handle target must route to the very object the allocation bound"
        );
    }

    /// An Echo narrowed to its `echo` method by the opt-in predicate.
    struct Narrowed;

    impl ValueReceiver for Narrowed {
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
        catalog.bind("MyService::Narrow", Narrowed.into_receiver());
        let handler = CatalogHandler::new(Arc::new(catalog), Arc::default(), Vec::new());
        let visible = request(
            Target::Path("MyService::Narrow"),
            "echo",
            vec![Value::Int(7)],
        );
        assert_eq!(
            roundtrip_on(&handler, &visible, &mut NoYield),
            Answer::Ok(Value::Int(7)),
            "a truthy predicate answer leaves the call unchanged"
        );
        let hidden = request(Target::Path("MyService::Narrow"), "explode", vec![]);
        assert_eq!(
            fault_type(&roundtrip_on(&handler, &hidden, &mut NoYield)),
            "undefined",
            "a falsy predicate answer must reject the dispatch as undefined before the method runs"
        );
    }

    #[test]
    fn narrowing_predicate_applies_to_a_handle_target() {
        let handles: Arc<Mutex<HandleTable>> = Arc::default();
        let id = handles
            .lock()
            .unwrap()
            .alloc(Narrowed.into_receiver())
            .unwrap();
        let handler = CatalogHandler::new(Arc::new(Catalog::default()), handles, Vec::new());
        let visible = request(Target::Handle(id), "echo", vec![Value::Int(7)]);
        assert_eq!(
            roundtrip_on(&handler, &visible, &mut NoYield),
            Answer::Ok(Value::Int(7))
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
        let make = request(Target::Path("MyService::KV"), "make", vec![]);
        let Answer::Ok(token) = roundtrip_on(&handler, &make, &mut NoYield) else {
            panic!("make must answer with a Handle token");
        };
        let read = request(Target::Path("MyService::KV"), "read_label", vec![token]);
        assert_eq!(
            roundtrip_on(&handler, &read, &mut NoYield),
            Answer::Ok(Value::Str("bob".into())),
            "a Handle passed back as an argument must resolve to the bound object"
        );
    }

    #[test]
    fn a_malformed_payload_folds_into_a_runtime_fault() {
        // The envelope is well-formed; only its payload is garbage — the
        // arm the codec owns, so the handler answers with a fault
        // rather than letting the failure reach the driver.
        let call = Call {
            target: Target::Path("MyService::KV"),
            method: "echo",
            block_given: false,
            payload: &[0xd9],
        };
        let reply = handler()
            .dispatch(call, &mut NoYield)
            .expect("this handler never returns None");
        assert_eq!(
            fault_type(&answer(reply)),
            "runtime",
            "an undecodable payload must fold into the runtime fault arm"
        );
    }

    fn block_request(method: &str, args: Vec<Value>) -> Sent {
        let mut req = request(Target::Path("MyService::KV"), method, args);
        req.block_given = true;
        req
    }

    #[test]
    fn yield_results_flow_back_through_the_receiver_value() {
        let req = block_request("yield_each", vec![Value::Int(1), Value::Int(2)]);
        let mut channel = Scripted::new(vec![
            arm(YieldReply::Ok, Value::Int(10)),
            arm(YieldReply::Ok, Value::Int(20)),
        ]);
        assert_eq!(
            roundtrip_with(&req, &mut channel),
            Answer::Ok(Value::Array(vec![Value::Int(10), Value::Int(20)]))
        );
    }

    #[test]
    fn break_answers_the_guest_with_the_break_value() {
        let req = block_request(
            "yield_each",
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
        );
        let mut channel = Scripted::new(vec![
            arm(YieldReply::Ok, Value::Int(10)),
            arm(YieldReply::Break, Value::Sym("stop".into())),
        ]);
        assert_eq!(
            roundtrip_with(&req, &mut channel),
            Answer::Ok(Value::Sym("stop".into()))
        );
    }

    #[test]
    fn break_overrides_even_a_receiver_that_swallows_it() {
        let req = block_request("swallow_break", vec![]);
        let mut channel = Scripted::new(vec![arm(YieldReply::Break, Value::Sym("stop".into()))]);
        assert_eq!(
            roundtrip_with(&req, &mut channel),
            Answer::Ok(Value::Sym("stop".into())),
            "the guest must receive the break value even when the receiver discards YieldError::Break"
        );
    }

    #[test]
    fn receiver_that_never_yields_discards_the_block() {
        let req = block_request("ignores_block", vec![]);
        assert_eq!(
            roundtrip_with(&req, &mut NoYield),
            Answer::Ok(Value::Sym("ok".into()))
        );
    }

    #[test]
    fn propagated_block_failure_folds_into_a_runtime_fault() {
        let req = block_request("yield_each", vec![Value::Int(1)]);
        let mut channel = Scripted::new(vec![YieldReply::Error(ErrorRecord {
            name: "LocalJumpError".into(),
            message: "crossed".into(),
            backtrace: Vec::new(),
        })]);
        assert_eq!(
            fault_type(&roundtrip_with(&req, &mut channel)),
            "runtime",
            "a propagated block failure must fold into the runtime fault envelope"
        );
    }
}
