//! Byte oracle between the two core-envelope implementations.
//!
//! `kobako-runtime` encodes for the host, `kobako-codec` for the guest.
//! Neither was derived from the other, so this is what keeps them from
//! drifting: each side's bytes must decode on the other into the same
//! message, in both directions and on every arm the layout defines.
//!
//! The layer's own unit tests pin each side against its golden vectors;
//! this pins the two sides against each other.

use kobako_codec::envelope as guest;
use kobako_runtime::envelope as host;

/// Bytes both sides must agree carry no meaning at this layer — the shape
/// of a payload is the adapter's business, so the oracle deliberately uses
/// bytes no MessagePack decoder would accept.
const OPAQUE: &[u8] = &[0xc1, 0x00, 0xff, 0x92];

#[test]
fn a_path_call_crosses_in_both_directions() {
    let host_call = host::Call {
        target: host::Target::Path("MyService::KV"),
        method: "get",
        block_given: false,
        payload: OPAQUE,
    };
    let guest_call = guest::Call {
        target: guest::Target::Path("MyService::KV".into()),
        method: "get".into(),
        block_given: false,
        payload: OPAQUE.to_vec(),
    };

    assert_eq!(
        host_call.encode(),
        guest_call.encode(),
        "a constant-path Call encoded by either peer must produce the same bytes"
    );
    assert_eq!(
        guest::Call::decode(&host_call.encode()),
        Ok(guest_call.clone()),
        "a Call the host encoded must decode on the guest into the same message"
    );
    assert_eq!(
        host::Call::decode(&guest_call.encode()),
        Ok(host_call),
        "a Call the guest encoded must decode on the host into the same message"
    );
}

#[test]
fn a_handle_call_crosses_in_both_directions() {
    let host_call = host::Call {
        target: host::Target::Handle(0x7fff_ffff),
        method: "commit",
        block_given: true,
        payload: &[],
    };
    let guest_call = guest::Call {
        target: guest::Target::Handle(0x7fff_ffff),
        method: "commit".into(),
        block_given: true,
        payload: Vec::new(),
    };

    assert_eq!(
        host_call.encode(),
        guest_call.encode(),
        "a Handle-targeted Call at the id cap must encode identically on both peers"
    );
    assert_eq!(
        guest::Call::decode(&host_call.encode()),
        Ok(guest_call),
        "a Handle-targeted Call must cross host to guest unchanged"
    );
}

#[test]
fn both_reply_arms_cross_in_both_directions() {
    let arms: [(host::Reply, guest::Reply); 2] = [
        (
            host::Reply::Ok(OPAQUE.to_vec()),
            guest::Reply::Ok(OPAQUE.to_vec()),
        ),
        (
            host::Reply::Fault(OPAQUE.to_vec()),
            guest::Reply::Fault(OPAQUE.to_vec()),
        ),
    ];
    for (host_reply, guest_reply) in arms {
        assert_eq!(
            host_reply.encode(),
            guest_reply.encode(),
            "a Reply must encode identically on both peers, arm for arm"
        );
        assert_eq!(
            guest::Reply::decode(&host_reply.encode()),
            Ok(guest_reply.clone()),
            "a Reply the host encoded must decode on the guest into the same arm"
        );
        assert_eq!(
            host::Reply::decode(&guest_reply.encode()),
            Ok(host_reply),
            "a Reply the guest encoded must decode on the host into the same arm"
        );
    }
}

#[test]
fn every_yield_reply_arm_crosses_in_both_directions() {
    let host_error = host::ErrorRecord {
        class: "LocalJumpError".into(),
        message: "no block given".into(),
        backtrace: vec!["(eval):1".into(), "(snippet:Helper):3".into()],
    };
    let guest_error = guest::ErrorRecord {
        class: "LocalJumpError".into(),
        message: "no block given".into(),
        backtrace: vec!["(eval):1".into(), "(snippet:Helper):3".into()],
    };

    let arms: [(host::YieldReply, guest::YieldReply); 3] = [
        (
            host::YieldReply::Ok(OPAQUE.to_vec()),
            guest::YieldReply::Ok(OPAQUE.to_vec()),
        ),
        (
            host::YieldReply::Break(OPAQUE.to_vec()),
            guest::YieldReply::Break(OPAQUE.to_vec()),
        ),
        (
            host::YieldReply::Error(host_error),
            guest::YieldReply::Error(guest_error),
        ),
    ];
    for (host_reply, guest_reply) in arms {
        assert_eq!(
            host_reply.encode(),
            guest_reply.encode(),
            "a Yield Reply must encode identically on both peers, arm for arm"
        );
        assert_eq!(
            guest::YieldReply::decode(&host_reply.encode()),
            Ok(guest_reply.clone()),
            "a Yield Reply the host encoded must decode on the guest into the same arm"
        );
        assert_eq!(
            host::YieldReply::decode(&guest_reply.encode()),
            Ok(host_reply),
            "a Yield Reply the guest encoded must decode on the host into the same arm"
        );
    }
}

#[test]
fn an_outcome_result_crosses_in_both_directions() {
    let host_outcome = host::Outcome::Result(OPAQUE.to_vec());
    let guest_outcome = guest::Outcome::Result(OPAQUE.to_vec());

    assert_eq!(
        host_outcome.encode(),
        guest_outcome.encode(),
        "a Result Outcome must encode identically on both peers"
    );
    assert_eq!(
        host::Outcome::decode(&guest_outcome.encode()),
        Ok(host_outcome),
        "the outcome the guest writes must decode on the host into the same message"
    );
}

#[test]
fn a_panic_crosses_with_its_attribution_intact() {
    let guest_panic = guest::Outcome::Panic(guest::Panic {
        origin: guest::ORIGIN_SERVICE.into(),
        error: guest::ErrorRecord {
            class: "Kobako::ServiceError".into(),
            message: "boom".into(),
            backtrace: vec!["(eval):2".into()],
        },
        details: OPAQUE.to_vec(),
    });

    match host::Outcome::decode(&guest_panic.encode()) {
        Ok(host::Outcome::Panic(panic)) => {
            assert!(
                panic.from_service(),
                "a guest-written service panic must attribute to the Service on the host"
            );
            assert_eq!(
                panic.error.class, "Kobako::ServiceError",
                "the Error Record must cross with its class intact"
            );
            assert_eq!(
                panic.details, OPAQUE,
                "panic details must cross as opaque bytes the envelope never parses"
            );
        }
        other => panic!("expected a Panic, got {other:?}"),
    }
}

#[test]
fn a_run_crosses_from_host_to_guest() {
    let host_run = host::Run {
        entrypoint: "Entry".into(),
        payload: OPAQUE.to_vec(),
    };
    let guest_run = guest::Run {
        entrypoint: "Entry".into(),
        payload: OPAQUE.to_vec(),
    };

    assert_eq!(
        host_run.encode(),
        guest_run.encode(),
        "a Run must encode identically on both peers"
    );
    assert_eq!(
        guest::Run::decode(&host_run.encode()),
        Ok(guest_run),
        "the Run the host writes to the command buffer must decode on the guest unchanged"
    );
}

#[test]
fn both_invocation_frames_cross_from_host_to_guest() {
    let host_preamble = host::Preamble {
        paths: vec!["MyService::KV".into(), "File".into()],
    };
    let guest_preamble = guest::Preamble {
        paths: vec!["MyService::KV".into(), "File".into()],
    };
    assert_eq!(
        guest::Preamble::decode(&host_preamble.encode()),
        Ok(guest_preamble),
        "Frame 1 must decode on the guest into the paths the host bound"
    );

    let host_snippets = host::Snippets {
        entries: vec![
            host::Snippet::Source {
                name: "Helper".into(),
                body: "def helper; end".into(),
            },
            host::Snippet::Bytecode {
                body: vec![0x52, 0x49, 0x54, 0x45, 0x30, 0x30, 0x30, 0x36],
            },
        ],
    };
    let guest_snippets = guest::Snippets {
        entries: vec![
            guest::Snippet::Source {
                name: "Helper".into(),
                body: "def helper; end".into(),
            },
            guest::Snippet::Bytecode {
                body: vec![0x52, 0x49, 0x54, 0x45, 0x30, 0x30, 0x30, 0x36],
            },
        ],
    };
    assert_eq!(
        guest::Snippets::decode(&host_snippets.encode()),
        Ok(guest_snippets),
        "Frame 3 must decode on the guest into the snippets the host preloaded, in order"
    );
}

#[test]
fn both_peers_refuse_an_unknown_call_kind() {
    let bytes: &[u8] = &[9, 0, 0, 0, 0];
    assert!(
        host::Call::decode(bytes).is_err() && guest::Call::decode(bytes).is_err(),
        "a Call kind that is neither path nor handle must be refused by both peers"
    );
}

#[test]
fn both_peers_refuse_a_zero_length_reply() {
    assert!(
        host::Reply::decode(&[]).is_err() && guest::Reply::decode(&[]).is_err(),
        "a Reply carrying not even a tag must be refused by both peers"
    );
}

#[test]
fn both_peers_refuse_the_reserved_yield_reply_tag() {
    let bytes: &[u8] = &[0x03];
    assert!(
        host::YieldReply::decode(bytes).is_err() && guest::YieldReply::decode(bytes).is_err(),
        "the reserved 0x03 Yield Reply tag must be refused by both peers"
    );
}

#[test]
fn both_peers_refuse_an_unknown_outcome_tag() {
    let bytes: &[u8] = &[0x7f, 0x00];
    assert!(
        host::Outcome::decode(bytes).is_err() && guest::Outcome::decode(bytes).is_err(),
        "an Outcome tag that is neither result nor panic must be refused by both peers"
    );
}
