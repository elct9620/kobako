//! Kernel integration (SPEC.md B-41) — the universal `=~` fallback.
//!
//! `String` defines its own regexp-aware `=~`; for every other receiver the
//! C Onigmo gem made `obj =~ x` return `nil` (MRI's deprecated `Object#=~`).
//! Defining it on `Kernel` puts that fallback on every object.

use beni::{Module, Mrb, Value};

pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let kernel = mrb.define_module(c"Kernel")?;
    kernel.define_method(mrb, c"=~", beni::method!(kernel_eqtilde, -1))?;
    Ok(())
}

/// `Kernel#=~` — always `nil`; a receiver that is neither `String` nor
/// `Regexp` never matches. The operand is ignored.
fn kernel_eqtilde(_mrb: &Mrb, _self: Value) -> Value {
    Value::nil()
}
