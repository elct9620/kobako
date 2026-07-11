//! A note-taking host that runs untrusted mruby *plugins*.
//!
//! The host owns a small note store and lets a plugin batch-edit it —
//! but the plugin is untrusted mruby, so it runs inside a
//! `kobako::Sandbox` and can touch the host only through the
//! capabilities the host chose to grant. This example is the narrative
//! tour of the three SDK conveniences a low-level host would otherwise
//! assemble by hand:
//!
//!   * a **Service** (`Notes::Store`) the plugin calls like a constant;
//!   * a **capability Handle** — `Store.open` hands back a live `Note`
//!     the plugin calls methods on but can never serialize or forge;
//!   * a **block yield** — `note.each_tag { |t| … }` runs a guest block
//!     the host drives one tag at a time.
//!
//! After the plugin returns its note Handle, the host `resolve`s it back
//! to the very `Note` the plugin mutated and reads the final state — the
//! Rust spelling of restore-to-original-object.

use std::any::Any;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kobako::{
    Error, Fault, FaultKind, Handles, Options, Receiver, Sandbox, Value, YieldError, Yielder,
};

/// The plugin the host runs when none is given on the command line. It
/// reaches the host only through the granted capabilities: the
/// `Notes::Store` service and the note Handle `open` returns.
const DEFAULT_PLUGIN: &str = r##"
note = Notes::Store.open("welcome")

note.append("\n\n(reviewed by the tidy-tags plugin)")
note.tag("reviewed")
note.tag("greeting")   # already present - tag returns false

puts "#{note.title} - tags after tidy:"
count = note.each_tag { |name| puts "  * #{name}" }
puts "#{count} tag(s) total"

note
"##;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(wasm_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: kobako-plugin-host <path/to/kobako.wasm> [mruby-plugin]");
        return ExitCode::FAILURE;
    };
    let plugin = args
        .next()
        .unwrap_or_else(|| DEFAULT_PLUGIN.trim().to_string());

    // The same caps the Ruby gem exposes as `Sandbox` options; the
    // default profile is `Hermetic` - frozen clocks and entropy.
    let options = Options {
        timeout: Some(Duration::from_secs(5)),
        memory_limit: Some(64 * 1024 * 1024),
        stdout_limit: Some(64 * 1024),
        stderr_limit: Some(64 * 1024),
        ..Options::default()
    };
    let mut sandbox = match Sandbox::new(&wasm_path, options) {
        Ok(sandbox) => sandbox,
        Err(err) => {
            eprintln!("cannot load guest: {err}");
            return ExitCode::FAILURE;
        }
    };

    // The host state the plugin edits. `Store` is shared behind an `Arc`:
    // the same store the plugin dispatches into outlives the invocation,
    // so the edits are ordinary host state afterwards.
    let store = Arc::new(Store::seeded());
    if let Err(err) = sandbox.bind("Notes::Store", store.clone()) {
        eprintln!("cannot bind Notes::Store: {err}");
        return ExitCode::FAILURE;
    }

    // Captures and usage survive a guest failure, so read them before
    // classifying the outcome — output a plugin produced before it
    // raised is not lost.
    let result = sandbox.eval(&plugin);
    dump_output(&sandbox);
    match result {
        Ok(value) => report_success(&sandbox, &value),
        Err(err) => report_error(&err),
    }
}

/// A single note: an immutable id plus the mutable body and tags the
/// plugin edits. State lives behind a `Mutex` because `Receiver::call`
/// takes `&self` — the receiver crosses the engine boundary as
/// `Arc<dyn Receiver>` and answers dispatches through interior
/// mutability.
struct Note {
    id: String,
    state: Mutex<NoteState>,
}

struct NoteState {
    title: String,
    body: String,
    tags: Vec<String>,
}

impl Note {
    fn new(id: &str, title: &str, body: &str, tags: &[&str]) -> Self {
        Note {
            id: id.to_string(),
            state: Mutex::new(NoteState {
                title: title.to_string(),
                body: body.to_string(),
                tags: tags.iter().map(|t| t.to_string()).collect(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, NoteState> {
        self.state.lock().expect("the note mutex is never poisoned")
    }

    /// Yield each tag to the guest block one at a time, honouring a
    /// `break` by stopping early. Returns the number of tags yielded.
    fn each_tag(&self, block: Option<&mut Yielder<'_>>) -> Result<Value, Fault> {
        let Some(yielder) = block else {
            return Err(Fault::new(FaultKind::Argument, "each_tag requires a block"));
        };
        let tags = self.lock().tags.clone();
        let mut yielded = 0;
        for tag in tags {
            match yielder.call(&[Value::Str(tag)]) {
                Ok(_) => yielded += 1,
                Err(YieldError::Break) => break,
                Err(err) => return Err(err.into()),
            }
        }
        Ok(Value::Int(yielded))
    }
}

impl Receiver for Note {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Value, Fault> {
        if !kwargs.is_empty() {
            return Err(Fault::new(
                FaultKind::Argument,
                "Note methods take no keyword arguments",
            ));
        }
        match (method, args) {
            ("title", []) => Ok(Value::Str(self.lock().title.clone())),
            ("body", []) => Ok(Value::Str(self.lock().body.clone())),
            ("append", [Value::Str(text)]) => {
                self.lock().body.push_str(text);
                Ok(Value::Nil)
            }
            ("tag", [Value::Str(name)]) => {
                let mut state = self.lock();
                if state.tags.iter().any(|t| t == name) {
                    return Ok(Value::Bool(false));
                }
                state.tags.push(name.clone());
                Ok(Value::Bool(true))
            }
            ("each_tag", []) => self.each_tag(block),
            ("title" | "body" | "append" | "tag" | "each_tag", _) => Err(Fault::new(
                FaultKind::Argument,
                format!("Note##{method} was called with the wrong arguments"),
            )),
            (other, _) => Err(Fault::new(
                FaultKind::Undefined,
                format!("Note has no method :{other}"),
            )),
        }
    }
}

/// The note store bound as `Notes::Store`. Holds every note behind an
/// `Arc` so the Handle it hands the guest and the host both point at one
/// live object.
struct Store {
    notes: Mutex<HashMap<String, Arc<Note>>>,
}

impl Store {
    /// A store pre-seeded with the note the default plugin opens.
    fn seeded() -> Self {
        let welcome = Arc::new(Note::new(
            "welcome",
            "Welcome",
            "Welcome to kobako!",
            &["greeting"],
        ));
        let mut notes = HashMap::new();
        notes.insert(welcome.id.clone(), welcome);
        Store {
            notes: Mutex::new(notes),
        }
    }

    /// Look up a note, creating an empty one on first open — so a plugin
    /// can start a new note as well as edit a seeded one.
    fn open(&self, id: &str) -> Arc<Note> {
        self.notes
            .lock()
            .expect("the store mutex is never poisoned")
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Note::new(id, id, "", &[])))
            .clone()
    }
}

impl Receiver for Store {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        _block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Value, Fault> {
        if !kwargs.is_empty() {
            return Err(Fault::new(
                FaultKind::Argument,
                "Notes::Store methods take no keyword arguments",
            ));
        }
        match (method, args) {
            // `open` hands the guest a capability Handle: the live note
            // rides the wire as an opaque token, never a serialized copy.
            ("open", [Value::Str(id)]) => handles.alloc(self.open(id)),
            ("open", _) => Err(Fault::new(
                FaultKind::Argument,
                "Notes::Store.open takes one String note id",
            )),
            (other, _) => Err(Fault::new(
                FaultKind::Undefined,
                format!("Notes::Store has no method :{other}"),
            )),
        }
    }
}

/// The plugin returned a value: print it, then — when it returned a note
/// Handle — recover the very `Note` it mutated and read the final
/// host-side state.
fn report_success(sandbox: &Sandbox, value: &Value) -> ExitCode {
    println!("plugin returned: {}", render(value));

    if let Some(note) = resolve_note(sandbox, value) {
        let state = note.lock();
        println!();
        println!("host recovered note {:?} after the plugin ran:", note.id);
        println!("  title: {}", state.title);
        println!("  tags:  {}", state.tags.join(", "));
        println!("  body:  {:?}", state.body);
    }
    ExitCode::SUCCESS
}

/// Map every failure channel onto an exit code, the way a frontend
/// surfaces `Error` on its own error surface.
fn report_error(err: &Error) -> ExitCode {
    dump_error_details(err);
    eprintln!("plugin failed: {err}");
    ExitCode::FAILURE
}

/// Resolve a returned `Value::Handle` back to the concrete `Note`:
/// upcast the resolved `Arc<dyn Receiver>` to `Arc<dyn Any>` and
/// downcast — a plugin cannot fabricate a Handle, so a live id always
/// recovers the object the host allocated.
fn resolve_note(sandbox: &Sandbox, value: &Value) -> Option<Arc<Note>> {
    let receiver = sandbox.resolve(value)?;
    let any: Arc<dyn Any + Send + Sync> = receiver;
    any.downcast::<Note>().ok()
}

/// Print the captured stdout / stderr and the resource usage of the last
/// invocation.
fn dump_output(sandbox: &Sandbox) {
    dump_capture("stdout", sandbox.stdout(), sandbox.stdout_truncated());
    dump_capture("stderr", sandbox.stderr(), sandbox.stderr_truncated());
    if let Some(usage) = sandbox.usage() {
        println!(
            "usage: wall_time={:.6}s memory_peak={} bytes",
            usage.wall_time, usage.memory_peak
        );
    }
}

/// A guest failure keeps its captures — print them before the error so
/// output the plugin produced before it raised is not lost.
fn dump_error_details(err: &Error) {
    if let Error::Sandbox(failure) | Error::Service(failure) | Error::Bytecode(failure) = err {
        for line in &failure.backtrace {
            eprintln!("  {line}");
        }
    }
}

fn dump_capture(name: &str, bytes: &[u8], truncated: bool) {
    if bytes.is_empty() {
        return;
    }
    let clipped = if truncated { " (truncated)" } else { "" };
    println!("{name}{clipped}:");
    print!("{}", String::from_utf8_lossy(bytes));
}

/// Render a decoded wire `Value` in Ruby `#inspect` style so the printed
/// result reads like what the plugin returned.
fn render(value: &Value) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => format!("{s:?}"),
        Value::Bin(bytes) => format!("<{} binary bytes>", bytes.len()),
        Value::Sym(name) => format!(":{name}"),
        Value::Handle(id) => format!("#<Note Handle {id}>"),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(render).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Map(pairs) => {
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("{} => {}", render(k), render(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        Value::ErrEnv(bytes) => format!("<{} fault envelope bytes>", bytes.len()),
    }
}
