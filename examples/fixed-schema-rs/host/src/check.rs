//! The claims this example makes, run as invocations.
//!
//! An example is not covered by the test suite of the repository it
//! lives in, so the claims in its README are only as good as something
//! that runs them. Each case below is one claim, stated as what a guest
//! script ends up with — which is also the only place these contracts
//! are observable: a codec and a dispatch both need a live interpreter,
//! so neither can be reached from a unit test on the host.

use std::sync::Arc;

use kobako::Sandbox;

use crate::store::{ProtoKv, Store};

/// What an invocation must end up with.
enum Expect {
    /// The outcome bytes, exactly. Bytes rather than text because that
    /// is the claim: what the schema carries out is a byte string, and
    /// an expectation written as `&str` could not even spell the case
    /// that matters.
    Ends(&'static [u8]),
    /// The invocation fails, and the failure names this.
    Fails(&'static str),
}

/// One claim and the invocation that settles it.
struct Case {
    claim: &'static str,
    source: &'static str,
    /// Whether the invocation fills the declared Service. A run that
    /// leaves it unfilled is how the fail-closed claim is reached.
    filled: bool,
    expect: Expect,
}

const CASES: &[Case] = &[
    Case {
        claim: "a String outcome reaches the host as its own bytes",
        source: r#""plain\xffbytes""#,
        filled: true,
        expect: Expect::Ends(b"plain\xffbytes"),
    },
    Case {
        claim: "an outcome this schema cannot carry fails the invocation",
        source: "42",
        filled: true,
        expect: Expect::Fails("Kobako::SandboxError"),
    },
    Case {
        claim: "a method no gem defined cannot reach the wire, because the \
                schema that would have to describe it does not exist",
        source: "begin; MyService::KV.frobnicate; rescue NotImplementedError => e; \
                 e.class.to_s; end",
        filled: true,
        expect: Expect::Ends(b"NotImplementedError"),
    },
    Case {
        claim: "that refusal is a missing capability rather than a runtime \
                error, so a bare rescue does not swallow it",
        source: "begin; MyService::KV.frobnicate; rescue => e; \"swallowed\"; end",
        filled: true,
        expect: Expect::Fails("NotImplementedError"),
    },
    Case {
        claim: "a declared Service left unfilled refuses as undefined, and the \
                guest reads that refusal under no schema at all",
        source: r#"
begin
  MyService::KV.get("k")
rescue => e
  e.message.include?("undefined") ? "refused-undefined" : e.message
end
"#,
        filled: false,
        expect: Expect::Ends(b"refused-undefined"),
    },
    Case {
        claim: "a Handle reaches its object as a Call target and as an argument alike",
        // Doubled hashes because the script's own `"#{...}` would
        // otherwise close a single-hash raw string.
        source: r##"
session = MyService::KV.open("tenant/")
session.put("a", "1")
"#{session.get("a")}/#{MyService::KV.count(session)}"
"##,
        filled: true,
        expect: Expect::Ends(b"1/1"),
    },
    Case {
        claim: "a method this gem defines carries a block, and the host yields into it",
        source: r#"
MyService::KV.put("alpha", "1")
MyService::KV.put("beta", "2")
seen = []
MyService::KV.each_key { |key| seen << key; key }
seen.join(",")
"#,
        filled: true,
        expect: Expect::Ends(b"alpha,beta"),
    },
    Case {
        claim: "the rule that governs an outcome governs a block's answer too",
        source: r#"
MyService::KV.put("alpha", "1")
begin
  MyService::KV.each_key { |key| 42 }
rescue => e
  e.class.to_s
end
"#,
        filled: true,
        expect: Expect::Ends(b"RuntimeError"),
    },
    Case {
        claim: "a script cannot construct a Handle it was not handed",
        source: "begin; MyService::Session.new(1); rescue => e; e.class.to_s; end",
        filled: true,
        expect: Expect::Ends(b"NoMethodError"),
    },
    Case {
        claim: "a script cannot re-point a Handle it holds at another id",
        source: r#"
session = MyService::KV.open("tenant/")
begin
  session.instance_variable_set(:@__kv_handle__, 1)
  "re-pointed"
rescue => e
  e.class.to_s
end
"#,
        filled: true,
        expect: Expect::Ends(b"FrozenError"),
    },
];

/// The `#run` claim, which no `Case` can express: it enters at a
/// preloaded entrypoint rather than with a script, and reaches a
/// request-scoped capability and an invocation-scoped one at once.
const ENTRY_CLAIM: &str =
    "an entrypoint reaches a capability handed to it and one bound for the invocation";

/// Run every case, reporting each and failing on the first mismatch.
pub fn run(sandbox: &Sandbox) -> Result<(), String> {
    let mut failed = 0;
    for case in CASES {
        match settle(sandbox, case) {
            Ok(()) => println!("  ok   {}", case.claim),
            Err(detail) => {
                println!("  FAIL {}\n       {detail}", case.claim);
                failed += 1;
            }
        }
    }
    match settle_entry(sandbox) {
        Ok(()) => println!("  ok   {ENTRY_CLAIM}"),
        Err(detail) => {
            println!("  FAIL {ENTRY_CLAIM}\n       {detail}");
            failed += 1;
        }
    }

    let total = CASES.len() + 1;
    if failed > 0 {
        return Err(format!("{failed} of {total} claims did not hold"));
    }
    println!("\n{total} claims hold");
    Ok(())
}

fn settle_entry(sandbox: &Sandbox) -> Result<(), String> {
    let answer = crate::entry::invoke(sandbox, b"ping")?;
    let expected = b"handled=ping scoped=ping shared=ping";
    if answer == expected {
        return Ok(());
    }
    Err(format!(
        "ended on {:?}, expected {:?}",
        String::from_utf8_lossy(&answer),
        String::from_utf8_lossy(expected)
    ))
}

fn settle(sandbox: &Sandbox, case: &Case) -> Result<(), String> {
    let store = Arc::new(Store::default());
    let execution = sandbox
        .eval_with(case.source, |ctx| {
            if case.filled {
                ctx.bind(crate::KV_PATH, Arc::new(ProtoKv(store.clone())))?;
            }
            Ok(())
        })
        .map_err(|err| format!("the invocation did not start: {err}"))?;

    match (execution.payload(), &case.expect) {
        (Ok(bytes), Expect::Ends(expected)) if bytes == *expected => Ok(()),
        (Ok(bytes), Expect::Ends(expected)) => Err(format!(
            "ended on {:?}, expected {:?}",
            String::from_utf8_lossy(bytes),
            String::from_utf8_lossy(expected)
        )),
        (Err(failure), Expect::Fails(expected)) => {
            let reported = failure.to_string();
            if reported.contains(expected) {
                Ok(())
            } else {
                Err(format!("failed with {reported:?}, expected {expected:?}"))
            }
        }
        (Ok(bytes), Expect::Fails(expected)) => Err(format!(
            "ended on {:?}, expected a failure naming {expected:?}",
            String::from_utf8_lossy(bytes)
        )),
        (Err(failure), Expect::Ends(expected)) => Err(format!(
            "failed with {failure}, expected {:?}",
            String::from_utf8_lossy(expected)
        )),
    }
}
