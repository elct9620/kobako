//! MessagePack ↔ mruby value conversion, the walk beneath
//! `super::MsgpackCodec`.
//!
//! The methods stay on `Kobako` (so call sites read
//! `kobako.try_codec_value(val)` rather than
//! `convert::try_codec_value(&kobako, val)`) via a second `impl` block;
//! the codec façade frames what they produce into payload positions.
//!
//! Three concerns:
//!
//! 1. **Value conversion** (`try_codec_value`) — the single guest→host
//!    value converter, shared by the `#eval` / `#run` outcome, the
//!    yield-block result, and the dispatch Call args / kwargs. A value
//!    with no wire representation yields `None`, never a coerced
//!    `Object#to_s` string: the outcome caller emits a Panic envelope, the
//!    yield caller a `0x04` error Yield Reply, and the dispatch caller
//!    raises at the guest call site. SPEC.md § Behavior pins "no implicit
//!    inspect / to_h / to_s conversion" across all three guest→host value
//!    paths.
//! 2. **Args / kwargs unpacking** (`extract_hash_kwargs` /
//!    `unpack_args_kwargs`) — used by the `method_missing` C bridges to
//!    convert a dispatch call's positional rest slice and its separate
//!    keyword Hash into wire args and kwargs, running each leaf through
//!    `try_codec_value` and reporting the first unrepresentable value as an
//!    `CodecError`.
//! 3. **Fault reading** (`decode_fault`) — the Reply arm the envelope
//!    tagged as a failure, read into the fields the bridge raises with.

use crate::codec::CodecError;
use crate::codec::Fault;
use crate::runtime::{IntegerOutOfRange, Kobako};
use beni::Value;
use kobako_codec::msgpack::codec::{self, Decoder, Value as CodecValue};
// The encode-side walk caps at the same depth the decoder enforces; the
// constant lives in `kobako-codec` so the two guest walks share one bound
// (docs/wire/payload-msgpack.md § Structural Nesting Depth).
use kobako_codec::msgpack::codec::MAX_NESTING_DEPTH;

/// Read a Reply's fault body — an ext 0x02 frame wrapping the
/// `{type, message}` map — into the two fields the bridge raises with.
/// The envelope named the arm; this reads what it carried, which is the
/// payload codec's half of the job.
pub(crate) fn decode_fault(body: &[u8]) -> Result<Fault, codec::Error> {
    let CodecValue::Fault(inner_bytes) = Decoder::new(body).read_only_value()? else {
        return Err(codec::Error::Malformed(
            "the fault arm of a Reply must carry a Fault (ext 0x02)",
        ));
    };
    let CodecValue::Map(pairs) = Decoder::new(&inner_bytes).read_value()? else {
        return Err(codec::Error::Malformed(
            "malformed error response from the host",
        ));
    };

    let mut kind = None;
    let mut message = None;
    for (key, value) in pairs {
        match (key, value) {
            (CodecValue::Str(name), CodecValue::Str(text)) if name == "type" => kind = Some(text),
            (CodecValue::Str(name), CodecValue::Str(text)) if name == "message" => {
                message = Some(text)
            }
            _ => {}
        }
    }
    Ok(Fault {
        kind: kind.ok_or(codec::Error::Malformed(
            "error response from the host is missing the field: type",
        ))?,
        message: message.ok_or(codec::Error::Malformed(
            "error response from the host is missing the field: message",
        ))?,
    })
}

/// The unpacked form of a dispatch Call's argument list: positional args
/// followed by Symbol-keyed kwargs pairs.
type UnpackedArgs = (
    Vec<kobako_codec::msgpack::codec::Value>,
    Vec<(String, kobako_codec::msgpack::codec::Value)>,
);

impl Kobako {
    /// Decode every key/value pair from an mruby Hash into `out` as
    /// `(String, codec::Value)` pairs. The outer `String` carries the
    /// key's name; `payload::Arguments`'s `Encode` impl re-emits
    /// each name as a `Value::Sym` (ext 0x00) per
    /// docs/wire/payload-msgpack.md § Ext Types. Keys arriving as either
    /// mruby `Symbol` or `String` reduce
    /// to the same UTF-8 name via `Object#to_s`. A value with no wire
    /// representation aborts the walk with `CodecError` so the
    /// caller raises at the guest dispatch call site rather than coercing it.
    pub(crate) fn extract_hash_kwargs(
        &self,
        hash: beni::Hash,
        out: &mut Vec<(String, kobako_codec::msgpack::codec::Value)>,
    ) -> Result<(), CodecError> {
        let keys_ary = hash.keys(self.mrb());
        for key_val in keys_ary.entries() {
            // A hostile Hash subclass whose `[]` raises reads as `nil`
            // for that key rather than faulting this marshalling helper.
            let val = hash.get(self.mrb(), key_val).unwrap_or(Value::nil());
            let encoded = self
                .try_codec_value(val)
                .ok_or_else(|| CodecError::unrepresentable(self, val))?;
            out.push((key_val.to_string(self.mrb()), encoded));
        }
        Ok(())
    }

    /// Convert a dispatch call's positional `rest` slice and its separate
    /// keyword `kwargs` Hash into wire args and kwargs. Every element of
    /// `rest` is a positional argument — an explicit `{...}` Hash literal
    /// among them stays positional, matching Ruby 3 call semantics; the
    /// keyword bucket arrives already separated from `mrb_get_args`, empty
    /// when the call passed no keywords.
    ///
    /// `rest` is typed as `&[Value]` even though the underlying buffer
    /// came from mruby's variadic out-param; `Value` is
    /// `#[repr(transparent)]` over `mrb_value` so the slice layouts
    /// are identical (the bridge call site casts once).
    pub(crate) fn unpack_args_kwargs(
        &self,
        rest: &[Value],
        kwargs_hash: beni::Hash,
    ) -> Result<UnpackedArgs, CodecError> {
        let mut args: Vec<kobako_codec::msgpack::codec::Value> = Vec::with_capacity(rest.len());
        for &mrb_val in rest {
            let encoded = self
                .try_codec_value(mrb_val)
                .ok_or_else(|| CodecError::unrepresentable(self, mrb_val))?;
            args.push(encoded);
        }

        let mut kwargs: Vec<(String, kobako_codec::msgpack::codec::Value)> = Vec::new();
        self.extract_hash_kwargs(kwargs_hash, &mut kwargs)?;

        Ok((args, kwargs))
    }

    /// Convert each element of an mruby Array through the strict value
    /// converter, returning a `Vec<Option<..>>` the caller collapses to a
    /// single `None` when any element has no wire representation.
    fn array_to_codec(
        &self,
        val: Value,
        depth: usize,
    ) -> Vec<Option<kobako_codec::msgpack::codec::Value>> {
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
    /// `Value::Str` — distinct codec encodings per
    /// docs/wire/payload-msgpack.md § Ext Types.
    fn hash_to_codec(
        &self,
        val: Value,
        depth: usize,
    ) -> Vec<(
        Option<kobako_codec::msgpack::codec::Value>,
        Option<kobako_codec::msgpack::codec::Value>,
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

    /// Convert a `Value` to a kobako `kobako_codec::msgpack::codec::Value` — the
    /// single guest→host value converter, shared by the `#eval` / `#run`
    /// outcome, the yield-block result, and the dispatch Call args /
    /// kwargs. Symbol values map to `Value::Sym` (ext 0x00); Array / Hash
    /// values map to `Value::Array` / `Value::Map` recursively
    /// (docs/wire/payload-msgpack.md § Type Mapping #7-#8) so a collection retains
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
    /// (outcome), a `0x04` error Yield Reply (yield), or a raise at the
    /// dispatch call site rather than handing the host a misleading String.
    pub(crate) fn try_codec_value(
        &self,
        val: Value,
    ) -> Option<kobako_codec::msgpack::codec::Value> {
        self.try_codec_value_at(val, 0)
    }

    fn try_codec_value_at(
        &self,
        val: Value,
        depth: usize,
    ) -> Option<kobako_codec::msgpack::codec::Value> {
        use beni::FromValue;
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
            // Yield Reply path rather than overflowing the wasm stack.
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

    /// Convert a kobako `kobako_codec::msgpack::codec::Value` into a `Value`
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host through
    /// `Kobako::Handle`'s instance-level `method_missing` and the bridge's
    /// `forward_to_dispatch` round-trip).
    pub(crate) fn to_mrb_value(
        &self,
        val: kobako_codec::msgpack::codec::Value,
    ) -> Result<Value, IntegerOutOfRange> {
        use beni::IntoValue;
        let mrb = self.mrb();
        Ok(match val {
            CodecValue::Nil => Value::nil(),
            CodecValue::Bool(b) => b.into_value(mrb),
            CodecValue::Int(n) => self.narrow_int(n)?,
            CodecValue::UInt(n) => self.narrow_int(n)?,
            CodecValue::Float(f) => f.into_value(mrb),
            CodecValue::Str(s) => mrb.str_new(s.as_bytes()).as_value(),
            CodecValue::Handle(id) => self.mint_handle(id),
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
            // malformed Reply that smuggles one through.
            CodecValue::Fault(_) => Value::nil(),
        })
    }
}
