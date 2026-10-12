//! The one place a downcall is read and answered: the request's tag picks
//! the `Guest` entry, and whatever it answers is framed here.

use kobako_transport::envelope::{
    CompileReply, Downcall, ErrorRecord, Origin, Outcome, Panic, Preamble, Unsupported,
};

use crate::{preamble, Guest};

/// Answer one downcall request as the encoded message the host reads.
///
/// A tag this guest does not serve is answered as unsupported, so the
/// host can tell "not here" from a guest that broke. A request that does
/// not frame answers with the Panic of a guest that could not read its
/// invocation, in wording apart from every refusal of its arguments.
pub fn serve<G: Guest>(request: &[u8]) -> Vec<u8> {
    match Downcall::decode(request) {
        Ok(Downcall::Eval(source)) => invoke(|preamble| G::eval(preamble, source)),
        Ok(Downcall::Run(run)) => invoke(|preamble| G::run(preamble, run)),
        Ok(Downcall::Yield(args)) => G::yield_to_block(&args).encode(),
        Ok(Downcall::Compile) => compile::<G>().encode(),
        Ok(_) => {
            Unsupported::new(format!("this guest does not serve tag {:#04x}", request[0])).encode()
        }
        Err(_) => Outcome::Panic(Panic {
            origin: Origin::Sandbox,
            error: ErrorRecord {
                name: "Kobako::Transport::Error".into(),
                message: "failed to decode the invocation request".into(),
                backtrace: Vec::new(),
            },
            available: Vec::new(),
        })
        .encode(),
    }
}

/// An invocation starts from its Preamble; one that cannot be read
/// answers as the boot failure it is.
fn invoke(entry: impl FnOnce(Preamble) -> Outcome) -> Vec<u8> {
    preamble::read_preamble()
        .map(entry)
        .unwrap_or_else(Outcome::Panic)
        .encode()
}

/// A compile reads the Preamble an invocation would, and reports one it
/// cannot read with the same record.
fn compile<G: Guest>() -> CompileReply {
    preamble::read_preamble()
        .map(G::compile)
        .unwrap_or_else(|refused| CompileReply::Error(refused.error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako_transport::envelope::{Run, YieldReply};

    /// A guest that opts out of block support by leaving the
    /// `yield_to_block` default in place.
    struct NoBlockGuest;

    impl Guest for NoBlockGuest {
        fn eval(_preamble: Preamble, _source: Vec<u8>) -> Outcome {
            Outcome::Ok(Vec::new())
        }
        fn run(_preamble: Preamble, _run: Run) -> Outcome {
            Outcome::Ok(Vec::new())
        }
    }

    // @behavior T-181
    #[test]
    fn a_guest_serving_no_block_answers_a_yield_as_unsupported() {
        let answer = serve::<NoBlockGuest>(&[0x03, 0x90]);

        assert!(
            matches!(YieldReply::decode(&answer), Ok(YieldReply::Unsupported(_))),
            "a yield through serve against a guest with the default yield_to_block must \
             answer as unsupported rather than trap"
        );
    }

    // `serve` reads the Preamble from stdin before it routes a compile, so
    // the default answer is witnessed on its own; the routing itself is
    // witnessed through the real guest's compile.
    // @behavior T-281
    #[test]
    fn a_guest_that_does_not_compile_answers_a_compile_as_unsupported() {
        assert!(
            matches!(
                NoBlockGuest::compile(Preamble::default()),
                CompileReply::Unsupported(_)
            ),
            "a compile against a guest with the default Guest::compile must answer as \
             unsupported rather than fail"
        );
    }

    // @behavior T-277
    #[test]
    fn a_tag_the_guest_does_not_serve_is_answered_as_unsupported() {
        let answer = serve::<NoBlockGuest>(&[0x09]);

        assert_eq!(
            answer[0], 0x15,
            "a downcall under an unassigned tag through serve must answer on the unsupported arm"
        );
    }

    // @behavior OC-044
    #[test]
    fn an_unframed_run_answers_as_the_sandboxs_wire_failure() {
        let answer = serve::<NoBlockGuest>(&[0x02, 0xff]);
        let Ok(Outcome::Panic(panic)) = Outcome::decode(&answer) else {
            panic!("a run whose body does not frame must answer the invocation with a Panic");
        };

        assert_eq!(
            (panic.origin, panic.error.name.as_str()),
            (Origin::Sandbox, "Kobako::Transport::Error"),
            "an unframed Run through serve must attribute to the Sandbox under the wire \
             failure's own name"
        );
        assert!(panic.error.backtrace.is_empty());
        assert!(panic.available.is_empty());
    }

    // The invalid tag and a tag another direction carries are refused
    // when read, so the guest never reaches an entry for either.
    // @behavior OC-044
    #[test]
    fn a_request_the_wire_refuses_answers_as_the_sandboxs_wire_failure() {
        for request in [&[0x00][..], &[0x04, 0x90][..]] {
            let answer = serve::<NoBlockGuest>(request);

            assert!(
                matches!(
                    Outcome::decode(&answer),
                    Ok(Outcome::Panic(Panic { origin: Origin::Sandbox, ref error, .. }))
                        if error.name == "Kobako::Transport::Error"
                ),
                "a downcall under tag {:#04x} through serve must answer as the Sandbox's \
                 wire failure rather than as unsupported",
                request[0]
            );
        }
    }
}
