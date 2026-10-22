//! The bundled guest — `KobakoGuest` composes the published
//! `kobako-mruby` harness with the `kobako-io` IO / File / Kernel gem and forwards the
//! `kobako_core::Guest` contract to the inherited flows. The
//! `kobako-regexp` and `kobako-json` capability gems are wired only under
//! the `regexp` / `json` features, so the default binary stays pure mruby
//! + `kobako-io`.

use beni::{Error, Mrb};
use kobako_core::{CompileReply, Flag, Outcome, Preamble, Run, YieldReply};

/// The bundled mruby guest behind `data/kobako.wasm`.
pub(crate) struct KobakoGuest;

impl kobako_mruby::MrbGuest for KobakoGuest {
    // MessagePack is the schema this binary speaks, chosen here rather
    // than assumed below: the harness and the transport read no payload
    // byte of their own, so a shell wanting another wire names it here
    // and nothing beneath changes.
    type Codec = kobako_mruby::MsgpackCodec;

    // KobakoBridge is the harness built-in — the provided flows
    // install it themselves; the hook wires the rest of the bundled
    // gem set.
    fn init_gems(mrb: &Mrb) -> Result<(), Error> {
        mrb.init_gem::<kobako_io::KobakoIo>()?;
        #[cfg(feature = "regexp")]
        mrb.init_gem::<kobako_regexp::KobakoRegexp>()?;
        #[cfg(feature = "json")]
        mrb.init_gem::<kobako_json::KobakoJson>()?;
        Ok(())
    }

    /// `disable_file` removes `File` before any user code runs, for a Host
    /// App that wants a guest with no file surface at all. Without a Mount
    /// `File` already reaches nothing, so this narrows
    /// what the guest can name, not what it can reach.
    fn apply_flag(mrb: &Mrb, flag: &Flag) -> Result<bool, Error> {
        use beni::Module;

        match flag.name.as_str() {
            "disable_file" => {
                mrb.class_object().const_remove(mrb, c"File")?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

// Forwarding impl — the orphan rule keeps it here in the shell;
// overriding a flow would mean implementing it in place of the
// forward.
impl kobako_core::Guest for KobakoGuest {
    fn eval(preamble: Preamble, source: Vec<u8>) -> Outcome {
        <KobakoGuest as kobako_mruby::MrbGuest>::eval(preamble, source)
    }

    fn run(preamble: Preamble, run: Run) -> Outcome {
        <KobakoGuest as kobako_mruby::MrbGuest>::run(preamble, run)
    }

    fn yield_to_block(req: &[u8]) -> YieldReply {
        <KobakoGuest as kobako_mruby::MrbGuest>::yield_to_block(req)
    }

    fn compile(preamble: Preamble) -> CompileReply {
        <KobakoGuest as kobako_mruby::MrbGuest>::compile(preamble)
    }
}

kobako_core::export_guest!(KobakoGuest);
