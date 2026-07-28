//! The mruby-value bridge — everything that reads a value out of the VM
//! or builds one into it.
//!
//! This is the surface a payload codec is handed. A codec is the shell's
//! choice and may come from outside this repository, so what it may do to
//! the interpreter is what this module exposes and no more: mint a
//! Handle, narrow an integer, and read a class name through
//! `super::Kobako::mrb`. The dispatch bridge reaches the same surface for
//! the ivar and funcall readers.
//!
//! The two constructions carrying an invariant of their own — a minted
//! Handle and a narrowed Integer — are reachable only by calling them.
//! A codec that assembled either value itself could hand the guest a
//! Handle it can re-point or an Integer that is not the number the wire
//! carried.

use beni::Value;

use super::Kobako;

/// Mangled instance-variable name that `Kobako::Handle#initialize`
/// stores the Handle id under. Read back through `Kobako::extract_handle_id`
/// at every method dispatch — keeping the literal in a single
/// `const` makes the writer / reader pairing impossible to drift
/// silently when the ivar layout changes.
const HANDLE_ID_IVAR: &core::ffi::CStr = c"@__kobako_id__";

/// Largest Handle id the wire admits (docs/wire-contract.md § Capability
/// Handle). Named here because `Kobako::mint_handle` enforces it for
/// itself: every layer that can admit an id states the bound rather than
/// inheriting it from the one before.
const HANDLE_ID_MAX: u32 = 0x7fff_ffff;

/// An inbound integer fell outside the guest's signed 32-bit `Integer`
/// range, which the MRB_INT32 build cannot hold. `Kobako::narrow_int`
/// refuses it rather than saturating to the nearest bound; each call site
/// fails its path the way that path reports any malformed inbound payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegerOutOfRange(pub i128);

impl IntegerOutOfRange {
    /// Operator-facing message naming the value the guest could not hold.
    pub fn message(self) -> String {
        format!(
            "integer {} is outside the guest's 32-bit Integer range",
            self.0
        )
    }
}

impl Kobako {
    /// Collect the Array-of-String a `recv.method` funcall returns into
    /// a `Vec<String>`; empty when the call raises or returns a
    /// non-Array, so the Panic envelope still serialises cleanly under
    /// guest-class shenanigans. The element count is the C array
    /// length, not a `.length` dispatch — a hostile subclass cannot
    /// feed a guest-chosen size into `Vec::with_capacity`.
    fn strings_from_funcall(&self, recv: Value, method: &std::ffi::CStr) -> Vec<String> {
        let Ok(val) = recv.funcall(self.mrb(), method, &[]) else {
            return Vec::new();
        };
        if val.classname(self.mrb()) != "Array" {
            return Vec::new();
        }
        // SAFETY: classname check above proves Array-tagged.
        let ary = unsafe { beni::Array::from_value_unchecked(val) };
        let entries = ary.entries();
        let mut out = Vec::with_capacity(entries.len());
        for elem in entries {
            // Rendered, not read as bytes: a backtrace line lands in the
            // envelope, which requires UTF-8 of its text fields, so a line
            // that is not degrades to empty rather than costing the whole
            // diagnostic. Value paths read bytes instead — see the codec.
            out.push(elem.to_string(self.mrb()));
        }
        out
    }

    /// Collect `exc_val.backtrace` (an mruby `Array of String`) into a
    /// Rust `Vec<String>`. Used by the guest panic path
    /// (`crate::flows::eval` / `crate::flows::run`) to populate the Panic
    /// envelope's `backtrace` field
    /// (docs/wire/envelope.md § Panic).
    ///
    /// mruby's default build keeps the backtrace, so `.backtrace`
    /// returns an Array of String. If the runtime is ever rebuilt
    /// without keep-mode the call yields a non-Array value (typically
    /// `nil`), which reads as an empty backtrace.
    pub fn extract_backtrace(&self, exc_val: Value) -> Vec<String> {
        self.strings_from_funcall(exc_val, c"backtrace")
    }

    /// Snapshot every top-level constant currently defined on `Object`
    /// by calling `Object.constants` and unpacking the returned Symbol
    /// Array into a `Vec<String>`. Used by `__kobako_run` to name the
    /// entrypoints an unresolved one could have been: a baseline taken
    /// after kobako install + preamble materialise (before snippet
    /// replay) is subtracted from a post-replay snapshot, yielding the
    /// constants the preloaded snippets contributed.
    pub fn top_level_constants(&self) -> Vec<String> {
        // SAFETY: `mrb->object_class` lives until `mrb_close`; the
        // shim behind `RClass::to_value` reuses mruby's own boxing
        // logic.
        let object_value = unsafe { self.mrb().object_class().to_value(self.mrb()) };
        self.strings_from_funcall(object_value, c"constants")
    }

    /// Store `id_val` into a fresh `Kobako::Handle` instance's
    /// `@__kobako_id__` ivar. Used by the `Kobako::Handle#initialize`
    /// C bridge.
    pub fn set_handle_id(&self, target: Value, id_val: Value) -> Result<(), beni::Error> {
        let sym = self.mrb().intern_cstr(HANDLE_ID_IVAR);
        target.iv_set(self.mrb(), sym, id_val)
    }

    /// Read the `u32` Handle id stored in a `Kobako::Handle` instance's
    /// `@__kobako_id__` instance variable. Returns 0 when the ivar is
    /// missing, not a Fixnum, or carries a negative payload — the
    /// resolver downstream treats id 0 as undefined. The id is unboxed
    /// rather than
    /// round-tripped through the mruby string machinery, which would
    /// silently truncate above `i32::MAX` and cost a string allocation
    /// on every dispatch.
    pub fn extract_handle_id(&self, handle_val: Value) -> u32 {
        let id_sym = self.mrb().intern_cstr(HANDLE_ID_IVAR);
        use beni::FromValue;
        let id_val = handle_val.iv_get(self.mrb(), id_sym);
        let Some(id) = i32::from_value(id_val) else {
            return 0;
        };
        if id < 0 {
            0
        } else {
            id as u32
        }
    }

    /// Mint the `Kobako::Handle` naming `id`, frozen so the guest cannot
    /// re-point it at an id it was never handed. The exact class matters as
    /// much as the freeze: dispatch derives a Handle target from an exact
    /// `Kobako::Handle` receiver, so a subclass would carry no target.
    ///
    /// The id cap is re-checked here rather than trusted from the caller.
    /// A codec is replaceable, so an id it failed to bound must not
    /// reach the `i32` the ivar holds and come back out as a different
    /// number.
    ///
    /// An id past the cap, like a Handle mruby declined to allocate,
    /// degrades to `nil`: the guest then holds a value that answers no
    /// dispatch, which fails at its next call rather than silently naming
    /// something else.
    pub fn mint_handle(&self, id: u32) -> Value {
        use beni::IntoValue;
        if id > HANDLE_ID_MAX {
            return Value::nil();
        }
        let mrb = self.mrb();
        self.handle_class
            .obj_new(mrb, &[(id as i32).into_value(mrb)])
            .map(|handle| handle.freeze(mrb))
            .unwrap_or(Value::nil())
    }

    /// Represent `n` as an mruby `Integer`, refusing anything the MRB_INT32
    /// build cannot hold rather than saturating it — neither side may ever
    /// see a different number than the wire carried
    /// (docs/wire/payload-msgpack.md § Integer Range).
    pub fn narrow_int<N>(&self, n: N) -> Result<Value, IntegerOutOfRange>
    where
        N: TryInto<i32> + Into<i128> + Copy,
    {
        use beni::IntoValue;
        match n.try_into() {
            Ok(narrowed) => Ok(i32::into_value(narrowed, self.mrb())),
            Err(_) => Err(IntegerOutOfRange(n.into())),
        }
    }
}
