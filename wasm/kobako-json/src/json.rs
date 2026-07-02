//! The guest `JSON` module — the `parse`, `generate`, and
//! `pretty_generate` module functions plus the `Object#as_json` opt-in
//! hook. The value conversion lives in `crate::convert`; this file owns
//! the Ruby-visible surface.
//!
//! `Object#as_json` extends a core class yet lives here, not in an
//! `object_ext.rs`: it is `generate`'s opt-in serialization protocol
//! rather than a general Object extension, so it stays with the JSON
//! surface it serves.

use crate::convert;
use crate::errors;
use beni::{format, Error, FromValue, Hash, Module, Mrb, RString, Symbol, Value};

/// Define the `JSON` module, its three functions, and the `as_json`
/// serialization hook on `Object`.
pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let json = mrb.define_module(c"JSON")?;
    json.define_module_function(mrb, c"parse", beni::method!(json_parse, -1))?;
    json.define_module_function(mrb, c"generate", beni::method!(json_generate, -1))?;
    json.define_module_function(
        mrb,
        c"pretty_generate",
        beni::method!(json_pretty_generate, -1),
    )?;

    // An object joins `generate` by overriding this; the `Object`-rooted
    // default raises, so a `Kobako::Handle` / `Member` / un-opted object
    // fails loud instead of dispatching to the host.
    mrb.object_class()
        .define_method(mrb, c"as_json", beni::method!(object_as_json, 0))?;
    Ok(())
}

/// `JSON.parse(source, symbolize_names: false)` — parse a JSON `String`
/// into native mruby values. A non-`String` source is a `TypeError`;
/// malformed input is a `JSON::ParserError`.
fn json_parse(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    let source = args.first().copied().unwrap_or(Value::nil());
    let rstr = RString::from_value(source)
        .ok_or_else(|| errors::type_error(mrb, "no implicit conversion of argument into String"))?;
    // Read the option before borrowing the source bytes — the lookup is
    // an mruby call that may move the heap.
    let symbolize = symbolize_names(mrb, args.get(1).copied());
    // SAFETY: the slice is consumed by `from_slice` before `decode`
    // allocates any mruby value, so the source string cannot move
    // underneath it; `arbitrary_precision` leaves the parsed tree owning
    // its own copies.
    let bytes = unsafe { rstr.as_bytes(mrb) };
    let json: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| errors::parser_error(mrb, &e.to_string()))?;
    convert::decode(mrb, &json, symbolize)
}

/// `JSON.generate(obj)` — a compact JSON `String`.
fn json_generate(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let obj = mrb.get_args::<format::O>();
    emit(mrb, obj, false)
}

/// `JSON.pretty_generate(obj)` — an indented JSON `String`.
fn json_pretty_generate(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let obj = mrb.get_args::<format::O>();
    emit(mrb, obj, true)
}

fn emit(mrb: &Mrb, obj: Value, pretty: bool) -> Result<Value, Error> {
    let json = convert::encode(mrb, obj, 0)?;
    let text = if pretty {
        serde_json::to_string_pretty(&json)
    } else {
        serde_json::to_string(&json)
    }
    .map_err(|e| errors::generator_error(mrb, &e.to_string()))?;
    Ok(mrb.str_new(text.as_bytes()).as_value())
}

/// `Object#as_json` — the raising default. The generator handles native
/// types directly and only calls this for a non-native value, so an
/// object that has not overridden it is refused here rather than
/// stringified through a host-dispatching `to_s`.
fn object_as_json(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Err(errors::generator_error(
        mrb,
        &format!(
            "{} has no JSON representation; define #as_json to opt in",
            self_.classname(mrb)
        ),
    ))
}

/// Read the truthy `symbolize_names:` option from a trailing kwargs Hash.
/// Absent or non-Hash arguments default to `false`.
fn symbolize_names(mrb: &Mrb, opts: Option<Value>) -> bool {
    let Some(opts) = opts else {
        return false;
    };
    let Some(hash) = Hash::from_value(opts) else {
        return false;
    };
    let key = Symbol::new(mrb, c"symbolize_names").as_value();
    hash.get(mrb, key).map(|v| v.to_bool()).unwrap_or(false)
}
