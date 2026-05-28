//! Codec ↔ mruby Value conversion methods on `super::Kobako`.
//!
//! Keep the `Kobako` façade lean by housing the codec-adjacent
//! translation in its own sibling file. The methods stay on `Kobako`
//! (so call sites read `kobako.to_codec_value(val)` rather than
//! `codec_convert::to_codec_value(&kobako, val)`) via a second `impl`
//! block.
//!
//! Three concerns live here:
//!
//! 1. **Transport-arg conversion** (`to_codec_value`) — the guest→host
//!    Request args / kwargs path; unknown types fall back to
//!    `Object#to_s`, the interchange representation a Service's
//!    `public_send` receives.
//! 2. **Return conversion** (`try_codec_value`) — the `#eval` / `#run`
//!    outcome and the yield-block result; returns `None` for a value
//!    with no wire representation so the caller emits a Panic envelope
//!    (outcome, docs/behavior.md E-06) or a `0x04` error YieldResponse
//!    (yield, docs/behavior.md E-22). A return value is never coerced
//!    through an implicit `to_s` / `inspect` — SPEC.md § Implementation
//!    Standards pins "objects without a wire representation take the
//!    Panic envelope path — no implicit inspect or to_h conversion".
//! 3. **Args / kwargs unpacking** (`extract_hash_kwargs` /
//!    `unpack_args_kwargs`) — used by the `method_missing` C bridges
//!    to split a `mrb_get_args` "n*" rest slice into positional args
//!    and trailing-Hash kwargs.

use super::Kobako;
use crate::mruby::Value;

impl Kobako {
    /// Decode every key/value pair from an mruby Hash into `out` as
    /// `(String, codec::Value)` pairs. The outer `String` carries the
    /// key's name; `Request`'s `crate::codec::Encode` impl re-emits
    /// each name as a `Value::Sym` (ext 0x00) per docs/wire-codec.md § Ext
    /// Types. Keys arriving as either mruby `Symbol` or `String` reduce
    /// to the same UTF-8 name via `Object#to_s`. Values go through
    /// `Kobako::to_codec_value`.
    #[cfg(target_arch = "wasm32")]
    pub fn extract_hash_kwargs(&self, hash: Value, out: &mut Vec<(String, crate::codec::Value)>) {
        // SAFETY: callers reach this only after a `classname == "Hash"`
        // gate, so the unchecked wrap is sound.
        let hash = unsafe { crate::mruby::Hash::from_value_unchecked(hash) };
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
    /// returning a `Vec<R>` ready to wrap in `Value::Array`. `convert`
    /// is a function pointer generic over its output so the two consumer
    /// converters share the iteration while preserving their per-converter
    /// recursion target: `to_codec_value` recurses with
    /// `R = crate::codec::Value` (each element `to_s`-coerced), while
    /// `try_codec_value` recurses with `R = Option<crate::codec::Value>`
    /// so a single unrepresentable element collapses the whole Array to
    /// `None`.
    #[cfg(target_arch = "wasm32")]
    fn array_to_codec<R>(&self, val: Value, convert: fn(&Self, Value) -> R) -> Vec<R> {
        // SAFETY: callers reach this only after a `classname == "Array"`
        // gate, so the unchecked wrap is sound.
        let ary = unsafe { crate::mruby::Array::from_value_unchecked(val) };
        let len = self.collection_len(val);
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            let elem = ary.entry(i as i32);
            items.push(convert(self, elem));
        }
        items
    }

    /// Iterate an mruby Hash and convert each key/value pair via
    /// `convert`, returning a `Vec<(R, R)>` ready to wrap in
    /// `Value::Map`. Both the key and the value flow through the
    /// same `convert` so a `Symbol` key arrives as `Value::Sym`
    /// (ext 0x00) and a `String` key as `Value::Str` — distinct codec
    /// encodings per docs/wire-codec.md § Ext Types. Generic over `R`
    /// for the same reason as `array_to_codec`.
    #[cfg(target_arch = "wasm32")]
    fn hash_to_codec<R>(&self, val: Value, convert: fn(&Self, Value) -> R) -> Vec<(R, R)> {
        // SAFETY: callers reach this only after a `classname == "Hash"`
        // gate, so the unchecked wrap is sound.
        let hash = unsafe { crate::mruby::Hash::from_value_unchecked(val) };
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

    /// Convert a `Value` to a kobako `crate::codec::Value` for use
    /// as a transport argument or keyword value. Symbol values map to
    /// `crate::codec::Value::Sym` (ext 0x00, docs/wire-codec.md
    /// § Ext Types). Array / Hash values map to
    /// `crate::codec::Value::Array` / `crate::codec::Value::Map`
    /// recursively (docs/wire-codec.md § Type Mapping #7-#8). Unknown
    /// types fall back to `Object#to_s`.
    ///
    /// ## Why two converters
    ///
    /// This is the **transport-arg** converter. Hash arguments are still
    /// decoded into kwargs separately via `Kobako::extract_hash_kwargs`
    /// when they trail the positional list; a Hash that arrives here is
    /// either nested inside an Array argument or sitting in a non-final
    /// positional slot, and travels natively as
    /// `crate::codec::Value::Map`. The sibling `Kobako::try_codec_value`
    /// handles the **return path** (the `#eval` / `#run` outcome and the
    /// yield-block result) and returns `None` for an unrepresentable
    /// value instead of coercing it. Do not unify the two: an argument
    /// the guest hands to a Service tolerates a best-effort `to_s`, but a
    /// return value with no wire representation must fail loudly so the
    /// host raises rather than receive a misleading String.
    #[cfg(target_arch = "wasm32")]
    pub fn to_codec_value(&self, val: Value) -> crate::codec::Value {
        use crate::codec::Value as CodecValue;
        use crate::mruby::FromValue;
        // Scalar leaves dispatch on mruby's own type tag through the safe
        // `FromValue` downcast (which folds the `mrb_type` guard into the
        // unbox) rather than a classname-string match.
        if let Some(n) = i32::from_value(val) {
            return CodecValue::Int(n as i64);
        }
        if let Some(f) = f64::from_value(val) {
            return CodecValue::Float(f);
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

    /// Convert a `Value` to a kobako `crate::codec::Value` for a guest
    /// **return** path — the `#eval` / `#run` outcome Result envelope and
    /// the yield-block result. Array / Hash values map to
    /// `crate::codec::Value::Array` / `crate::codec::Value::Map`
    /// recursively (docs/wire-codec.md § Type Mapping #7-#8) so a return
    /// of a collection retains element-level fidelity.
    ///
    /// A `Kobako::Handle` proxy the guest holds (a Service return per
    /// B-14, or a `#run` argument auto-wrap per B-34) re-emits as an
    /// `ext 0x01` Capability Handle carrying its id, so the host restores
    /// it to its original object on every guest→host value path
    /// (docs/behavior.md B-37) — the invocation result and the yield-block
    /// result alike.
    ///
    /// Returns `None` when `val` has no wire representation (any type
    /// outside the 12-entry wire set, or a collection containing such a
    /// value). The return contract forbids an implicit `to_s` / `inspect`
    /// coercion, so the caller turns `None` into a Panic envelope
    /// (outcome, docs/behavior.md E-06 / B-06) or a `0x04` error
    /// YieldResponse (yield, docs/behavior.md E-22) rather than handing
    /// the host a misleading String.
    #[cfg(target_arch = "wasm32")]
    pub fn try_codec_value(&self, val: Value) -> Option<crate::codec::Value> {
        use crate::codec::Value as CodecValue;
        use crate::mruby::FromValue;
        // Scalar-leaf downcast through the safe `FromValue` seam, as in
        // `to_codec_value`.
        if let Some(n) = i32::from_value(val) {
            return Some(CodecValue::Int(n as i64));
        }
        if let Some(f) = f64::from_value(val) {
            return Some(CodecValue::Float(f));
        }
        match val.classname(self.mrb()) {
            "NilClass" => Some(CodecValue::Nil),
            "TrueClass" => Some(CodecValue::Bool(true)),
            "FalseClass" => Some(CodecValue::Bool(false)),
            "String" => Some(CodecValue::Str(val.to_string(self.mrb()))),
            "Symbol" => Some(CodecValue::Sym(val.to_string(self.mrb()))),
            // A Capability Handle the guest received earlier this
            // invocation is wire-representable: re-emit it as ext 0x01 so
            // the host restores the original object (docs/behavior.md
            // B-37). id 0 means a missing or forged ivar — treat as
            // unrepresentable rather than emit a wire-violation Handle.
            "Kobako::Handle" => match self.extract_handle_id(val) {
                0 => None,
                id => Some(CodecValue::Handle(id)),
            },
            // A single unrepresentable element collapses the whole
            // collection to `None` — `collect::<Option<Vec<_>>>()`
            // short-circuits on the first `None`.
            "Array" => self
                .array_to_codec(val, Self::try_codec_value)
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(CodecValue::Array),
            "Hash" => self
                .hash_to_codec(val, Self::try_codec_value)
                .into_iter()
                .map(|(k, v)| k.zip(v))
                .collect::<Option<Vec<_>>>()
                .map(CodecValue::Map),
            _ => None,
        }
    }

    /// Convert a kobako `crate::codec::Value` into a `Value`
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host through
    /// `Kobako::Handle`'s instance-level `method_missing` and the bridge's
    /// `forward_to_dispatch` round-trip, docs/behavior.md B-17).
    #[cfg(target_arch = "wasm32")]
    pub fn to_mrb_value(&self, val: crate::codec::Value) -> Value {
        use crate::codec::Value as CodecValue;
        use crate::mruby::IntoValue;
        let mrb = self.mrb();
        match val {
            CodecValue::Nil => Value::nil(),
            CodecValue::Bool(b) => b.into_value(mrb),
            CodecValue::Int(n) => {
                // mrb_int on wasm32 is 32-bit (MRB_INT32); clamp to i32.
                let n32 = n.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                n32.into_value(mrb)
            }
            CodecValue::UInt(n) => {
                let n32 = n.min(i32::MAX as u64) as i32;
                n32.into_value(mrb)
            }
            CodecValue::Float(f) => f.into_value(mrb),
            CodecValue::Str(s) => match std::ffi::CString::new(s.as_str()) {
                Ok(cs) => mrb.str_new_cstr(&cs),
                Err(_) => mrb.str_new(s.as_bytes()),
            },
            CodecValue::Handle(id) => self
                .handle_class
                .obj_new(mrb, &[(id as i32).into_value(mrb)]),
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
