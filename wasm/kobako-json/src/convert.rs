//! `serde_json::Value` ⇄ mruby `Value` bridge for the JSON surface.
//!
//! `decode` (inbound, used by `parse`) maps parsed JSON to native mruby
//! values; `encode` (outbound, used by `generate` / `pretty_generate`)
//! maps mruby values to JSON. Both honor one nesting bound owned by this
//! capability.
//!
//! The outbound walk classifies a value by its native mruby type, not its
//! Ruby class identity, and reaches a non-native object ONLY through the
//! `Object`-rooted `as_json` hook. Keying on the type tag means a subclass
//! of a native type still serializes as that kind, while a capability proxy
//! — whatever names it answers to — cannot masquerade as one. It
//! never probes a value via `respond_to?` or a conversion protocol
//! (`to_ary` / `to_hash` / a bare `to_json`): on a capability proxy those
//! forward to the host — `respond_to?` lies (the proxy answers every name)
//! and an undefined convert method fires `method_missing`. A method
//! *defined* on `Object` never reaches `method_missing`, so the hook
//! refuses a `Kobako::Handle` / `Member` / un-opted object without a host
//! round-trip.

use crate::errors::{generator_error, parser_error};
use beni::{Array, Error, FromValue, Hash, IntoValue, Mrb, RString, Symbol, Value};
use serde_json::{Map, Number, Value as JsonValue};

/// The maximum container nesting `generate` accepts. serde_json's parse
/// recursion limit (128) admits 127 nested levels and rejects the 128th;
/// the outbound walk mirrors that exact bound so a generated structure
/// always re-parses — a value one path rejects the other rejects too.
const MAX_NESTING_DEPTH: usize = 127;

/// 2^53 — the largest magnitude an f64 carries without integer precision
/// loss, the ceiling of the parse integer policy's `Float` band.
const FLOAT_EXACT_INT_LIMIT: u128 = 1 << 53;

/// Map a parsed `serde_json::Value` to a native mruby value tree
/// (`parse`). Object keys become `Symbol` when `symbolize` is set,
/// `String` otherwise.
pub(crate) fn decode(mrb: &Mrb, json: &JsonValue, symbolize: bool) -> Result<Value, Error> {
    match json {
        JsonValue::Null => Ok(Value::nil()),
        JsonValue::Bool(b) => Ok(b.into_value(mrb)),
        JsonValue::Number(n) => decode_number(mrb, n),
        JsonValue::String(s) => Ok(mrb.str_new(s.as_bytes()).as_value()),
        JsonValue::Array(items) => {
            let ary = mrb.ary_new();
            for item in items {
                let elem = decode(mrb, item, symbolize)?;
                // Fresh array, never frozen — the push cannot raise.
                let _ = ary.push(mrb, elem);
            }
            Ok(ary.as_value())
        }
        JsonValue::Object(map) => {
            let hash = mrb.hash_new();
            for (k, v) in map {
                let key = decode_key(mrb, k, symbolize);
                let val = decode(mrb, v, symbolize)?;
                // Fresh hash, never frozen — the set cannot raise.
                let _ = hash.set(mrb, key, val);
            }
            Ok(hash.as_value())
        }
    }
}

/// Apply the integer-range policy. A real maps to `Float`; an
/// integer maps to `Integer` when it fits the guest's 32-bit width, to an
/// exact `Float` up to 2^53, and otherwise raises rather than degrade.
fn decode_number(mrb: &Mrb, n: &Number) -> Result<Value, Error> {
    // `arbitrary_precision` keeps the original literal, so its textual
    // form distinguishes an integer from a real even past `u64` range.
    let literal = n.to_string();
    if literal.contains(['.', 'e', 'E']) {
        let f = n
            .as_f64()
            .ok_or_else(|| parser_error(mrb, "number is out of range"))?;
        return Ok(f.into_value(mrb));
    }
    match literal.parse::<i128>() {
        Ok(value) => {
            if let Ok(small) = i32::try_from(value) {
                Ok(small.into_value(mrb))
            } else if value.unsigned_abs() <= FLOAT_EXACT_INT_LIMIT {
                Ok((value as f64).into_value(mrb))
            } else {
                Err(too_large(mrb, &literal))
            }
        }
        Err(_) => Err(too_large(mrb, &literal)),
    }
}

fn too_large(mrb: &Mrb, literal: &str) -> Error {
    parser_error(
        mrb,
        &format!(
            "integer {} exceeds the range the guest can represent exactly",
            clamp_literal(literal)
        ),
    )
}

/// Cap an untrusted numeric literal before it enters an error message, so a
/// pathologically long digit run cannot inflate the exception. A literal past
/// the cap shows a bounded prefix and its length instead of its full text.
fn clamp_literal(literal: &str) -> String {
    const MAX: usize = 32;
    if literal.len() <= MAX {
        return literal.to_string();
    }
    let head: String = literal.chars().take(MAX).collect();
    format!("{head}... ({} digits)", literal.len())
}

/// Build a `String` or interned `Symbol` object key. Interning routes
/// through `String#to_sym` rather than a `CStr` so an arbitrary key byte
/// sequence (a key may carry a NUL) interns intact.
fn decode_key(mrb: &Mrb, key: &str, symbolize: bool) -> Value {
    let s = mrb.str_new(key.as_bytes()).as_value();
    if symbolize {
        s.funcall(mrb, c"to_sym", &[]).unwrap_or(s)
    } else {
        s
    }
}

/// Map an mruby value to a `serde_json::Value` (`generate` /
/// `pretty_generate`). `depth` is the current nesting level, bounded by
/// `MAX_NESTING_DEPTH`.
pub(crate) fn encode(mrb: &Mrb, val: Value, depth: usize) -> Result<JsonValue, Error> {
    // Dispatch on the value's native mruby type through the safe `FromValue`
    // downcast / tag predicates, as the guest codec does — never on its Ruby
    // class identity, so a native subclass still serializes as its kind and a
    // capability proxy cannot masquerade by answering a name.
    if let Some(n) = i32::from_value(val) {
        return Ok(JsonValue::Number(Number::from(n)));
    }
    if let Some(f) = f64::from_value(val) {
        return number_from_f64(mrb, f);
    }
    if val.is_nil() {
        return Ok(JsonValue::Null);
    }
    if val.is_true() {
        return Ok(JsonValue::Bool(true));
    }
    if val.is_false() {
        return Ok(JsonValue::Bool(false));
    }
    if let Some(s) = RString::from_value(val) {
        return Ok(JsonValue::String(utf8_string(mrb, s)?));
    }
    if Symbol::from_value(val).is_some() {
        return Ok(JsonValue::String(val.to_string(mrb)));
    }
    if let Some(ary) = Array::from_value(val) {
        return encode_array(mrb, ary, depth);
    }
    if let Some(hash) = Hash::from_value(val) {
        return encode_hash(mrb, hash, depth);
    }
    // Any other value — including a `Kobako::Handle` or `Member` — is reached
    // only through the `Object`-rooted `as_json` hook.
    encode_via_as_json(mrb, val, depth)
}

fn number_from_f64(mrb: &Mrb, f: f64) -> Result<JsonValue, Error> {
    Number::from_f64(f)
        .map(JsonValue::Number)
        .ok_or_else(|| generator_error(mrb, "NaN and Infinity are not valid JSON"))
}

fn encode_array(mrb: &Mrb, ary: Array, depth: usize) -> Result<JsonValue, Error> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(too_deep(mrb));
    }
    let entries = ary.entries();
    let mut items = Vec::with_capacity(entries.len());
    for elem in entries {
        items.push(encode(mrb, elem, depth + 1)?);
    }
    Ok(JsonValue::Array(items))
}

fn encode_hash(mrb: &Mrb, hash: Hash, depth: usize) -> Result<JsonValue, Error> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(too_deep(mrb));
    }
    let keys = hash.keys(mrb);
    let entries = keys.entries();
    let mut map = Map::with_capacity(entries.len());
    for key in entries {
        // `hash.get` is the C hash lookup, not a Ruby `[]` dispatch, so a key
        // missing from the snapshot (e.g. removed mid-walk) reads as `nil`
        // rather than faulting the recursive converter.
        let value = hash.get(mrb, key).unwrap_or(Value::nil());
        map.insert(encode_key(mrb, key)?, encode(mrb, value, depth + 1)?);
    }
    Ok(JsonValue::Object(map))
}

/// Render an object key as its JSON string form. A `String`, `Symbol`, or
/// JSON-native scalar (number, `nil`, boolean) renders to text, as in
/// CRuby; any other key — an `Array`, a `Hash`, a `Kobako::Handle`, a
/// `Member`, or any non-native object — is refused through the same
/// boundary as a non-native value, never stringified through a
/// host-dispatching `to_s`. The `as_json` opt-in applies to values, never
/// to keys.
fn encode_key(mrb: &Mrb, key: Value) -> Result<String, Error> {
    if let Some(n) = i32::from_value(key) {
        return Ok(n.to_string());
    }
    if f64::from_value(key).is_some() {
        return Ok(key.to_string(mrb));
    }
    if key.is_nil() {
        return Ok(String::new());
    }
    if key.is_true() {
        return Ok("true".to_string());
    }
    if key.is_false() {
        return Ok("false".to_string());
    }
    if let Some(s) = RString::from_value(key) {
        return utf8_string(mrb, s);
    }
    if Symbol::from_value(key).is_some() {
        return Ok(key.to_string(mrb));
    }
    Err(generator_error(
        mrb,
        &format!("{} is not a valid JSON object key", key.classname(mrb)),
    ))
}

fn encode_via_as_json(mrb: &Mrb, val: Value, depth: usize) -> Result<JsonValue, Error> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(too_deep(mrb));
    }
    // The `Object#as_json` default raises `JSON::GeneratorError`; only an
    // object that overrode it returns a value to encode. The raise
    // resolves on `Object`, never `method_missing`, so a proxy refuses
    // here without forwarding to the host.
    let projected = val.funcall(mrb, c"as_json", &[])?;
    encode(mrb, projected, depth + 1)
}

/// Read a `String` value's bytes as a Rust `String`. JSON text is UTF-8,
/// so a non-UTF-8 byte sequence is refused rather than lossily transcoded.
fn utf8_string(mrb: &Mrb, s: RString) -> Result<String, Error> {
    String::from_utf8(s.to_bytes()).map_err(|_| generator_error(mrb, "string is not valid UTF-8"))
}

fn too_deep(mrb: &Mrb) -> Error {
    generator_error(mrb, "nesting is too deep")
}
