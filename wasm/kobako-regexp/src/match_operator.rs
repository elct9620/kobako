//! The match operator on the receivers CRuby gives one besides `String`
//! and `Regexp`: a `Symbol` matches as its name, and `nil` never matches.
//! Every other receiver has none, as `Object#=~` is gone from CRuby.

use crate::string_ext;
use beni::{Error, Module, Mrb, Qnil, ReprValue, Symbol, Value};

pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let symbol = mrb.class_get(c"Symbol")?;
    symbol.define_method(mrb, c"=~", beni::method!(sym_eqtilde, 1))?;
    let nil = mrb.class_get(c"NilClass")?;
    nil.define_method(mrb, c"=~", beni::method!(nil_eqtilde, 1))?;
    Ok(())
}

/// `Symbol#=~` — the name, never its rendering, answers as a String would.
fn sym_eqtilde(mrb: &Mrb, self_: Symbol, operand: Value) -> Result<Value, Error> {
    string_ext::str_eqtilde(mrb, self_.to_str(mrb).as_value(), operand)
}

/// `NilClass#=~` — always `nil`.
fn nil_eqtilde(_mrb: &Mrb, _self: Value, _operand: Value) -> Qnil {
    beni::value::qnil()
}
