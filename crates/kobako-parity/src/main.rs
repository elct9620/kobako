//! The Rust half of the Ruby↔Rust differential parity harness.
//!
//! `test/parity/` sends one declarative scenario per length-prefixed
//! frame (the `CargoOracle` protocol); this bin assembles the same
//! Sandbox the Ruby executor assembles, runs the scenario's
//! invocations through the SDK, and answers with the **raw**
//! observables — status, tagged value, captures, usage — one object
//! per invocation. All normalization (what is compared, what is
//! diagnostic-only) lives on the Ruby side, so the two frontends'
//! outputs stay directly diffable.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map, Value as Json};

use kobako::{
    Error, Fault, FaultKind, Handles, Options, Profile, Receiver, RunArg, Sandbox, Value, Yielder,
};

/// The scenario's opaque host objects by declared label, shared by the
/// stub behaviors (allocation), the run-argument auto-wrap, and the
/// observable tagger (identity lookup via `Arc::ptr_eq`).
type Opaques = Vec<(String, Arc<dyn Receiver>)>;

/// High bit of the frame length word: the payload is a harness-level
/// error message, not an observables object.
const ERROR_FLAG: u32 = 0x8000_0000;

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    while let Some(frame) = read_frame(&mut input) {
        let (payload, flag) = match run_scenario(&frame) {
            Ok(observables) => (observables.to_string().into_bytes(), 0),
            Err(message) => (message.into_bytes(), ERROR_FLAG),
        };
        write_frame(&mut output, &payload, flag);
    }
}

fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header).ok()?;
    let len = u32::from_be_bytes(header) & !ERROR_FLAG;
    let mut body = vec![0u8; len as usize];
    input.read_exact(&mut body).ok()?;
    Some(body)
}

fn write_frame(output: &mut impl Write, payload: &[u8], flag: u32) {
    let word = (payload.len() as u32) | flag;
    output
        .write_all(&word.to_be_bytes())
        .and_then(|()| output.write_all(payload))
        .and_then(|()| output.flush())
        .expect("harness closed the pipe mid-frame");
}

/// Execute one scenario: build the Sandbox, apply registrations, run
/// every invocation, and collect the raw observables.
fn run_scenario(frame: &[u8]) -> Result<Json, String> {
    let scenario: Json =
        serde_json::from_slice(frame).map_err(|err| format!("malformed scenario JSON: {err}"))?;
    let wasm_path = scenario["wasm_path"]
        .as_str()
        .ok_or("scenario must carry wasm_path")?;
    let mut sandbox = Sandbox::new(wasm_path, parse_options(&scenario["options"])?)
        .map_err(|err| format!("Sandbox construction failed: {err}"))?;

    for namespace in scenario["defines"].as_array().unwrap_or(&Vec::new()) {
        let name = namespace
            .as_str()
            .ok_or("defines entries must be strings")?;
        sandbox
            .define(name)
            .map_err(|err| format!("define failed: {err}"))?;
    }
    let mut opaques: Opaques = Vec::new();
    for service in scenario["services"].as_array().unwrap_or(&Vec::new()) {
        bind_service(&mut sandbox, service, &mut opaques)?;
    }
    for preload in scenario["preloads"].as_array().unwrap_or(&Vec::new()) {
        apply_preload(&mut sandbox, preload)?;
    }

    let invocations = scenario["invocations"]
        .as_array()
        .ok_or("scenario must carry invocations")?;
    let mut observables = Vec::with_capacity(invocations.len());
    for invocation in invocations {
        observables.push(observe(&mut sandbox, invocation, &mut opaques)?);
    }
    Ok(Json::Array(observables))
}

/// The closed preload-kind set the Ruby executor interprets too.
/// Snippet failures are invocation-time observables (replay), so a
/// preload here never fails on a well-formed scenario.
fn apply_preload(sandbox: &mut Sandbox, preload: &Json) -> Result<(), String> {
    match preload["kind"].as_str() {
        Some("source") => {
            let name = preload["name"]
                .as_str()
                .ok_or("source preload must carry name")?;
            let code = preload["code"]
                .as_str()
                .ok_or("source preload must carry code")?;
            sandbox.preload(name, code)
        }
        Some("bytecode") => {
            let hex = preload["hex"]
                .as_str()
                .ok_or("bytecode preload must carry hex")?;
            sandbox.preload_binary(unhex(hex)?)
        }
        other => return Err(format!("unknown preload kind {other:?}")),
    }
    .map_err(|err| format!("preload failed: {err}"))
}

fn parse_options(options: &Json) -> Result<Options, String> {
    let mut parsed = Options {
        timeout: options["timeout_ms"].as_u64().map(Duration::from_millis),
        memory_limit: options["memory_limit"].as_u64().map(|n| n as usize),
        stdout_limit: options["stdout_limit"].as_u64().map(|n| n as usize),
        stderr_limit: options["stderr_limit"].as_u64().map(|n| n as usize),
        ..Options::default()
    };
    if let Some(profile) = options["profile"].as_str() {
        parsed.profile = match profile {
            "hermetic" => Profile::Hermetic,
            "permissive" => Profile::Permissive,
            other => return Err(format!("unknown profile {other:?}")),
        };
    }
    Ok(parsed)
}

fn bind_service(
    sandbox: &mut Sandbox,
    service: &Json,
    opaques: &mut Opaques,
) -> Result<(), String> {
    let namespace = service["namespace"]
        .as_str()
        .ok_or("service must carry namespace")?;
    let member = service["member"]
        .as_str()
        .ok_or("service must carry member")?;
    let mut methods = HashMap::new();
    if let Some(entries) = service["methods"].as_object() {
        for (name, behavior) in entries {
            methods.insert(name.clone(), parse_behavior(behavior, opaques)?);
        }
    }
    let exposed = match service["exposed"].as_array() {
        Some(names) => Some(
            names
                .iter()
                .map(|name| {
                    name.as_str()
                        .map(str::to_string)
                        .ok_or("exposed entries must be strings")
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        None => None,
    };
    sandbox
        .bind(
            namespace,
            member,
            Arc::new(StubReceiver { methods, exposed }),
        )
        .map_err(|err| format!("bind failed: {err}"))
}

/// The closed stub-behavior set both executors interpret identically;
/// it grows append-only alongside the scenario corpus. Fault kinds are
/// deliberately absent from the DSL: `undefined` / `argument` faults
/// must arise from the same conditions on both sides (a method the
/// stub does not have, a call shape it does not take), never from a
/// stub declaration.
enum Behavior {
    /// Return the first positional argument (or nil).
    Echo,
    /// Like `Echo`, but the declared signature takes no keyword
    /// arguments: kwargs on the wire are a parameter binding failure,
    /// mirroring the Ruby stub's positional-only lambda.
    EchoPositional,
    /// Return a constant tagged value.
    Value(Value),
    /// The receiver itself fails (the Ruby stub raises; this side
    /// surfaces the SDK's receiver-failure channel).
    Raise(String),
    /// Yield each positional argument to the guest block and return
    /// the array of block results.
    YieldEach,
    /// Hand the guest a labeled opaque host object as a capability
    /// Handle — the same instance on every call, so identity is
    /// observable.
    Opaque(Arc<dyn Receiver>),
    /// Answer with the label of the opaque object a (possibly
    /// Array-nested) Handle argument resolves to.
    ReadLabel,
}

fn parse_behavior(behavior: &Json, opaques: &mut Opaques) -> Result<Behavior, String> {
    match behavior["behavior"].as_str() {
        Some("echo") => Ok(Behavior::Echo),
        Some("echo_positional") => Ok(Behavior::EchoPositional),
        Some("value") => Ok(Behavior::Value(untag_value(&behavior["value"])?)),
        Some("raise") => {
            // The Ruby stub raises a RuntimeError and its dispatcher
            // folds the fault message as "<class>: <message>"; produce
            // the same wording so a guest that rescues and reads
            // `e.message` observes identically.
            let message = behavior["message"].as_str().unwrap_or("stub failure");
            Ok(Behavior::Raise(format!("RuntimeError: {message}")))
        }
        Some("yield_each") => Ok(Behavior::YieldEach),
        Some("opaque") => {
            let label = behavior["label"]
                .as_str()
                .ok_or("opaque behavior must carry label")?;
            Ok(Behavior::Opaque(register_opaque(opaques, label)))
        }
        Some("read_label") => Ok(Behavior::ReadLabel),
        other => Err(format!("unknown stub behavior {other:?}")),
    }
}

/// Create and register a labeled opaque object so the tagger can
/// recover its identity from a resolved Handle.
fn register_opaque(opaques: &mut Opaques, label: &str) -> Arc<dyn Receiver> {
    let object: Arc<dyn Receiver> = Arc::new(OpaqueStub {
        label: label.to_string(),
    });
    opaques.push((label.to_string(), object.clone()));
    object
}

/// A deliberately non-wire host object with a scenario-declared
/// identity; its only Service surface is `label`, mirroring the Ruby
/// executor's `Parity::OpaqueObject` struct.
struct OpaqueStub {
    label: String,
}

impl Receiver for OpaqueStub {
    fn call(
        &self,
        method: &str,
        _args: &[Value],
        _kwargs: &[(String, Value)],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Value, Fault> {
        match method {
            "label" => Ok(Value::Str(self.label.clone())),
            _ => Err(Fault::new(
                FaultKind::Undefined,
                format!("method :{method} is not a Service method"),
            )),
        }
    }
}

/// A bound Member whose behavior is fully described by the scenario.
/// An `exposed` list is the scenario's respond_to_guest? narrowing —
/// absent means the surface stays unchanged.
struct StubReceiver {
    methods: HashMap<String, Behavior>,
    exposed: Option<Vec<String>>,
}

impl Receiver for StubReceiver {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Value, Fault> {
        match self.methods.get(method) {
            Some(Behavior::Echo) => Ok(args.first().cloned().unwrap_or(Value::Nil)),
            Some(Behavior::EchoPositional) => {
                if kwargs.is_empty() {
                    Ok(args.first().cloned().unwrap_or(Value::Nil))
                } else {
                    // Ruby's binding failure counts the kwargs as one
                    // trailing positional hash; mirror the wording.
                    Err(Fault::new(
                        FaultKind::Argument,
                        format!(
                            "wrong number of arguments (given {}, expected 0..1)",
                            args.len() + 1
                        ),
                    ))
                }
            }
            Some(Behavior::Value(value)) => Ok(value.clone()),
            Some(Behavior::Raise(message)) => Err(Fault::new(FaultKind::Runtime, message.clone())),
            Some(Behavior::YieldEach) => yield_each(args, block),
            Some(Behavior::Opaque(object)) => handles.alloc(object.clone()),
            Some(Behavior::ReadLabel) => read_label(args, handles),
            None => Err(Fault::new(
                FaultKind::Undefined,
                format!("method :{method} is not a Service method"),
            )),
        }
    }

    fn respond_to_guest(&self, method: &str) -> bool {
        self.exposed
            .as_ref()
            .is_none_or(|names| names.iter().any(|name| name == method))
    }
}

/// Resolve the first (possibly Array-nested) Handle argument and
/// answer with its object's label — the Ruby stub reads `arg.label`
/// off the restored object the dispatcher handed it.
fn read_label(args: &[Value], handles: &Handles<'_>) -> Result<Value, Fault> {
    let mut arg = args
        .first()
        .ok_or_else(|| Fault::new(FaultKind::Runtime, "read_label needs an argument"))?;
    while let Value::Array(items) = arg {
        arg = items
            .first()
            .ok_or_else(|| Fault::new(FaultKind::Runtime, "read_label got an empty Array"))?;
    }
    let object = handles
        .resolve(arg)
        .ok_or_else(|| Fault::new(FaultKind::Runtime, "read_label needs a live Handle"))?;
    object.call("label", &[], &[], None, handles)
}

/// Yield each positional argument, collecting the block results. A
/// call without a block mirrors the Ruby stub's `nil.call` crash — a
/// runtime fault, never a stub-declared one.
fn yield_each(args: &[Value], block: Option<&mut Yielder<'_>>) -> Result<Value, Fault> {
    let Some(block) = block else {
        return Err(Fault::new(
            FaultKind::Runtime,
            "NoMethodError: undefined method 'call' for nil",
        ));
    };
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        out.push(block.call(std::slice::from_ref(arg))?);
    }
    Ok(Value::Array(out))
}

/// Run one invocation and emit its raw observable object.
fn observe(
    sandbox: &mut Sandbox,
    invocation: &Json,
    opaques: &mut Opaques,
) -> Result<Json, String> {
    let result = match invocation["verb"].as_str() {
        Some("eval") => {
            let source = invocation["source"]
                .as_str()
                .ok_or("eval invocation must carry source")?;
            sandbox.eval(source)
        }
        Some("run") => {
            let target = invocation["target"]
                .as_str()
                .ok_or("run invocation must carry target")?;
            let args = match invocation["args"].as_array() {
                Some(tagged) => tagged
                    .iter()
                    .map(|tag| untag_run_arg(tag, opaques))
                    .collect::<Result<_, _>>()?,
                None => Vec::new(),
            };
            let kwargs = match invocation["kwargs"].as_object() {
                Some(tagged) => tagged
                    .iter()
                    .map(|(key, tag)| Ok((key.clone(), untag_run_arg(tag, opaques)?)))
                    .collect::<Result<_, String>>()?,
                None => Vec::new(),
            };
            sandbox.run_with(target, args, kwargs)
        }
        Some("late_bind") => late_bind(sandbox, invocation)?,
        other => return Err(format!("unknown invocation verb {other:?}")),
    };

    let mut observable = Map::new();
    match result {
        Ok(value) => {
            observable.insert("status".into(), json!("ok"));
            observable.insert("value".into(), tag_value(&value, sandbox, opaques));
        }
        Err(error) => {
            let (status, failure) = classify(&error);
            observable.insert("status".into(), json!(status));
            if let Some(failure) = failure {
                observable.insert("class".into(), json!(failure.class));
                observable.insert("message".into(), json!(failure.message));
            }
        }
    }
    observable.insert("stdout_hex".into(), json!(hex(sandbox.stdout())));
    observable.insert("stderr_hex".into(), json!(hex(sandbox.stderr())));
    observable.insert("stdout_truncated".into(), json!(sandbox.stdout_truncated()));
    observable.insert("stderr_truncated".into(), json!(sandbox.stderr_truncated()));
    observable.insert(
        "usage".into(),
        match sandbox.usage() {
            Some(usage) => json!({
                "wall_time": usage.wall_time,
                "memory_peak": usage.memory_peak,
            }),
            None => Json::Null,
        },
    );
    Ok(Json::Object(observable))
}

/// A registration arriving after the first invocation: the seal
/// refusal is the observable; a successful bind observes as ok.
fn late_bind(sandbox: &mut Sandbox, invocation: &Json) -> Result<Result<Value, Error>, String> {
    let namespace = invocation["namespace"]
        .as_str()
        .ok_or("late_bind invocation must carry namespace")?;
    let member = invocation["member"]
        .as_str()
        .ok_or("late_bind invocation must carry member")?;
    let stub = StubReceiver {
        methods: HashMap::new(),
        exposed: None,
    };
    Ok(sandbox
        .bind(namespace, member, Arc::new(stub))
        .map(|()| Value::Nil))
}

/// The neutral parity status of each error variant, plus the guest
/// failure record when the variant carries one. The wildcard arm
/// answers a status the Ruby executor never produces, so an SDK error
/// variant this runner does not yet classify surfaces as a loud
/// parity mismatch instead of a silent bucket.
fn classify(error: &Error) -> (&'static str, Option<&kobako::GuestFailure>) {
    match error {
        Error::Timeout(_) => ("timeout", None),
        Error::MemoryLimit(_) => ("memory_limit", None),
        Error::Trap(_) => ("trap", None),
        Error::Sandbox(failure) => ("sandbox", Some(failure)),
        Error::Bytecode(failure) => ("bytecode", Some(failure)),
        Error::Service(failure) => ("service", Some(failure)),
        Error::Setup(_) => ("setup", None),
        Error::Sealed(_) => ("sealed", None),
        Error::Argument(_) => ("argument", None),
        _ => ("unclassified_error_variant", None),
    }
}

/// Tagged JSON form of a wire `Value` — lossless (ints ride as
/// strings, bytes as hex, map order preserved) so both executors
/// render byte-identical tags. A Handle tags as the identity of the
/// host object it resolves to — the Ruby executor sees the restored
/// object itself, so a raw id would never compare.
fn tag_value(value: &Value, sandbox: &Sandbox, opaques: &Opaques) -> Json {
    match value {
        Value::Nil => json!({"t": "nil"}),
        Value::Bool(b) => json!({"t": "bool", "v": b}),
        Value::Int(n) => json!({"t": "int", "v": n.to_string()}),
        Value::UInt(n) => json!({"t": "int", "v": n.to_string()}),
        Value::Float(f) => json!({"t": "float", "v": f}),
        Value::Str(s) => json!({"t": "str", "v": s}),
        Value::Bin(bytes) => json!({"t": "bin", "hex": hex(bytes)}),
        Value::Sym(name) => json!({"t": "sym", "v": name}),
        Value::Array(items) => {
            json!({"t": "array", "v": items.iter().map(|v| tag_value(v, sandbox, opaques)).collect::<Vec<_>>()})
        }
        Value::Map(pairs) => json!({
            "t": "map",
            "v": pairs
                .iter()
                .map(|(k, v)| json!([tag_value(k, sandbox, opaques), tag_value(v, sandbox, opaques)]))
                .collect::<Vec<_>>(),
        }),
        Value::Handle(_) => json!({"t": "opaque", "label": handle_label(value, sandbox, opaques)}),
        Value::ErrEnv(bytes) => json!({"t": "errenv", "hex": hex(bytes)}),
    }
}

/// The declared label of the opaque object a result Handle resolves
/// to; `None` for an object outside the scenario's opaque set (no
/// closed-DSL scenario produces one).
fn handle_label(value: &Value, sandbox: &Sandbox, opaques: &Opaques) -> Option<String> {
    let resolved = sandbox.resolve(value)?;
    opaques
        .iter()
        .find(|(_, object)| Arc::ptr_eq(object, &resolved))
        .map(|(label, _)| label.clone())
}

/// A `run` argument off its tagged form: the `opaque` tag becomes a
/// fresh labeled host object (registered so the tagger can recover its
/// identity), every other tag stays a wire value.
fn untag_run_arg(tagged: &Json, opaques: &mut Opaques) -> Result<RunArg, String> {
    if tagged["t"].as_str() == Some("opaque") {
        let label = tagged["label"]
            .as_str()
            .ok_or_else(|| format!("malformed tagged value: {tagged}"))?;
        return Ok(RunArg::Object(register_opaque(opaques, label)));
    }
    untag_value(tagged).map(RunArg::Value)
}

/// Tagged JSON back to a wire `Value` (stub constants in scenarios).
fn untag_value(tagged: &Json) -> Result<Value, String> {
    let err = || format!("malformed tagged value: {tagged}");
    match tagged["t"].as_str().ok_or_else(err)? {
        "nil" => Ok(Value::Nil),
        "bool" => Ok(Value::Bool(tagged["v"].as_bool().ok_or_else(err)?)),
        "int" => {
            // The tag rides both signed and unsigned wire ints as one
            // decimal string; accept the full range back.
            let digits = tagged["v"].as_str().ok_or_else(err)?;
            digits
                .parse()
                .map(Value::Int)
                .or_else(|_| digits.parse().map(Value::UInt))
                .map_err(|_| err())
        }
        "float" => Ok(Value::Float(tagged["v"].as_f64().ok_or_else(err)?)),
        "str" => Ok(Value::Str(tagged["v"].as_str().ok_or_else(err)?.into())),
        "bin" => Ok(Value::Bin(unhex(tagged["hex"].as_str().ok_or_else(err)?)?)),
        "sym" => Ok(Value::Sym(tagged["v"].as_str().ok_or_else(err)?.into())),
        "array" => {
            let items = tagged["v"].as_array().ok_or_else(err)?;
            Ok(Value::Array(
                items.iter().map(untag_value).collect::<Result<_, _>>()?,
            ))
        }
        "map" => {
            let pairs = tagged["v"].as_array().ok_or_else(err)?;
            let mut entries = Vec::with_capacity(pairs.len());
            for pair in pairs {
                let kv = pair.as_array().filter(|kv| kv.len() == 2).ok_or_else(err)?;
                entries.push((untag_value(&kv[0])?, untag_value(&kv[1])?));
            }
            Ok(Value::Map(entries))
        }
        _ => Err(err()),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("hex payload must have even length".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}
