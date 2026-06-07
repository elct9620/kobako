//! The bundled guest — `KobakoGuest` composes the published `kobako`
//! harness with the `kobako-io` IO / Kernel gem and forwards the
//! `kobako_core::Guest` contract to the inherited flows.

use beni::{Error, Mrb};

/// The bundled mruby guest behind `data/kobako.wasm`.
pub(crate) struct KobakoGuest;

impl kobako::MrbGuest for KobakoGuest {
    // KobakoBridge is the harness built-in — the provided flows
    // install it themselves; the hook wires the rest of the bundled
    // gem set.
    fn init_gems(mrb: &Mrb) -> Result<(), Error> {
        mrb.init_gem::<kobako_io::KobakoIo>()
    }
}

// Forwarding impl — the orphan rule keeps it here in the shell;
// overriding a flow would mean implementing it in place of the
// forward.
impl kobako_core::Guest for KobakoGuest {
    fn eval() {
        <KobakoGuest as kobako::MrbGuest>::eval();
    }

    fn run(env: &[u8]) {
        <KobakoGuest as kobako::MrbGuest>::run(env);
    }

    fn yield_to_block(req: &[u8]) -> u64 {
        <KobakoGuest as kobako::MrbGuest>::yield_to_block(req)
    }
}

kobako_core::export_guest!(KobakoGuest);
