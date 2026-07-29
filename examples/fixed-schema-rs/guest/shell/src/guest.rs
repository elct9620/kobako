//! The guest this shell assembles.
//!
//! Two declarations make a guest: the schema its payloads travel under
//! and the gems its scripts can reach. Everything else — booting the
//! interpreter, framing messages, replaying snippets, routing a
//! dispatch — is the harness's, inherited by forwarding.

use beni::{Error, Mrb};

/// The guest behind `guest.wasm`.
pub(crate) struct KvGuest;

impl kobako_mruby::MrbGuest for KvGuest {
    // Named here rather than inherited: nothing beneath this line reads
    // a payload byte, so the schema is the shell's to pick and the
    // transport is unchanged by the choice.
    type Codec = crate::codec::RawBytes;

    // The wire-tied bridge installs itself before this hook runs, so
    // what a script sees beyond the mruby core is exactly what is named
    // here — one gem, granting one Service surface and no ambient
    // capability.
    fn init_gems(mrb: &Mrb) -> Result<(), Error> {
        mrb.init_gem::<kv::KvGem>()
    }
}

// The orphan rule keeps this forwarding impl in the shell. Implementing
// a flow here instead of forwarding it is how a shell replaces one.
impl kobako_core::Guest for KvGuest {
    fn eval() {
        <KvGuest as kobako_mruby::MrbGuest>::eval();
    }

    fn run(env: &[u8]) {
        <KvGuest as kobako_mruby::MrbGuest>::run(env);
    }

    fn yield_to_block(req: &[u8]) -> u64 {
        <KvGuest as kobako_mruby::MrbGuest>::yield_to_block(req)
    }
}

kobako_core::export_guest!(KvGuest);
