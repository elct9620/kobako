//! The bundled guest — `kobako_core::Guest` impl wiring the mruby
//! invocation bodies in `crate::abi` to the macro-emitted ABI exports.

use crate::abi;

/// The bundled mruby guest behind `data/kobako.wasm`.
pub(crate) struct KobakoGuest;

impl kobako_core::Guest for KobakoGuest {
    fn eval() {
        abi::eval();
    }

    fn run(env: &[u8]) {
        abi::run(env);
    }

    fn yield_to_block(req: &[u8]) -> u64 {
        abi::yield_to_block(req)
    }
}

kobako_core::export_guest!(KobakoGuest);
