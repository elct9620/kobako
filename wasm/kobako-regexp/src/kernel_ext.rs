//! Kernel integration — the universal `=~` fallback.
//!
//! `String` defines its own regexp-aware `=~`; for every other receiver
//! `obj =~ x` returns `nil`, matching MRI's deprecated `Object#=~`.
//! Defining it on `Kernel` puts that fallback on every object.

use beni::{Module, Mrb, Value};

pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let kernel = mrb.define_module(c"Kernel")?;
    kernel.define_method(mrb, c"=~", beni::method!(kernel_eqtilde, 1))?;
    Ok(())
}

/// `Kernel#=~` — always `nil`; a receiver that is neither `String` nor
/// `Regexp` never matches, whatever it is matched against. CRuby takes
/// the one operand and answers `nil` the same way.
fn kernel_eqtilde(_mrb: &Mrb, _self: Value, _operand: Value) -> Value {
    Value::nil()
}
