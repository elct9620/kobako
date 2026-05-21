//! Wire ↔ Value conversion methods on [`super::Kobako`].
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
use crate::mruby::sys;
use crate::mruby::sys::Value;

/// `mrb_protect_error` body that re-enters mruby to compute
/// `Object#inspect` on the value pointed to by `userdata`. Used by
/// [`Kobako::protected_inspect_or_classname`] so a user-defined
/// `inspect` that raises lands on `*error == TRUE` instead of
/// longjmp-ing past the Rust frame holding the wire conversion.
///
/// Stays in raw `mrb_value` form because mruby's `mrb_protect_error`
/// expects a `mrb_value (*body)(mrb_state*, void*)`; the userdata
/// pointer aliases a raw `mrb_value` on the caller's stack.
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
    let val = Value::from_raw(unsafe { *(userdata as *const sys::mrb_value) });
    // SAFETY: `mrb` is live by `mrb_protect_error`'s contract.
    let mrb_ref = unsafe { crate::mruby::Mrb::borrow_raw(mrb) };
    val.call(mrb_ref, c"inspect", &[]).into_raw()
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
    pub fn extract_hash_kwargs(&self, hash: Value, out: &mut Vec<(String, crate::codec::Value)>) {
        let keys_ary = self.mrb().hash_keys(hash);
        let keys_len = self.collection_len(keys_ary);
        for i in 0..keys_len {
            // SAFETY: `keys_ary` is Array-tagged (mrb_hash_keys always
            // returns an Array); `i` stays in range by `keys_len`.
            let key_val = unsafe { keys_ary.ary_entry(i as i32) };
            let val = self.mrb().hash_get(hash, key_val);
            out.push((key_val.to_string(self.mrb()), self.to_wire_value(val)));
        }
    }

    /// Split a `rest` slice (from `mrb_get_args` `"n*"`) into positional
    /// wire args and keyword wire kwargs. The last element is absorbed
    /// into kwargs when it is a Hash; all other elements become
    /// positional args.
    ///
    /// `rest` is typed as `&[Value]` even though the underlying buffer
    /// came from mruby's variadic out-param; `Value` is
    /// `#[repr(transparent)]` over `mrb_value` so the slice layouts
    /// are identical (the bridge call site casts once).
    #[cfg(target_arch = "wasm32")]
    pub fn unpack_args_kwargs(
        &self,
        rest: &[Value],
    ) -> (Vec<crate::codec::Value>, Vec<(String, crate::codec::Value)>) {
        let mut wire_args: Vec<crate::codec::Value> = Vec::new();
        let mut wire_kwargs: Vec<(String, crate::codec::Value)> = Vec::new();

        for (idx, &mrb_val) in rest.iter().enumerate() {
            let is_hash = mrb_val.classname(self.mrb()) == "Hash" && idx == rest.len() - 1;
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
        val: Value,
        convert: fn(&Self, Value) -> crate::codec::Value,
    ) -> Vec<crate::codec::Value> {
        let len = self.collection_len(val);
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            // SAFETY: val is Array-tagged (callers gate by classname),
            // and originates from +self.mrb+.
            let elem = unsafe { val.ary_entry(i as i32) };
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
        val: Value,
        convert: fn(&Self, Value) -> crate::codec::Value,
    ) -> Vec<(crate::codec::Value, crate::codec::Value)> {
        let keys_ary = self.mrb().hash_keys(val);
        let len = self.collection_len(keys_ary);
        let mut pairs = Vec::with_capacity(len);
        for i in 0..len {
            // SAFETY: keys_ary is Array-tagged from hash_keys, val is
            // Hash-tagged from caller; `i` stays in range by `len`.
            let key = unsafe { keys_ary.ary_entry(i as i32) };
            let v = self.mrb().hash_get(val, key);
            pairs.push((convert(self, key), convert(self, v)));
        }
        pairs
    }

    /// Convert a [`Value`] to a kobako wire [`crate::codec::Value`]
    /// for use as an RPC argument or keyword value. Symbol values map to
    /// [`crate::codec::Value::Sym`] (ext 0x00, docs/wire-codec.md
    /// § Ext Types). Array / Hash values map to
    /// [`crate::codec::Value::Array`] / [`crate::codec::Value::Map`]
    /// recursively (docs/wire-codec.md § Type Mapping #7-#8). Unknown
    /// types fall back to `Object#to_s`.
    ///
    /// ## Why two converters
    ///
    /// This is the **RPC-path** converter. Hash arguments are still
    /// decoded into kwargs separately via [`Kobako::extract_hash_kwargs`]
    /// when they trail the positional list; a Hash that arrives here is
    /// either nested inside an Array argument or sitting in a non-final
    /// positional slot, and travels natively as
    /// [`crate::codec::Value::Map`]. The sibling
    /// [`Kobako::to_wire_outcome`] handles the **outcome-path** (the
    /// script's last-expression value) and uses `inspect` for its
    /// unknown-type fallback instead. Do not unify the two: the outcome
    /// path is read as a display representation, while RPC arguments
    /// are interchange values that reach a Service's `public_send`.
    #[cfg(target_arch = "wasm32")]
    pub fn to_wire_value(&self, val: Value) -> crate::codec::Value {
        use crate::codec::Value as WireValue;
        // SAFETY in this method: `unbox_integer` / `unbox_float` are
        // gated by their respective classname arms.
        match val.classname(self.mrb()) {
            "NilClass" => WireValue::Nil,
            "TrueClass" => WireValue::Bool(true),
            "FalseClass" => WireValue::Bool(false),
            "Integer" => WireValue::Int(unsafe { val.unbox_integer() } as i64),
            "Float" => WireValue::Float(unsafe { val.unbox_float() }),
            "String" => WireValue::Str(val.to_string(self.mrb())),
            "Symbol" => WireValue::Sym(val.to_string(self.mrb())),
            "Array" => WireValue::Array(self.array_to_wire(val, Self::to_wire_value)),
            "Hash" => WireValue::Map(self.hash_to_wire(val, Self::to_wire_value)),
            // Fallback: route through `.to_s`.
            _ => WireValue::Str(val.to_string(self.mrb())),
        }
    }

    /// Convert a [`Value`] to a kobako wire [`crate::codec::Value`]
    /// for inclusion in the outcome Result envelope. Used by
    /// `__kobako_eval` to serialize the user script's last-expression
    /// value. Array / Hash values map to
    /// [`crate::codec::Value::Array`] / [`crate::codec::Value::Map`]
    /// recursively (docs/wire-codec.md § Type Mapping #7-#8) so a
    /// script returning a collection retains element-level fidelity.
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
    pub fn to_wire_outcome(&self, val: Value) -> crate::codec::Value {
        use crate::codec::Value as WireValue;
        // SAFETY in this method: as `to_wire_value`.
        match val.classname(self.mrb()) {
            "NilClass" => WireValue::Nil,
            "TrueClass" => WireValue::Bool(true),
            "FalseClass" => WireValue::Bool(false),
            "Integer" => WireValue::Int(unsafe { val.unbox_integer() } as i64),
            "Float" => WireValue::Float(unsafe { val.unbox_float() }),
            "String" => WireValue::Str(val.to_string(self.mrb())),
            "Symbol" => WireValue::Sym(val.to_string(self.mrb())),
            "Array" => WireValue::Array(self.array_to_wire(val, Self::to_wire_outcome)),
            "Hash" => WireValue::Map(self.hash_to_wire(val, Self::to_wire_outcome)),
            other => WireValue::Str(self.protected_inspect_or_classname(val, other)),
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
    fn protected_inspect_or_classname(&self, val: Value, class_name: &str) -> String {
        // mrb_protect_error's body signature uses raw mrb_value (it's
        // a typedef on the mruby side), so the userdata pointer
        // aliases a raw mrb_value cell — drop into the raw form for
        // the duration of the call.
        let mut payload = val.as_raw();
        let mut error: sys::mrb_bool = 0;
        // SAFETY: bridge frame — `self.mrb` is live; the body closure
        // reads `payload` once via the `userdata` pointer and never
        // outlives the call frame.
        let result = Value::from_raw(unsafe {
            sys::mrb_protect_error(
                self.mrb,
                protected_inspect_body,
                &mut payload as *mut sys::mrb_value as *mut core::ffi::c_void,
                &mut error,
            )
        });
        if error != 0 {
            format!("#<{}>", class_name)
        } else {
            result.to_string(self.mrb())
        }
    }

    /// Convert a kobako wire [`crate::codec::Value`] into a [`Value`]
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::RPC::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host through
    /// `Kobako::RPC::Handle#method_missing` → [`Kobako::dispatch_invoke`],
    /// docs/behavior.md B-17).
    #[cfg(target_arch = "wasm32")]
    pub fn to_mrb_value(&self, val: crate::codec::Value) -> Value {
        use crate::codec::Value as WireValue;
        let mrb = self.mrb();
        match val {
            WireValue::Nil => Value::nil(),
            WireValue::Bool(b) => {
                if b {
                    Value::true_()
                } else {
                    Value::false_()
                }
            }
            WireValue::Int(n) => {
                // mrb_int on wasm32 is 32-bit (MRB_INT32); clamp to i32.
                let n32 = n.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                Value::from_int(mrb, n32)
            }
            WireValue::UInt(n) => {
                let n32 = n.min(i32::MAX as u64) as i32;
                Value::from_int(mrb, n32)
            }
            WireValue::Float(f) => Value::from_float(mrb, f),
            WireValue::Str(s) => match std::ffi::CString::new(s.as_str()) {
                Ok(cs) => mrb.str_new_cstr(&cs),
                Err(_) => mrb.str_new(s.as_bytes()),
            },
            WireValue::Handle(id) => {
                let id_val = Value::from_int(mrb, id as i32).as_raw();
                // SAFETY: `mrb` is live; `self.handle_class` was
                // produced by `install_raw` / `resolve_raw`; `id_val`
                // originates from the same VM.
                Value::from_raw(unsafe {
                    sys::mrb_obj_new(
                        mrb.as_ptr(),
                        self.handle_class.as_raw(),
                        1,
                        &id_val as *const sys::mrb_value,
                    )
                })
            }
            WireValue::Bin(bytes) => mrb.str_new(&bytes),
            WireValue::Sym(name) => {
                // Intern via String#to_sym — mruby's mrb_symbol_value
                // bit-layout is build-private (we use
                // MRB_WORDBOX_NO_INLINE_FLOAT) so we go through the VM.
                mrb.str_new(name.as_bytes()).call(mrb, c"to_sym", &[])
            }
            WireValue::Array(items) => {
                let ary = mrb.ary_new();
                for item in items {
                    let elem = self.to_mrb_value(item);
                    mrb.ary_push(ary, elem);
                }
                ary
            }
            WireValue::Map(pairs) => {
                let hash = mrb.hash_new();
                for (k, v) in pairs {
                    let key = self.to_mrb_value(k);
                    let val = self.to_mrb_value(v);
                    mrb.hash_set(hash, key, val);
                }
                hash
            }
            // ext 0x02 envelopes are consumed by the exception path
            // (`raise_service_error`) before reaching value
            // conversion; the defensive nil here covers any
            // malformed Response that smuggles one through.
            WireValue::ErrEnv(_) => Value::nil(),
        }
    }
}
