use magnus::{Error, Ruby};

mod wasm;

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("Kobako")?;
    wasm::init(ruby, module)?;
    Ok(())
}
