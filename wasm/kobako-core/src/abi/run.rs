//! The Run a `__kobako_run` invocation was handed, read before any flow
//! sees it.

use kobako_transport::envelope::{ErrorRecord, Origin, Outcome, Panic, Run};

/// Read the Run envelope the host wrote, or answer the invocation with
/// the Panic a Run that does not frame fails under.
///
/// The refusal is the guest's to report because the guest is where the
/// bytes failed to frame. Its wording stays apart from every refusal of
/// the Run's arguments, so a host can tell a framing desync from a
/// payload the codec could not read.
pub fn decode_run(env: &[u8]) -> Result<Run, Outcome> {
    Run::decode(env).map_err(|_| {
        Outcome::Panic(Panic {
            origin: Origin::Sandbox,
            error: ErrorRecord {
                name: "Kobako::Transport::Error".into(),
                message: "failed to decode the invocation request".into(),
                backtrace: Vec::new(),
            },
            available: Vec::new(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_framed_run_reaches_the_flow_whole() {
        // Spelled as the literal layout bytes rather than through
        // `Run::encode`, so the expectation does not follow the encoder.
        let env = [0, 0, 0, 4, b'M', b'a', b'i', b'n', 0x90];

        assert_eq!(
            decode_run(&env),
            Ok(Run {
                entrypoint: "Main".into(),
                payload: vec![0x90],
            }),
            "a Run the host framed must reach the flow with its entrypoint and payload intact"
        );
    }

    // @behavior OC-044
    #[test]
    fn an_unframed_run_answers_as_the_sandboxs_wire_failure() {
        let Err(Outcome::Panic(panic)) = decode_run(&[0xff]) else {
            panic!("a Run whose envelope does not frame must answer the invocation with a Panic");
        };

        assert_eq!(
            (panic.origin, panic.error.name.as_str()),
            (Origin::Sandbox, "Kobako::Transport::Error"),
            "an unframed Run through decode_run must attribute to the Sandbox under the wire \
             failure's own name"
        );
        assert!(panic.error.backtrace.is_empty());
        assert!(panic.available.is_empty());
    }
}
