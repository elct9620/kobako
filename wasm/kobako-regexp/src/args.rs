//! The call-frame read the variadic bodies in this gem make beside the
//! arguments `method!`'s any-arity form already hands them.

use beni::scan_args::scan_args;
use beni::{Error, Mrb, Proc, RArray};

/// The call's block. Any-arity bodies receive the arguments as a slice
/// but not the block, so it is read from the frame, the splat taking
/// whatever positionals the call passed.
pub(crate) fn block(mrb: &Mrb) -> Result<Option<Proc>, Error> {
    Ok(scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?.block)
}
