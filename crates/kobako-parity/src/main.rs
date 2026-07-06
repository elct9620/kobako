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

use kobako::{Error, Fault, FaultKind, Member, Options, Profile, Sandbox, Value};

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
    for service in scenario["services"].as_array().unwrap_or(&Vec::new()) {
        bind_service(&mut sandbox, service)?;
    }
    for preload in scenario["preloads"].as_array().unwrap_or(&Vec::new()) {
        apply_preload(&mut sandbox, preload)?;
    }

    let invocations = scenario["invocations"]
        .as_array()
        .ok_or("scenario must carry invocations")?;
    let mut observables = Vec::with_capacity(invocations.len());
    for invocation in invocations {
        observables.push(observe(&mut sandbox, invocation)?);
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

fn bind_service(sandbox: &mut Sandbox, service: &Json) -> Result<(), String> {
    let namespace = service["namespace"]
        .as_str()
        .ok_or("service must carry namespace")?;
    let member = service["member"]
        .as_str()
        .ok_or("service must carry member")?;
    let mut methods = HashMap::new();
    if let Some(entries) = service["methods"].as_object() {
        for (name, behavior) in entries {
            methods.insert(name.clone(), parse_behavior(behavior)?);
        }
    }
    sandbox
        .bind(namespace, member, Arc::new(StubMember { methods }))
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
    /// Return a constant tagged value.
    Value(Value),
    /// The member itself fails (the Ruby stub raises; this side
    /// surfaces the SDK's member-failure channel).
    Raise(String),
}

fn parse_behavior(behavior: &Json) -> Result<Behavior, String> {
    match behavior["behavior"].as_str() {
        Some("echo") => Ok(Behavior::Echo),
        Some("value") => Ok(Behavior::Value(untag_value(&behavior["value"])?)),
        Some("raise") => {
            // The Ruby stub raises a RuntimeError and its dispatcher
            // folds the fault message as "<class>: <message>"; produce
            // the same wording so a guest that rescues and reads
            // `e.message` observes identically.
            let message = behavior["message"].as_str().unwrap_or("stub failure");
            Ok(Behavior::Raise(format!("RuntimeError: {message}")))
        }
        other => Err(format!("unknown stub behavior {other:?}")),
    }
}

/// A bound Member whose behavior is fully described by the scenario.
struct StubMember {
    methods: HashMap<String, Behavior>,
}

impl Member for StubMember {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        _kwargs: &[(String, Value)],
    ) -> Result<Value, Fault> {
        match self.methods.get(method) {
            Some(Behavior::Echo) => Ok(args.first().cloned().unwrap_or(Value::Nil)),
            Some(Behavior::Value(value)) => Ok(value.clone()),
            Some(Behavior::Raise(message)) => Err(Fault::new(FaultKind::Runtime, message.clone())),
            None => Err(Fault::new(
                FaultKind::Undefined,
                format!("method :{method} is not a Service method"),
            )),
        }
    }
}

/// Run one invocation and emit its raw observable object.
fn observe(sandbox: &mut Sandbox, invocation: &Json) -> Result<Json, String> {
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
            sandbox.run(target)
        }
        Some("late_bind") => late_bind(sandbox, invocation)?,
        other => return Err(format!("unknown invocation verb {other:?}")),
    };

    let mut observable = Map::new();
    match result {
        Ok(value) => {
            observable.insert("status".into(), json!("ok"));
            observable.insert("value".into(), tag_value(&value));
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
    let stub = StubMember {
        methods: HashMap::new(),
    };
    Ok(sandbox
        .bind(namespace, member, Arc::new(stub))
        .map(|()| Value::Nil))
}

/// The neutral parity status of each error variant, plus the guest
/// failure record when the variant carries one.
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
    }
}

/// Tagged JSON form of a wire `Value` — lossless (ints ride as
/// strings, bytes as hex, map order preserved) so both executors
/// render byte-identical tags.
fn tag_value(value: &Value) -> Json {
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
            json!({"t": "array", "v": items.iter().map(tag_value).collect::<Vec<_>>()})
        }
        Value::Map(pairs) => json!({
            "t": "map",
            "v": pairs
                .iter()
                .map(|(k, v)| json!([tag_value(k), tag_value(v)]))
                .collect::<Vec<_>>(),
        }),
        Value::Handle(id) => json!({"t": "handle", "id": id}),
        Value::ErrEnv(bytes) => json!({"t": "errenv", "hex": hex(bytes)}),
    }
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
