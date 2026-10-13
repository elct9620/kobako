//! Symbol ⇄ Regexp integration — the `Symbol` methods CRuby defines as the
//! same call on the Symbol's name as a String. The match operator lives
//! beside `nil`'s in `match_operator`.

use crate::string_ext;
use beni::{Error, Module, Mrb, ReprValue, Symbol, Value};

pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let cls = mrb.class_get(c"Symbol")?;
    cls.define_method(mrb, c"match", beni::method!(sym_match, 1, 1))?;
    cls.define_method(mrb, c"match?", beni::method!(sym_match_p, 1, 1))?;
    cls.define_method(mrb, c"[]", beni::method!(sym_aref, -1))?;
    cls.define_method(mrb, c"slice", beni::method!(sym_aref, -1))?;
    Ok(())
}

/// The String methods read the block from this same frame, so a block given
/// to the Symbol reaches the match.
fn sym_match(mrb: &Mrb, self_: Symbol, pattern: Value, pos: Option<Value>) -> Result<Value, Error> {
    string_ext::str_match(mrb, self_.to_str(mrb).as_value(), pattern, pos)
}

fn sym_match_p(
    mrb: &Mrb,
    self_: Symbol,
    pattern: Value,
    pos: Option<Value>,
) -> Result<Value, Error> {
    string_ext::str_match_p(mrb, self_.to_str(mrb).as_value(), pattern, pos)
}

fn sym_aref(mrb: &Mrb, self_: Symbol, args: &[Value]) -> Result<Value, Error> {
    string_ext::str_aref(mrb, self_.to_str(mrb).as_value(), args)
}
