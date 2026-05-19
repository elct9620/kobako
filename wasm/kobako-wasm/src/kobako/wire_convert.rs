//! Wire ↔ mrb_value conversion methods on [`super::Kobako`].
//!
//! Mirrors the host-side `lib/kobako/wire/envelope/` split: keep the
//! `Kobako` façade lean and house the codec-adjacent translation
//! in its own sibling file. The methods stay on `Kobako` (so call
//! sites read `kobako.to_wire_value(val)` rather than
//! `wire_convert::to_wire_value(&kobako, val)`) via a second `impl`
//! block.
//!
//! Three concerns live here:
//!
//! 1. **RPC-arg conversion** (`to_wire_value`) — unknown types fall
//!    back to `Object#to_s`, the interchange representation.
//! 2. **Outcome conversion** (`to_wire_outcome`) — unknown types fall
//!    back to `Object#inspect`, the display representation. The
//!    inspect call is protected by `mrb_protect_error` (see H-3 in
//!    docs/spec-rubrics-todo.md) so a user-defined `inspect` that
//!    raises cannot longjmp past the Rust frame.
//! 3. **Wire-arg unpacking** (`extract_hash_kwargs` /
//!    `unpack_args_kwargs`) — used by the `method_missing` C bridges
//!    to split a `mrb_get_args` "n*" rest slice into positional args
//!    and trailing-Hash kwargs.

use super::Kobako;
#[cfg(target_arch = "wasm32")]
use crate::cstr;
use crate::mruby::sys;

/// `mrb_protect_error` body that re-enters mruby to compute
/// `Object#inspect` on the value pointed to by `userdata`. Used by
/// [`Kobako::protected_inspect_or_classname`] so a user-defined
/// `inspect` that raises lands on `*error == TRUE` instead of
/// longjmp-ing past the Rust frame holding the wire conversion.
///
/// # Safety
///
/// `userdata` must point to a live `mrb_value` produced by the same
/// VM as `mrb`. The function reads exactly one `mrb_value` from
/// `userdata` and does not retain the pointer after return.
#[cfg(target_arch = "wasm32")]
unsafe extern "C" fn protected_inspect_body(
    mrb: *mut sys::mrb_state,
    userdata: *mut core::ffi::c_void,
) -> sys::mrb_value {
    // SAFETY: see item-level doc.
    let val = unsafe { *(userdata as *const sys::mrb_value) };
    // SAFETY: `mrb` is live by `mrb_protect_error`'s contract; `val`
    // came from the same VM by the function-level safety contract.
    unsafe { val.call(mrb, cstr!("inspect"), &[]) }
}

impl Kobako {
    /// Decode every key/value pair from an mruby Hash into `out` as
    /// `(String, codec::Value)` pairs. The outer `String` carries the
    /// key's name; [`crate::rpc::envelope::encode_request`] re-emits each name
    /// as a wire-level `Value::Sym` (ext 0x00) per docs/wire-codec.md
    /// § Ext Types. Keys arriving as either mruby `Symbol` or `String`
    /// reduce to the same UTF-8 name via `Object#to_s`. Values go
    /// through [`Kobako::to_wire_value`].
    #[cfg(target_arch = "wasm32")]
    pub fn extract_hash_kwargs(
        &self,
        hash: sys::mrb_value,
        out: &mut Vec<(String, crate::codec::Value)>,
    ) {
        let keys_ary = self.hash_keys(hash);
        let keys_len = self.collection_len(keys_ary);
        for i in 0..keys_len {
            let key_val = self.ary_entry(keys_ary, i as i32);
            let val = self.hash_get(hash, key_val);
            out.push((self.to_string_of(key_val), self.to_wire_value(val)));
        }
    }

    /// Split a `rest` slice (from `mrb_get_args` `"n*"`) into positional
    /// wire args and keyword wire kwargs. The last element is absorbed
    /// into kwargs when it is a Hash; all other elements become
    /// positional args.
    #[cfg(target_arch = "wasm32")]
    pub fn unpack_args_kwargs(
        &self,
        rest: &[sys::mrb_value],
    ) -> (Vec<crate::codec::Value>, Vec<(String, crate::codec::Value)>) {
        let mut wire_args: Vec<crate::codec::Value> = Vec::new();
        let mut wire_kwargs: Vec<(String, crate::codec::Value)> = Vec::new();

        for (idx, &mrb_val) in rest.iter().enumerate() {
            let is_hash = self.classname_of(mrb_val) == "Hash" && idx == rest.len() - 1;
            if is_hash {
                self.extract_hash_kwargs(mrb_val, &mut wire_kwargs);
            } else {
                wire_args.push(self.to_wire_value(mrb_val));
            }
        }

        (wire_args, wire_kwargs)
    }

    /// Iterate an mruby Array and convert each element via `convert`,
    /// returning a `Vec<Value>` ready to wrap in [`Value::Array`].
    /// `convert` is a function pointer so the two consumer converters
    /// ([`Kobako::to_wire_value`] and [`Kobako::to_wire_outcome`]) can
    /// share the iteration while preserving their per-converter
    /// recursion target — the outcome path must keep recursing on
    /// `to_wire_outcome` so unknown nested types fall back to
    /// `inspect`, not `to_s`.
    #[cfg(target_arch = "wasm32")]
    fn array_to_wire(
        &self,
        val: sys::mrb_value,
        convert: fn(&Self, sys::mrb_value) -> crate::codec::Value,
    ) -> Vec<crate::codec::Value> {
        let len = self.collection_len(val);
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            let elem = self.ary_entry(val, i as i32);
            items.push(convert(self, elem));
        }
        items
    }

    /// Iterate an mruby Hash and convert each key/value pair via
    /// `convert`, returning a `Vec<(Value, Value)>` ready to wrap in
    /// [`Value::Map`]. Both the key and the value flow through the
    /// same `convert` so a `Symbol` key arrives as [`Value::Sym`]
    /// (ext 0x00) and a `String` key as [`Value::Str`] — distinct on
    /// the wire per docs/wire-codec.md § Ext Types.
    #[cfg(target_arch = "wasm32")]
    fn hash_to_wire(
        &self,
        val: sys::mrb_value,
        convert: fn(&Self, sys::mrb_value) -> crate::codec::Value,
    ) -> Vec<(crate::codec::Value, crate::codec::Value)> {
        let keys_ary = self.hash_keys(val);
        let len = self.collection_len(keys_ary);
        let mut pairs = Vec::with_capacity(len);
        for i in 0..len {
            let key = self.ary_entry(keys_ary, i as i32);
            let v = self.hash_get(val, key);
            pairs.push((convert(self, key), convert(self, v)));
        }
        pairs
    }

    /// Convert an `mrb_value` to a kobako wire [`crate::codec::Value`]
    /// for use as an RPC argument or keyword value. Symbol values map to
    /// [`Value::Sym`] (ext 0x00, docs/wire-codec.md § Ext Types).
    /// Array / Hash values map to [`Value::Array`] / [`Value::Map`]
    /// recursively (docs/wire-codec.md § Type Mapping #7-#8). Unknown
    /// types fall back to `Object#to_s`.
    ///
    /// ## Why two converters
    ///
    /// This is the **RPC-path** converter. Hash arguments are still
    /// decoded into kwargs separately via [`Kobako::extract_hash_kwargs`]
    /// when they trail the positional list; a Hash that arrives here is
    /// either nested inside an Array argument or sitting in a non-final
    /// positional slot, and travels natively as [`Value::Map`]. The
    /// sibling [`Kobako::to_wire_outcome`] handles the **outcome-path**
    /// (the script's last-expression value) and uses `inspect` for its
    /// unknown-type fallback instead. Do not unify the two: the outcome
    /// path is read as a display representation, while RPC arguments
    /// are interchange values that reach a Service's `public_send`.
    #[cfg(target_arch = "wasm32")]
    pub fn to_wire_value(&self, val: sys::mrb_value) -> crate::codec::Value {
        use crate::codec::Value;
        match self.classname_of(val) {
            "NilClass" => Value::Nil,
            "TrueClass" => Value::Bool(true),
            "FalseClass" => Value::Bool(false),
            "Integer" => Value::Int(self.unbox_integer(val) as i64),
            "Float" => Value::Float(self.unbox_float(val)),
            "String" => Value::Str(self.to_string_of(val)),
            "Symbol" => Value::Sym(self.to_string_of(val)),
            "Array" => Value::Array(self.array_to_wire(val, Self::to_wire_value)),
            "Hash" => Value::Map(self.hash_to_wire(val, Self::to_wire_value)),
            // Fallback: route through `.to_s`.
            _ => Value::Str(self.to_string_of(val)),
        }
    }

    /// Convert an `mrb_value` to a kobako wire [`crate::codec::Value`]
    /// for inclusion in the outcome Result envelope. Used by
    /// `__kobako_eval` to serialize the user script's last-expression
    /// value. Array / Hash values map to [`Value::Array`] /
    /// [`Value::Map`] recursively (docs/wire-codec.md § Type Mapping #7-#8)
    /// so a script returning a collection retains element-level fidelity.
    ///
    /// ## Why this differs from [`Kobako::to_wire_value`]
    ///
    /// Unknown types fall back to `Object#inspect` rather than
    /// `Object#to_s`. The outcome envelope is read by host-side
    /// callers as a *display* representation, not an interchange
    /// value, so `inspect` (which quotes strings, shows class names)
    /// is the right shape. Nested values inside an Array or Hash also
    /// flow through `inspect` for unknown types — the recursive call
    /// lands back in this same arm.
    #[cfg(target_arch = "wasm32")]
    pub fn to_wire_outcome(&self, val: sys::mrb_value) -> crate::codec::Value {
        use crate::codec::Value;
        match self.classname_of(val) {
            "NilClass" => Value::Nil,
            "TrueClass" => Value::Bool(true),
            "FalseClass" => Value::Bool(false),
            "Integer" => Value::Int(self.unbox_integer(val) as i64),
            "Float" => Value::Float(self.unbox_float(val)),
            "String" => Value::Str(self.to_string_of(val)),
            "Symbol" => Value::Sym(self.to_string_of(val)),
            "Array" => Value::Array(self.array_to_wire(val, Self::to_wire_outcome)),
            "Hash" => Value::Map(self.hash_to_wire(val, Self::to_wire_outcome)),
            other => Value::Str(self.protected_inspect_or_classname(val, other)),
        }
    }

    /// Coerce `val` to its `Object#inspect` form for the outcome
    /// envelope, protected by `mrb_protect_error` so a user-defined
    /// `inspect` that raises does not longjmp past Rust frames.
    ///
    /// docs/wire-contract.md § Outcome Envelope leaves the fallback
    /// string format to the host; we surface `inspect` on success and
    /// `"#<ClassName>"` on the protected-call failure path so the host
    /// sees a recognisable identifier rather than the raised exception.
    #[cfg(target_arch = "wasm32")]
    fn protected_inspect_or_classname(&self, val: sys::mrb_value, class_name: &str) -> String {
        let mut payload = val;
        let mut error: sys::mrb_bool = 0;
        // SAFETY: bridge frame — `self.mrb` is live; the body closure
        // reads `payload` once via the `userdata` pointer and never
        // outlives the call frame.
        let result = unsafe {
            sys::mrb_protect_error(
                self.mrb,
                protected_inspect_body,
                &mut payload as *mut sys::mrb_value as *mut core::ffi::c_void,
                &mut error,
            )
        };
        if error != 0 {
            format!("#<{}>", class_name)
        } else {
            self.to_string_of(result)
        }
    }

    /// Convert a kobako wire [`crate::codec::Value`] into an `mrb_value`
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::RPC::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host through
    /// `Kobako::RPC::Handle#method_missing` → [`Kobako::dispatch_invoke`],
    /// docs/behavior.md B-17).
    #[cfg(target_arch = "wasm32")]
    pub fn to_mrb_value(&self, val: crate::codec::Value) -> sys::mrb_value {
        use crate::codec::Value;
        // SAFETY: `self.mrb` is live; cached class refs were produced by
        // `install_raw` / `resolve_raw`.
        unsafe {
            match val {
                Value::Nil => self.nil_value(),
                Value::Bool(b) => {
                    if b {
                        self.true_value()
                    } else {
                        self.false_value()
                    }
                }
                Value::Int(n) => {
                    // mrb_int on wasm32 is 32-bit (MRB_INT32); clamp to i32.
                    let n32 = n.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    sys::mrb_boxing_int_value(self.mrb, n32)
                }
                Value::UInt(n) => {
                    let n32 = n.min(i32::MAX as u64) as i32;
                    sys::mrb_boxing_int_value(self.mrb, n32)
                }
                Value::Float(f) => sys::mrb_word_boxing_float_value(self.mrb, f),
                Value::Str(s) => match std::ffi::CString::new(s.as_str()) {
                    Ok(cs) => sys::mrb_str_new_cstr(self.mrb, cs.as_ptr()),
                    Err(_) => sys::mrb_str_new(
                        self.mrb,
                        s.as_ptr() as *const core::ffi::c_char,
                        s.len() as i32,
                    ),
                },
                Value::Handle(id) => {
                    let id_val = sys::mrb_boxing_int_value(self.mrb, id as i32);
                    sys::mrb_obj_new(
                        self.mrb,
                        self.handle_class,
                        1,
                        &id_val as *const sys::mrb_value,
                    )
                }
                Value::Bin(bytes) => sys::mrb_str_new(
                    self.mrb,
                    bytes.as_ptr() as *const core::ffi::c_char,
                    bytes.len() as i32,
                ),
                Value::Sym(name) => {
                    // Intern via String#to_sym — mruby's mrb_symbol_value
                    // bit-layout is build-private (we use
                    // MRB_WORDBOX_NO_INLINE_FLOAT) so we go through the VM.
                    let str_val = sys::mrb_str_new(
                        self.mrb,
                        name.as_ptr() as *const core::ffi::c_char,
                        name.len() as i32,
                    );
                    str_val.call(self.mrb, cstr!("to_sym"), &[])
                }
                Value::Array(items) => {
                    let ary = sys::mrb_ary_new(self.mrb);
                    for item in items {
                        let elem = self.to_mrb_value(item);
                        sys::mrb_ary_push(self.mrb, ary, elem);
                    }
                    ary
                }
                Value::Map(pairs) => {
                    let hash = sys::mrb_hash_new(self.mrb);
                    for (k, v) in pairs {
                        let key = self.to_mrb_value(k);
                        let val = self.to_mrb_value(v);
                        sys::mrb_hash_set(self.mrb, hash, key, val);
                    }
                    hash
                }
                // ext 0x02 envelopes are consumed by the exception path
                // (`raise_service_error`) before reaching value
                // conversion; the defensive nil here covers any
                // malformed Response that smuggles one through.
                Value::ErrEnv(_) => self.nil_value(),
            }
        }
    }
}
