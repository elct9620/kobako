//! The Call envelope — guest side.
//!
//! The guest builds a Call for every dispatch it initiates, and decodes
//! one only in the reverse-direction tests that pin the layout.

use super::codec::{put_bytes, put_u32, rest, take_text, take_u32, take_u8};
use super::Error;

const KIND_PATH: u8 = 0;
const KIND_HANDLE: u8 = 1;

/// The object a Call is addressed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A bound constant's path — `"MyService::KV"`, or a single segment
    /// like `"File"`.
    Path(String),
    /// A capability Handle, by the id this invocation's table issued.
    Handle(u32),
}

/// One dispatch invitation: where it goes, what to run, whether a block
/// came with it, and the arguments the resolved method consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub target: Target,
    pub method: String,
    pub block_given: bool,
    pub payload: Vec<u8>,
}

impl Call {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match &self.target {
            Target::Path(path) => {
                out.push(KIND_PATH);
                put_bytes(&mut out, path.as_bytes());
            }
            Target::Handle(id) => {
                out.push(KIND_HANDLE);
                put_u32(&mut out, *id);
            }
        }
        put_bytes(&mut out, self.method.as_bytes());
        out.push(u8::from(self.block_given));
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        let target = match take_u8(bytes, &mut at)? {
            KIND_PATH => Target::Path(take_text(bytes, &mut at)?.to_owned()),
            KIND_HANDLE => match take_u32(bytes, &mut at)? {
                0 => return Err(Error("Call target Handle id 0 is the invalid sentinel")),
                id => Target::Handle(id),
            },
            _ => return Err(Error("Call kind must be 0 (path) or 1 (handle)")),
        };
        let method = take_text(bytes, &mut at)?.to_owned();
        let block_given = match take_u8(bytes, &mut at)? {
            0 => false,
            1 => true,
            _ => return Err(Error("Call block_given must be 0 or 1")),
        };
        Ok(Call {
            target,
            method,
            block_given,
            payload: rest(bytes, &at).to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_target_round_trips() {
        let call = Call {
            target: Target::Path("MyService::KV".into()),
            method: "get".into(),
            block_given: false,
            payload: vec![0x92, 0x90, 0x80],
        };
        let encoded = call.encode();
        assert_eq!(
            Call::decode(&encoded),
            Ok(call),
            "a constant-path Call must survive a guest encode and decode unchanged"
        );
    }

    #[test]
    fn a_handle_target_round_trips() {
        let call = Call {
            target: Target::Handle(7),
            method: "commit".into(),
            block_given: true,
            payload: Vec::new(),
        };
        let encoded = call.encode();
        assert_eq!(
            Call::decode(&encoded),
            Ok(call),
            "a Handle-targeted Call must survive a guest encode and decode unchanged"
        );
    }

    #[test]
    fn the_payload_crosses_uninterpreted() {
        let call = Call {
            target: Target::Path("S".into()),
            method: "m".into(),
            block_given: false,
            payload: vec![0xc1, 0xff, 0x00],
        };
        let encoded = call.encode();
        assert_eq!(
            Call::decode(&encoded).map(|c| c.payload),
            Ok(vec![0xc1, 0xff, 0x00]),
            "a payload the envelope cannot parse must still cross unchanged"
        );
    }

    #[test]
    fn golden_layout_pins_field_order() {
        let call = Call {
            target: Target::Path("S".into()),
            method: "m".into(),
            block_given: true,
            payload: vec![0x01],
        };
        assert_eq!(
            call.encode(),
            vec![
                KIND_PATH, //
                0, 0, 0, 1, b'S', //
                0, 0, 0, 1, b'm', //
                1,    //
                0x01, //
            ],
            "the Call byte layout must stay fixed for both peers to agree"
        );
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        assert!(
            Call::decode(&[9, 0, 0, 0, 0]).is_err(),
            "a Call kind that is neither path nor handle must be rejected as a wire violation"
        );
    }

    #[test]
    fn handle_id_zero_is_refused() {
        let mut bytes = vec![KIND_HANDLE];
        put_u32(&mut bytes, 0);
        put_bytes(&mut bytes, b"m");
        bytes.push(0);
        assert!(
            Call::decode(&bytes).is_err(),
            "the invalid-sentinel Handle id must be rejected in the target position"
        );
    }

    #[test]
    fn a_non_boolean_block_flag_is_refused() {
        let mut bytes = vec![KIND_PATH];
        put_bytes(&mut bytes, b"S");
        put_bytes(&mut bytes, b"m");
        bytes.push(2);
        assert!(
            Call::decode(&bytes).is_err(),
            "a block_given byte that is neither 0 nor 1 must be rejected rather than coerced"
        );
    }
}
