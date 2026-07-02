use magnus::{Error, Ruby};

mod runtime;
mod snapshot;

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("Kobako")?;
    runtime::init(ruby, module)?;
    snapshot::init(ruby, module)?;
    Ok(())
}
