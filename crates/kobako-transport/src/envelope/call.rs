//! The Call envelope — a message inviting the other side to run one
//! method and answer.
//!
//! Borrows from the buffer it decoded, so the payload reaches a frontend
//! as a view rather than a copy.

use super::bytes::{Reader, Writer};
use super::Error;

/// Whether `target` names a bound constant or a capability Handle. The tag
/// is explicit so a side reads the routing fields without interpreting any
/// encoding but this one.
const KIND_PATH: u8 = 0;
const KIND_HANDLE: u8 = 1;

/// The object a Call is addressed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// A bound constant's path — `"MyService::KV"`, or a single segment
    /// like `"File"`.
    Path(&'a str),
    /// A capability Handle, by the id this invocation's table issued.
    Handle(u32),
}

/// One dispatch invitation: where it goes, what to run, whether a block
/// came with it, and the arguments the resolved method consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call<'a> {
    pub target: Target<'a>,
    pub method: &'a str,
    pub block_given: bool,
    pub payload: &'a [u8],
}

impl<'a> Call<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        let target = match reader.u8()? {
            KIND_PATH => Target::Path(reader.text()?),
            KIND_HANDLE => Target::Handle(reader.u32()?),
            _ => return Err(Error("Call kind must be 0 (path) or 1 (handle)")),
        };
        if let Target::Handle(0) = target {
            return Err(Error("Call target Handle id 0 is the invalid sentinel"));
        }
        let method = reader.text()?;
        let block_given = match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error("Call block_given must be 0 or 1")),
        };
        Ok(Call {
            target,
            method,
            block_given,
            payload: reader.remaining(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self.target {
            Target::Path(path) => {
                writer.u8(KIND_PATH).bytes(path.as_bytes());
            }
            Target::Handle(id) => {
                writer.u8(KIND_HANDLE).u32(id);
            }
        }
        writer
            .bytes(self.method.as_bytes())
            .u8(u8::from(self.block_given))
            .remainder(self.payload);
        writer.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_target_round_trips() {
        let call = Call {
            target: Target::Path("MyService::KV"),
            method: "get",
            block_given: false,
            payload: b"\x92\x90\x80",
        };
        let encoded = call.encode();
        assert_eq!(
            Call::decode(&encoded),
            Ok(call),
            "a constant-path Call must survive a host encode and decode unchanged"
        );
    }

    #[test]
    fn a_handle_target_round_trips() {
        let call = Call {
            target: Target::Handle(7),
            method: "commit",
            block_given: true,
            payload: b"",
        };
        let encoded = call.encode();
        assert_eq!(
            Call::decode(&encoded),
            Ok(call),
            "a Handle-targeted Call must survive a host encode and decode unchanged"
        );
    }

    #[test]
    fn the_payload_is_borrowed_not_interpreted() {
        // Bytes that are not valid msgpack: the envelope layer must carry
        // them through untouched, since the payload's encoding is not its
        // concern.
        let call = Call {
            target: Target::Path("S"),
            method: "m",
            block_given: false,
            payload: &[0xc1, 0xff, 0x00],
        };
        let encoded = call.encode();
        assert_eq!(
            Call::decode(&encoded).map(|c| c.payload.to_vec()),
            Ok(vec![0xc1, 0xff, 0x00]),
            "a payload the envelope cannot parse must still cross unchanged"
        );
    }

    #[test]
    fn golden_layout_pins_the_path_kind_and_field_order() {
        let call = Call {
            target: Target::Path("S"),
            method: "m",
            block_given: true,
            payload: b"\x01",
        };
        assert_eq!(
            call.encode(),
            vec![
                0, // kind: path
                0, 0, 0, 1, b'S', // target
                0, 0, 0, 1, b'm', // method
                1,    // block_given
                0x01, // payload remainder
            ],
            "a constant-path Call must encode as kind byte 0 followed by target, method, \
             block flag, and payload in that order"
        );
    }

    #[test]
    fn golden_layout_pins_the_handle_kind_and_its_bare_id() {
        let call = Call {
            target: Target::Handle(0x0102_0304),
            method: "m",
            block_given: false,
            payload: b"",
        };
        assert_eq!(
            call.encode(),
            vec![
                1, // kind: handle
                1, 2, 3, 4, // target: the id alone, with no length prefix
                0, 0, 0, 1, b'm', // method
                0,    // block_given
            ],
            "a Handle-targeted Call must encode as kind byte 1 followed by the bare \
             big-endian id, not a length-prefixed byte string"
        );
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        let bytes = [9u8, 0, 0, 0, 0];
        assert!(
            Call::decode(&bytes).is_err(),
            "a Call kind that is neither path nor handle must be rejected as a wire violation"
        );
    }

    #[test]
    fn handle_id_zero_is_refused() {
        let call_bytes = {
            let mut w = Writer::new();
            w.u8(KIND_HANDLE).u32(0).bytes(b"m").u8(0);
            w.into_bytes()
        };
        assert!(
            Call::decode(&call_bytes).is_err(),
            "the invalid-sentinel Handle id must be rejected in the target position"
        );
    }

    #[test]
    fn a_non_boolean_block_flag_is_refused() {
        let bytes = {
            let mut w = Writer::new();
            w.u8(KIND_PATH).bytes(b"S").bytes(b"m").u8(2);
            w.into_bytes()
        };
        assert!(
            Call::decode(&bytes).is_err(),
            "a block_given byte that is neither 0 nor 1 must be rejected rather than coerced"
        );
    }

    #[test]
    fn a_truncated_call_is_refused() {
        let bytes = [KIND_PATH, 0, 0, 0, 4, b'a'];
        assert!(
            Call::decode(&bytes).is_err(),
            "a Call whose target length overruns the message must be rejected, not truncated"
        );
    }
}
