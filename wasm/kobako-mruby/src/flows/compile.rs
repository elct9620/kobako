//! The `compile` downcall — the Preamble's source snippets as bytecode.
//!
//! Each source snippet is compiled under the `(snippet:Name)` filename a
//! replay would give it, and dumped with its debug info, so the bytecode
//! replays with the same backtrace attribution. Nothing compiled here
//! runs. Bytecode snippets have nothing to compile and are left out.

use super::boot;
use super::panic::load_panic;
use crate::runtime::Kobako;
use beni::DumpOptions;
use kobako_transport::envelope::{CompileReply, Panic, Preamble, Snippet};

pub(crate) fn compile<G: crate::MrbGuest>(preamble: Preamble) -> CompileReply {
    boot::boot_with_flags::<G>(&preamble)
        .and_then(|kobako| compile_sources(&kobako, &preamble.snippets.entries))
        .map_or_else(|failed| CompileReply::Error(failed.error), CompileReply::Ok)
}

/// One bytecode per source snippet in order, or the first failure: a
/// partial answer would replay some snippets as source and the rest as
/// bytecode for no gain.
fn compile_sources(kobako: &Kobako, snippets: &[Snippet]) -> Result<Vec<Vec<u8>>, Panic> {
    snippets
        .iter()
        .filter_map(|entry| match entry {
            Snippet::Source { name, body } => Some(compile_source(kobako, name, body)),
            Snippet::Bytecode { .. } => None,
        })
        .collect()
}

fn compile_source(kobako: &Kobako, name: &str, body: &str) -> Result<Vec<u8>, Panic> {
    let filename = boot::snippet_filename(name)?;
    let options = DumpOptions {
        debug_info: true,
        locals: false,
    };
    boot::snippet_context(kobako, &filename)?
        .compile(body.as_bytes())
        .and_then(|program| program.dump(kobako.mrb(), options))
        .map_err(|err| load_panic(kobako, &filename, err))
}

#[cfg(test)]
mod tests {
    use beni::{Error, Mrb, TryConvert};

    use super::*;
    use crate::codec::{CodecError, PayloadCodec};

    struct FloorCodec;

    impl PayloadCodec for FloorCodec {
        fn encode_value(_kobako: &Kobako, _value: beni::Value) -> Result<Vec<u8>, CodecError> {
            Err(CodecError::Unserved)
        }
    }

    struct BridgeOnly;

    impl crate::MrbGuest for BridgeOnly {
        type Codec = FloorCodec;

        fn init_gems(_mrb: &Mrb) -> Result<(), Error> {
            Ok(())
        }
    }

    fn source(name: &str, body: &str) -> Snippet {
        Snippet::Source {
            name: name.into(),
            body: body.into(),
        }
    }

    /// A fresh VM per call, so a test compiles in one interpreter and
    /// replays in another, as a host does.
    fn compiled(snippets: &[Snippet]) -> Result<Vec<Vec<u8>>, Panic> {
        let mrb = Mrb::open().expect("the interpreter must open");
        let kobako = Kobako::init::<BridgeOnly>(&mrb).expect("a bridge-only shell must boot");
        compile_sources(&kobako, snippets)
    }

    fn replayed(bytecodes: &[Vec<u8>], script: &str) -> Result<String, String> {
        let mrb = Mrb::open().expect("the interpreter must open");
        for bytecode in bytecodes {
            mrb.load_bytecode(bytecode)
                .map_err(|err| err.message(&mrb))?;
        }
        mrb.load_string(script.as_bytes())
            .and_then(|value| String::try_convert(value, &mrb))
            .map_err(|err| err.message(&mrb))
    }

    // @behavior MR-019
    #[test]
    fn compiled_snippets_replay_what_their_source_defines() {
        let bytecodes = compiled(&[
            source("Greeter", "module Greeter; def self.hi = 'hi'; end"),
            source("Shout", "module Shout; def self.of(s) = s.upcase; end"),
        ])
        .expect("snippets that parse must compile");

        assert_eq!(
            replayed(&bytecodes, "Shout.of(Greeter.hi)"),
            Ok("HI".to_string()),
            "source snippets through compile must come back as bytecode that, replayed in \
             order in another interpreter, defines what the source defined"
        );
    }

    // @behavior MR-020
    #[test]
    fn compiled_snippets_keep_their_name_in_backtraces() {
        let bytecodes = compiled(&[source("Boom", "def boom\n  raise 'x'\nend")])
            .expect("a snippet that parses must compile");

        let backtrace = replayed(
            &bytecodes,
            "begin; boom; rescue => e; e.backtrace.first; end",
        );

        assert!(
            backtrace
                .as_deref()
                .is_ok_and(|line| line.starts_with("(snippet:Boom):2")),
            "a method from a compiled snippet must raise with a backtrace naming the snippet \
             and its line, as the source replay does, got {backtrace:?}"
        );
    }

    // @behavior MR-021
    #[test]
    fn compiling_runs_nothing_in_the_snippet() {
        let bytecodes = compiled(&[source("Raises", "raise 'ran at compile'")]);

        assert!(
            bytecodes.is_ok(),
            "a snippet that raises when run through compile must still compile, since \
             compiling runs none of it"
        );
    }

    // @behavior MR-022
    #[test]
    fn one_snippet_that_does_not_compile_fails_the_whole_compile() {
        let failed = compiled(&[source("Fine", "1"), source("Broken", "def (")])
            .expect_err("a snippet that does not parse must fail the compile");

        assert_eq!(
            (
                failed.error.name.as_str(),
                failed.error.message.starts_with("(snippet:Broken):")
            ),
            ("SyntaxError", true),
            "a snippet that does not parse through compile must fail the whole compile as a \
             SyntaxError naming the snippet"
        );
    }

    // @behavior MR-023
    #[test]
    fn compiling_the_same_snippets_twice_yields_the_same_bytes() {
        let snippets = [source(
            "Mixed",
            "module Mixed\n  RATE = 1.5\n  TABLE = { a: 1, 'b' => [2.25, :c] }\n  \
             def self.calc(x, *r, key: 3, &b) = b ? b.(x) : \"#{x} #{key}\"\nend",
        )];

        assert_eq!(
            compiled(&snippets),
            compiled(&snippets),
            "the same snippets compiled in two fresh interpreters must yield byte-identical \
             bytecode"
        );
    }

    #[test]
    fn bytecode_snippets_are_left_out() {
        let bytecodes = compiled(&[
            Snippet::Bytecode {
                body: vec![0x52, 0x49, 0x54, 0x45],
            },
            source("Only", "1"),
        ])
        .expect("a source snippet beside a bytecode one must compile");

        assert_eq!(
            bytecodes.len(),
            1,
            "compile must answer one bytecode per source snippet and none for a bytecode one"
        );
    }
}
