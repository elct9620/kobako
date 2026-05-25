//! Codec ↔ mruby Value conversion methods on [`super::Kobako`].
//!
//! Keep the [`Kobako`] façade lean by housing the codec-adjacent
//! translation in its own sibling file. The methods stay on `Kobako`
//! (so call sites read `kobako.to_codec_value(val)` rather than
//! `codec_convert::to_codec_value(&kobako, val)`) via a second `impl`
//! block.
//!
//! Three concerns live here:
//!
//! 1. **Transport-arg conversion** (`to_codec_value`) — unknown types fall
//!    back to `Object#to_s`, the interchange representation.
//! 2. **Outcome conversion** (`to_codec_outcome`) — unknown types fall
//!    back to `Object#inspect`, the display representation. The
//!    inspect call is protected by `mrb_protect_error` (see H-3 in
//!    docs/spec-rubrics-todo.md) so a user-defined `inspect` that
//!    raises cannot longjmp past the Rust frame.
//! 3. **Args / kwargs unpacking** (`extract_hash_kwargs` /
//!    `unpack_args_kwargs`) — used by the `method_missing` C bridges
//!    to split a `mrb_get_args` "n*" rest slice into positional args
//!    and trailing-Hash kwargs.

use super::Kobako;
use crate::mruby::sys;
use crate::mruby::sys::Value;

impl Kobako {
    /// Decode every key/value pair from an mruby Hash into `out` as
    /// `(String, codec::Value)` pairs. The outer `String` carries the
    /// key's name; [`crate::transport::envelope::encode_request`] re-emits each
    /// name as a `Value::Sym` (ext 0x00) per docs/wire-codec.md § Ext
    /// Types. Keys arriving as either mruby `Symbol` or `String` reduce
    /// to the same UTF-8 name via `Object#to_s`. Values go through
    /// [`Kobako::to_codec_value`].
    #[cfg(target_arch = "wasm32")]
    pub fn extract_hash_kwargs(&self, hash: Value, out: &mut Vec<(String, crate::codec::Value)>) {
        // SAFETY: callers reach this only after a `classname == "Hash"`
        // gate, so the unchecked wrap is sound.
        let hash = unsafe { sys::Hash::from_value_unchecked(hash) };
        let keys_ary = hash.keys(self.mrb());
        let keys_len = self.collection_len(keys_ary.as_value());
        for i in 0..keys_len {
            let key_val = keys_ary.entry(i as i32);
            let val = hash.get(self.mrb(), key_val);
            out.push((key_val.to_string(self.mrb()), self.to_codec_value(val)));
        }
    }

    /// Split a `rest` slice (from `mrb_get_args` `"n*"`) into positional
    /// args and keyword kwargs. The last element is absorbed into
    /// kwargs when it is a Hash; all other elements become positional
    /// args.
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
        let mut args: Vec<crate::codec::Value> = Vec::new();
        let mut kwargs: Vec<(String, crate::codec::Value)> = Vec::new();

        for (idx, &mrb_val) in rest.iter().enumerate() {
            let is_hash = mrb_val.classname(self.mrb()) == "Hash" && idx == rest.len() - 1;
            if is_hash {
                self.extract_hash_kwargs(mrb_val, &mut kwargs);
            } else {
                args.push(self.to_codec_value(mrb_val));
            }
        }

        (args, kwargs)
    }

    /// Iterate an mruby Array and convert each element via `convert`,
    /// returning a `Vec<Value>` ready to wrap in [`Value::Array`].
    /// `convert` is a function pointer so the two consumer converters
    /// ([`Kobako::to_codec_value`] and [`Kobako::to_codec_outcome`]) can
    /// share the iteration while preserving their per-converter
    /// recursion target — the outcome path must keep recursing on
    /// `to_codec_outcome` so unknown nested types fall back to
    /// `inspect`, not `to_s`.
    #[cfg(target_arch = "wasm32")]
    fn array_to_codec(
        &self,
        val: Value,
        convert: fn(&Self, Value) -> crate::codec::Value,
    ) -> Vec<crate::codec::Value> {
        // SAFETY: callers reach this only after a `classname == "Array"`
        // gate, so the unchecked wrap is sound.
        let ary = unsafe { sys::Array::from_value_unchecked(val) };
        let len = self.collection_len(val);
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            let elem = ary.entry(i as i32);
            items.push(convert(self, elem));
        }
        items
    }

    /// Iterate an mruby Hash and convert each key/value pair via
    /// `convert`, returning a `Vec<(Value, Value)>` ready to wrap in
    /// [`Value::Map`]. Both the key and the value flow through the
    /// same `convert` so a `Symbol` key arrives as [`Value::Sym`]
    /// (ext 0x00) and a `String` key as [`Value::Str`] — distinct codec
    /// encodings per docs/wire-codec.md § Ext Types.
    #[cfg(target_arch = "wasm32")]
    fn hash_to_codec(
        &self,
        val: Value,
        convert: fn(&Self, Value) -> crate::codec::Value,
    ) -> Vec<(crate::codec::Value, crate::codec::Value)> {
        // SAFETY: callers reach this only after a `classname == "Hash"`
        // gate, so the unchecked wrap is sound.
        let hash = unsafe { sys::Hash::from_value_unchecked(val) };
        let keys_ary = hash.keys(self.mrb());
        let len = self.collection_len(keys_ary.as_value());
        let mut pairs = Vec::with_capacity(len);
        for i in 0..len {
            let key = keys_ary.entry(i as i32);
            let v = hash.get(self.mrb(), key);
            pairs.push((convert(self, key), convert(self, v)));
        }
        pairs
    }

    /// Convert a [`Value`] to a kobako [`crate::codec::Value`] for use
    /// as a transport argument or keyword value. Symbol values map to
    /// [`crate::codec::Value::Sym`] (ext 0x00, docs/wire-codec.md
    /// § Ext Types). Array / Hash values map to
    /// [`crate::codec::Value::Array`] / [`crate::codec::Value::Map`]
    /// recursively (docs/wire-codec.md § Type Mapping #7-#8). Unknown
    /// types fall back to `Object#to_s`.
    ///
    /// ## Why two converters
    ///
    /// This is the **transport-path** converter. Hash arguments are still
    /// decoded into kwargs separately via [`Kobako::extract_hash_kwargs`]
    /// when they trail the positional list; a Hash that arrives here is
    /// either nested inside an Array argument or sitting in a non-final
    /// positional slot, and travels natively as
    /// [`crate::codec::Value::Map`]. The sibling
    /// [`Kobako::to_codec_outcome`] handles the **outcome-path** (the
    /// script's last-expression value) and uses `inspect` for its
    /// unknown-type fallback instead. Do not unify the two: the outcome
    /// path is read as a display representation, while transport arguments
    /// are interchange values that reach a Service's `public_send`.
    #[cfg(target_arch = "wasm32")]
    pub fn to_codec_value(&self, val: Value) -> crate::codec::Value {
        use crate::codec::Value as CodecValue;
        // Direct-unbox primitives dispatch on mruby's own type tag
        // (`mrb_type`, via `is_integer` / `is_float`) so the `unsafe`
        // unbox precondition is established by the guard itself rather
        // than inferred from a classname-string match.
        if val.is_integer() {
            // SAFETY: `is_integer` confirmed MRB_TT_INTEGER tagging.
            return CodecValue::Int(unsafe { val.unbox_integer() } as i64);
        }
        if val.is_float() {
            // SAFETY: `is_float` confirmed MRB_TT_FLOAT tagging.
            return CodecValue::Float(unsafe { val.unbox_float() });
        }
        match val.classname(self.mrb()) {
            "NilClass" => CodecValue::Nil,
            "TrueClass" => CodecValue::Bool(true),
            "FalseClass" => CodecValue::Bool(false),
            "String" => CodecValue::Str(val.to_string(self.mrb())),
            "Symbol" => CodecValue::Sym(val.to_string(self.mrb())),
            "Array" => CodecValue::Array(self.array_to_codec(val, Self::to_codec_value)),
            "Hash" => CodecValue::Map(self.hash_to_codec(val, Self::to_codec_value)),
            // Fallback: route through `.to_s`.
            _ => CodecValue::Str(val.to_string(self.mrb())),
        }
    }

    /// Convert a [`Value`] to a kobako [`crate::codec::Value`] for
    /// inclusion in the outcome Result envelope. Used by
    /// `__kobako_eval` to serialize the user script's last-expression
    /// value. Array / Hash values map to
    /// [`crate::codec::Value::Array`] / [`crate::codec::Value::Map`]
    /// recursively (docs/wire-codec.md § Type Mapping #7-#8) so a
    /// script returning a collection retains element-level fidelity.
    ///
    /// ## Why this differs from [`Kobako::to_codec_value`]
    ///
    /// Unknown types fall back to `Object#inspect` rather than
    /// `Object#to_s`. The outcome envelope is read by host-side
    /// callers as a *display* representation, not an interchange
    /// value, so `inspect` (which quotes strings, shows class names)
    /// is the right shape. Nested values inside an Array or Hash also
    /// flow through `inspect` for unknown types — the recursive call
    /// lands back in this same arm.
    #[cfg(target_arch = "wasm32")]
    pub fn to_codec_outcome(&self, val: Value) -> crate::codec::Value {
        use crate::codec::Value as CodecValue;
        // Tag-predicate gate for the direct-unbox primitives, as in
        // `to_codec_value`.
        if val.is_integer() {
            // SAFETY: `is_integer` confirmed MRB_TT_INTEGER tagging.
            return CodecValue::Int(unsafe { val.unbox_integer() } as i64);
        }
        if val.is_float() {
            // SAFETY: `is_float` confirmed MRB_TT_FLOAT tagging.
            return CodecValue::Float(unsafe { val.unbox_float() });
        }
        match val.classname(self.mrb()) {
            "NilClass" => CodecValue::Nil,
            "TrueClass" => CodecValue::Bool(true),
            "FalseClass" => CodecValue::Bool(false),
            "String" => CodecValue::Str(val.to_string(self.mrb())),
            "Symbol" => CodecValue::Sym(val.to_string(self.mrb())),
            "Array" => CodecValue::Array(self.array_to_codec(val, Self::to_codec_outcome)),
            "Hash" => CodecValue::Map(self.hash_to_codec(val, Self::to_codec_outcome)),
            other => CodecValue::Str(self.protected_inspect_or_classname(val, other)),
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
        // [`Mrb::protect`] catches any exception `inspect` might raise
        // (user-defined `inspect` overriding the default) and surfaces
        // it as `Err` instead of long-jumping past the Rust frame.
        // docs/wire-contract.md § Outcome Envelope leaves the
        // fallback string format to the host — `"#<ClassName>"` is a
        // recognisable identifier when the raise path fires.
        match self.mrb().protect(|mrb| val.call(mrb, c"inspect", &[])) {
            Ok(result) => result.to_string(self.mrb()),
            Err(_) => format!("#<{}>", class_name),
        }
    }

    /// Convert a kobako [`crate::codec::Value`] into a [`Value`]
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host through
    /// `Kobako::Handle`'s instance-level `method_missing` and the bridge's
    /// `forward_to_dispatch` round-trip, docs/behavior.md B-17).
    #[cfg(target_arch = "wasm32")]
    pub fn to_mrb_value(&self, val: crate::codec::Value) -> Value {
        use crate::codec::Value as CodecValue;
        let mrb = self.mrb();
        match val {
            CodecValue::Nil => Value::nil(),
            CodecValue::Bool(b) => {
                if b {
                    Value::true_()
                } else {
                    Value::false_()
                }
            }
            CodecValue::Int(n) => {
                // mrb_int on wasm32 is 32-bit (MRB_INT32); clamp to i32.
                let n32 = n.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                Value::from_int(mrb, n32)
            }
            CodecValue::UInt(n) => {
                let n32 = n.min(i32::MAX as u64) as i32;
                Value::from_int(mrb, n32)
            }
            CodecValue::Float(f) => Value::from_float(mrb, f),
            CodecValue::Str(s) => match std::ffi::CString::new(s.as_str()) {
                Ok(cs) => mrb.str_new_cstr(&cs),
                Err(_) => mrb.str_new(s.as_bytes()),
            },
            CodecValue::Handle(id) => self
                .handle_class
                .obj_new(mrb, &[Value::from_int(mrb, id as i32)]),
            CodecValue::Bin(bytes) => mrb.str_new(&bytes),
            CodecValue::Sym(name) => {
                // Intern via String#to_sym — mruby's mrb_symbol_value
                // bit-layout is build-private (we use
                // MRB_WORDBOX_NO_INLINE_FLOAT) so we go through the VM.
                mrb.str_new(name.as_bytes()).call(mrb, c"to_sym", &[])
            }
            CodecValue::Array(items) => {
                let ary = mrb.ary_new();
                for item in items {
                    let elem = self.to_mrb_value(item);
                    ary.push(mrb, elem);
                }
                ary.as_value()
            }
            CodecValue::Map(pairs) => {
                let hash = mrb.hash_new();
                for (k, v) in pairs {
                    let key = self.to_mrb_value(k);
                    let val = self.to_mrb_value(v);
                    hash.set(mrb, key, val);
                }
                hash.as_value()
            }
            // ext 0x02 envelopes are consumed by the exception path
            // (`raise_service_error`) before reaching value
            // conversion; the defensive nil here covers any
            // malformed Response that smuggles one through.
            CodecValue::ErrEnv(_) => Value::nil(),
        }
    }
}
