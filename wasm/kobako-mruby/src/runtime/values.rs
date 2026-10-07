//! The mruby-value bridge — everything that reads a value out of the VM
//! or builds one into it.
//!
//! This is the surface a payload codec is handed. A codec is the shell's
//! choice and may come from outside this repository, so what it may do to
//! the interpreter is what this module exposes and no more: mint a
//! Handle, tell one from a look-alike, narrow an integer, and read a class
//! name through `super::Kobako::mrb`. The dispatch bridge reaches the same
//! surface for the ivar and funcall readers.
//!
//! The two constructions carrying an invariant of their own — a minted
//! Handle and a narrowed Integer — are reachable only by calling them.
//! A codec that assembled either value itself could hand the guest a
//! Handle it can re-point or an Integer that is not the number the wire
//! carried.

use beni::ReprValue;
use beni::Value;

use super::Kobako;

/// Mangled instance-variable name that `Kobako::Handle#initialize`
/// stores the Handle id under. Read back through `Kobako::extract_handle_id`
/// at every method dispatch — keeping the literal in a single
/// `const` makes the writer / reader pairing impossible to drift
/// silently when the ivar layout changes.
const HANDLE_ID_IVAR: &core::ffi::CStr = c"@__kobako_id__";

/// Largest Handle id the wire admits (docs/wire/README.md § Capability
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
    /// Empty when the call raises or answers anything else, so the Panic
    /// envelope still serialises under guest-class shenanigans. The element
    /// count comes from the C array rather than a `.length` dispatch, so a
    /// guest cannot choose it.
    fn strings_from_funcall(&self, recv: Value, method: &std::ffi::CStr) -> Vec<String> {
        use beni::FromValue;
        let Ok(val) = recv.funcall(self.mrb(), method, &[]) else {
            return Vec::new();
        };
        if val.classname(self.mrb()) != "Array" {
            return Vec::new();
        }
        // The tag proves the layout the name cannot, as in the codec's own
        // container arms.
        let Some(ary) = beni::RArray::from_value(val) else {
            return Vec::new();
        };
        let entries = ary.entries(self.mrb());
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

    /// The exception's backtrace, or empty for a runtime built without
    /// backtrace keep-mode.
    pub fn extract_backtrace(&self, exc_val: Value) -> Vec<String> {
        self.strings_from_funcall(exc_val, c"backtrace")
    }

    /// Every top-level constant currently defined on `Object`. It costs one
    /// allocation per name, so take it where the answer is kept.
    pub fn top_level_constants(&self) -> Vec<String> {
        let object_value = self.mrb().object_class().as_value();
        self.strings_from_funcall(object_value, c"constants")
    }

    /// Store `id_val` as a fresh `Kobako::Handle`'s id.
    pub fn set_handle_id(&self, target: Value, id_val: Value) -> Result<(), beni::Error> {
        use beni::{Object, RObject, TryConvert};
        RObject::try_convert(target, self.mrb())?.ivar_set(self.mrb(), HANDLE_ID_IVAR, id_val)
    }

    /// Whether `val` is a `Kobako::Handle` the decoder minted. The class the
    /// registration holds answers it, never the value's class name: an
    /// anonymous class takes the name of the constant it is assigned to, so
    /// the guest can name a class of its own after this one. A codec asks
    /// this before reading an id, the same question the dispatch seam asks
    /// of a receiver.
    pub fn is_handle(&self, val: Value) -> bool {
        val.is_instance_of(self.mrb(), self.registrations.handle_class)
    }

    /// The Handle id, or 0 — which the host resolves as undefined — when
    /// the ivar is missing or not a non-negative Fixnum. Unboxed rather than
    /// round-tripped through a string, which would truncate above
    /// `i32::MAX`.
    pub fn extract_handle_id(&self, handle_val: Value) -> u32 {
        use beni::{FromValue, Object, RObject};
        let Some(id) = RObject::from_value(handle_val)
            .and_then(|handle| handle.ivar_get::<_, Value>(self.mrb(), HANDLE_ID_IVAR).ok())
            .and_then(i32::from_value)
        else {
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
        let nil = beni::value::qnil().as_value();
        if id > HANDLE_ID_MAX {
            return nil;
        }
        let mrb = self.mrb();
        self.registrations
            .handle_class
            .new_instance(mrb, &[(id as i32).into_value(mrb)])
            .map(|handle| handle.freeze(mrb))
            .unwrap_or(nil)
    }

    /// Represent `n` as an mruby `Integer`, refusing anything the MRB_INT32
    /// build cannot hold rather than saturating it — neither side may ever
    /// see a different number than the wire carried.
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
