use magnus::{function, prelude::*, Error, Ruby};

mod wasm;

fn hello(subject: String) -> String {
    format!("Hello from Rust, {subject}!")
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("Kobako")?;
    module.define_singleton_method("hello", function!(hello, 1))?;
    wasm::init(ruby, module)?;
    Ok(())
}
