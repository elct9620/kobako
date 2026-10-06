//! The call-frame reads the variadic bodies in this gem make.
//!
//! A body with a fixed argument list takes them as typed parameters
//! instead; these two shapes are what `method!`'s any-arity form leaves
//! to the body.

use beni::scan_args::scan_args;
use beni::{Array, Error, Mrb, Proc, Value};

/// Owned, so the arguments outlive any mruby call the body makes
/// afterwards.
pub(crate) fn rest(mrb: &Mrb) -> Result<Vec<Value>, Error> {
    scan_args::<(), (), Array, (), (), ()>(mrb)?
        .splat
        .to_vec(mrb)
}

pub(crate) fn rest_block(mrb: &Mrb) -> Result<(Vec<Value>, Option<Proc>), Error> {
    let args = scan_args::<(), (), Array, (), (), Option<Proc>>(mrb)?;
    Ok((args.splat.to_vec(mrb)?, args.block))
}
