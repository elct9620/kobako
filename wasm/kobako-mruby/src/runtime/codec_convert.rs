//! Codec ↔ mruby Value conversion methods on `super::Kobako`.
//!
//! Keep the `Kobako` façade lean by housing the codec-adjacent
//! translation in its own sibling file. The methods stay on `Kobako`
//! (so call sites read `kobako.try_codec_value(val)` rather than
//! `codec_convert::try_codec_value(&kobako, val)`) via a second `impl`
//! block.
//!
//! Two concerns live here:
//!
//! 1. **Value conversion** (`try_codec_value`) — the single guest→host
//!    value converter, shared by the `#eval` / `#run` outcome, the
//!    yield-block result, and the dispatch Request args / kwargs. A value
//!    with no wire representation yields `None`, never a coerced
//!    `Object#to_s` string: the outcome caller emits a Panic envelope
//!    (E-06), the yield caller a `0x04` error YieldResponse (E-22), and the
//!    dispatch caller raises at the guest call site (E-55). SPEC.md
//!    § Behavior pins "no implicit inspect / to_h / to_s conversion" across
//!    all three guest→host value paths.
//! 2. **Args / kwargs unpacking** (`extract_hash_kwargs` /
//!    `unpack_args_kwargs`) — used by the `method_missing` C bridges to
//!    split a `mrb_get_args` "n*" rest slice into positional args and
//!    trailing-Hash kwargs, converting each leaf through `try_codec_value`
//!    and reporting the first unrepresentable value as `UnrepresentableArg`.

use super::Kobako;
use beni::Value;
// The encode-side walk caps at the same depth the decoder enforces; the
// constant lives in `kobako-codec` so the two guest walks share one bound
// (docs/wire-codec.md § Structural Nesting Depth).
use kobako_codec::codec::MAX_NESTING_DEPTH;

/// An inbound integer fell outside the guest's signed 32-bit `Integer`
/// range, which the MRB_INT32 build cannot hold. `to_mrb_value` refuses
/// it rather than saturating to the nearest bound (docs/wire-codec.md
/// § Integer Range); each call site fails its path the way it reports any
/// malformed inbound payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntegerOutOfRange(pub(crate) i128);

impl IntegerOutOfRange {
    /// Operator-facing message naming the value the guest could not hold.
    pub(crate) fn message(self) -> String {
        format!(
            "integer {} is outside the guest's 32-bit Integer range",
            self.0
        )
    }
}

/// A dispatch argument (or kwargs value) the guest tried to send has no
/// wire representation. The guest rejects it at the dispatch call site
/// rather than coercing it to an `Object#to_s` string (E-55), uniform with
/// the return-value (E-06) and yield-block (E-22) rejections. Carries the
/// offending value's class name for the operator-facing message.
#[derive(Debug)]
pub(crate) struct UnrepresentableArg {
    type_name: String,
}

impl UnrepresentableArg {
    /// Message naming the class the guest could not represent, matching the
    /// return / yield "... of type X is not a supported sandbox value type"
    /// convention.
    pub(crate) fn message(&self) -> String {
        format!(
            "argument of type {} is not a supported sandbox value type",
            self.type_name
        )
    }
}

/// The unpacked form of a dispatch Request's argument list: positional args
/// followed by Symbol-keyed kwargs pairs.
type UnpackedArgs = (
    Vec<kobako_codec::codec::Value>,
    Vec<(String, kobako_codec::codec::Value)>,
);

impl Kobako {
    /// Decode every key/value pair from an mruby Hash into `out` as
    /// `(String, codec::Value)` pairs. The outer `String` carries the
    /// key's name; `Request`'s `kobako_codec::codec::Encode` impl re-emits
    /// each name as a `Value::Sym` (ext 0x00) per docs/wire-codec.md § Ext
    /// Types. Keys arriving as either mruby `Symbol` or `String` reduce
    /// to the same UTF-8 name via `Object#to_s`. A value with no wire
    /// representation aborts the walk with `UnrepresentableArg` (E-55) so the
    /// caller raises at the guest dispatch call site rather than coercing it.
    pub(crate) fn extract_hash_kwargs(
        &self,
        hash: Value,
        out: &mut Vec<(String, kobako_codec::codec::Value)>,
    ) -> Result<(), UnrepresentableArg> {
        // SAFETY: callers reach this only after a `classname == "Hash"`
        // gate, so the unchecked wrap is sound.
        let hash = unsafe { beni::Hash::from_value_unchecked(hash) };
        let keys_ary = hash.keys(self.mrb());
        for key_val in keys_ary.entries() {
            // A hostile Hash subclass whose `[]` raises reads as `nil`
            // for that key rather than faulting this marshalling helper.
            let val = hash.get(self.mrb(), key_val).unwrap_or(Value::nil());
            let encoded = self
                .try_codec_value(val)
                .ok_or_else(|| self.unrepresentable_arg(val))?;
            out.push((key_val.to_string(self.mrb()), encoded));
        }
        Ok(())
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
    pub(crate) fn unpack_args_kwargs(
        &self,
        rest: &[Value],
    ) -> Result<UnpackedArgs, UnrepresentableArg> {
        let mut args: Vec<kobako_codec::codec::Value> = Vec::new();
        let mut kwargs: Vec<(String, kobako_codec::codec::Value)> = Vec::new();

        for (idx, &mrb_val) in rest.iter().enumerate() {
            let is_hash = mrb_val.classname(self.mrb()) == "Hash" && idx == rest.len() - 1;
            if is_hash {
                self.extract_hash_kwargs(mrb_val, &mut kwargs)?;
            } else {
                let encoded = self
                    .try_codec_value(mrb_val)
                    .ok_or_else(|| self.unrepresentable_arg(mrb_val))?;
                args.push(encoded);
            }
        }

        Ok((args, kwargs))
    }

    /// Tag `val` as a rejected dispatch argument (E-55), capturing its
    /// mruby class name for the operator-facing message.
    fn unrepresentable_arg(&self, val: Value) -> UnrepresentableArg {
        UnrepresentableArg {
            type_name: val.classname(self.mrb()),
        }
    }

    /// Convert each element of an mruby Array through the strict value
    /// converter, returning a `Vec<Option<..>>` the caller collapses to a
    /// single `None` when any element has no wire representation.
    fn array_to_codec(&self, val: Value, depth: usize) -> Vec<Option<kobako_codec::codec::Value>> {
        // SAFETY: callers reach this only after a `classname == "Array"`
        // gate, so the unchecked wrap is sound.
        let ary = unsafe { beni::Array::from_value_unchecked(val) };
        let entries = ary.entries();
        let mut items = Vec::with_capacity(entries.len());
        for elem in entries {
            items.push(self.try_codec_value_at(elem, depth + 1));
        }
        items
    }

    /// Convert each key/value pair of an mruby Hash through the strict value
    /// converter. Both the key and the value flow through it so a `Symbol`
    /// key arrives as `Value::Sym` (ext 0x00) and a `String` key as
    /// `Value::Str` — distinct codec encodings per docs/wire-codec.md § Ext
    /// Types.
    fn hash_to_codec(
        &self,
        val: Value,
        depth: usize,
    ) -> Vec<(
        Option<kobako_codec::codec::Value>,
        Option<kobako_codec::codec::Value>,
    )> {
        // SAFETY: callers reach this only after a `classname == "Hash"`
        // gate, so the unchecked wrap is sound.
        let hash = unsafe { beni::Hash::from_value_unchecked(val) };
        let keys_ary = hash.keys(self.mrb());
        let entries = keys_ary.entries();
        let mut pairs = Vec::with_capacity(entries.len());
        for key in entries {
            // As in `extract_hash_kwargs`: a raising `[]` reads as `nil`
            // rather than faulting the recursive converter.
            let v = hash.get(self.mrb(), key).unwrap_or(Value::nil());
            pairs.push((
                self.try_codec_value_at(key, depth + 1),
                self.try_codec_value_at(v, depth + 1),
            ));
        }
        pairs
    }

    /// Convert a `Value` to a kobako `kobako_codec::codec::Value` — the
    /// single guest→host value converter, shared by the `#eval` / `#run`
    /// outcome, the yield-block result, and the dispatch Request args /
    /// kwargs. Symbol values map to `Value::Sym` (ext 0x00); Array / Hash
    /// values map to `Value::Array` / `Value::Map` recursively
    /// (docs/wire-codec.md § Type Mapping #7-#8) so a collection retains
    /// element-level fidelity.
    ///
    /// A `Kobako::Handle` proxy the guest holds (a Service return, or a
    /// `#run` argument auto-wrap) re-emits as an `ext 0x01` Capability
    /// Handle carrying its id, so the host restores it to its original
    /// object on every guest→host value path.
    ///
    /// Returns `None` when `val` has no wire representation: any type
    /// outside the 12-entry wire set, a collection containing such a value,
    /// or a collection that nests beyond `MAX_NESTING_DEPTH` (a reference
    /// cycle necessarily does). No path coerces through an implicit `to_s` /
    /// `inspect`, so the caller surfaces the `None` as a Panic envelope
    /// (outcome, E-06), a `0x04` error YieldResponse (yield, E-22), or a
    /// raise at the dispatch call site (E-55) rather than handing the host a
    /// misleading String.
    pub(crate) fn try_codec_value(&self, val: Value) -> Option<kobako_codec::codec::Value> {
        self.try_codec_value_at(val, 0)
    }

    fn try_codec_value_at(&self, val: Value, depth: usize) -> Option<kobako_codec::codec::Value> {
        use beni::FromValue;
        use kobako_codec::codec::Value as CodecValue;
        // Scalar-leaf downcast through the safe `FromValue` seam.
        if let Some(n) = i32::from_value(val) {
            return Some(CodecValue::Int(n as i64));
        }
        if let Some(f) = f64::from_value(val) {
            return Some(CodecValue::Float(f));
        }
        match val.classname(self.mrb()).as_str() {
            "NilClass" => Some(CodecValue::Nil),
            "TrueClass" => Some(CodecValue::Bool(true)),
            "FalseClass" => Some(CodecValue::Bool(false)),
            "String" => Some(CodecValue::Str(val.to_string(self.mrb()))),
            "Symbol" => Some(CodecValue::Sym(val.to_string(self.mrb()))),
            // A Capability Handle the guest received earlier this
            // invocation is wire-representable: re-emit it as ext 0x01 so
            // the host restores the original object.
            // id 0 means a missing or forged ivar — treat as
            // unrepresentable rather than emit a wire-violation Handle.
            "Kobako::Handle" => match self.extract_handle_id(val) {
                0 => None,
                id => Some(CodecValue::Handle(id)),
            },
            // A single unrepresentable element collapses the whole
            // collection to `None` — `collect::<Option<Vec<_>>>()`
            // short-circuits on the first `None`. Past `MAX_NESTING_DEPTH`
            // (a too-deep structure or a reference cycle) the arm falls
            // through to `None`, so the caller takes the Panic / error
            // YieldResponse path rather than overflowing the wasm stack.
            "Array" if depth < MAX_NESTING_DEPTH => self
                .array_to_codec(val, depth)
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(CodecValue::Array),
            "Hash" if depth < MAX_NESTING_DEPTH => self
                .hash_to_codec(val, depth)
                .into_iter()
                .map(|(k, v)| k.zip(v))
                .collect::<Option<Vec<_>>>()
                .map(CodecValue::Map),
            _ => None,
        }
    }

    /// Convert a kobako `kobako_codec::codec::Value` into a `Value`
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host through
    /// `Kobako::Handle`'s instance-level `method_missing` and the bridge's
    /// `forward_to_dispatch` round-trip).
    pub(crate) fn to_mrb_value(
        &self,
        val: kobako_codec::codec::Value,
    ) -> Result<Value, IntegerOutOfRange> {
        use beni::IntoValue;
        use kobako_codec::codec::Value as CodecValue;
        let mrb = self.mrb();
        Ok(match val {
            CodecValue::Nil => Value::nil(),
            CodecValue::Bool(b) => b.into_value(mrb),
            CodecValue::Int(n) => {
                // mrb_int on wasm32 is signed 32-bit (MRB_INT32); a value
                // outside that range has no faithful guest representation
                // and is refused rather than saturated.
                let n32 = i32::try_from(n).map_err(|_| IntegerOutOfRange(n as i128))?;
                n32.into_value(mrb)
            }
            CodecValue::UInt(n) => {
                let n32 = i32::try_from(n).map_err(|_| IntegerOutOfRange(n as i128))?;
                n32.into_value(mrb)
            }
            CodecValue::Float(f) => f.into_value(mrb),
            CodecValue::Str(s) => mrb.str_new(s.as_bytes()).as_value(),
            CodecValue::Handle(id) => self
                .handle_class
                .obj_new(mrb, &[(id as i32).into_value(mrb)])
                // `Kobako::Handle#initialize` only stores an ivar on the
                // fresh instance and cannot raise; a lost Handle degrades
                // to `nil` (the error channel is reserved for the
                // integer-range refusal, the one conversion that fails on
                // ordinary data).
                .unwrap_or(Value::nil()),
            CodecValue::Bin(bytes) => mrb.str_new(&bytes).as_value(),
            CodecValue::Sym(name) => {
                // Intern via String#to_sym — mruby's mrb_symbol_value
                // bit-layout is build-private (we use
                // MRB_WORDBOX_NO_INLINE_FLOAT) so we go through the VM.
                // `to_sym` on this fresh String cannot raise; degrade to
                // the String itself.
                let s = mrb.str_new(name.as_bytes()).as_value();
                s.funcall(mrb, c"to_sym", &[]).unwrap_or(s)
            }
            CodecValue::Array(items) => {
                let ary = mrb.ary_new();
                for item in items {
                    let elem = self.to_mrb_value(item)?;
                    // Fresh array, never frozen — the push cannot raise.
                    let _ = ary.push(mrb, elem);
                }
                ary.as_value()
            }
            CodecValue::Map(pairs) => {
                let hash = mrb.hash_new();
                for (k, v) in pairs {
                    let key = self.to_mrb_value(k)?;
                    let val = self.to_mrb_value(v)?;
                    // Fresh hash, never frozen — the set cannot raise.
                    let _ = hash.set(mrb, key, val);
                }
                hash.as_value()
            }
            // ext 0x02 envelopes are consumed by the exception path
            // (`raise_service_error`) before reaching value
            // conversion; the defensive nil here covers any
            // malformed Response that smuggles one through.
            CodecValue::ErrEnv(_) => Value::nil(),
        })
    }
}
