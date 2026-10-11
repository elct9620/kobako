//! The requests each channel carries: a tag naming the operation, then
//! that operation's body.
//!
//! A tag outside this registry reads as `Unknown` with its body intact,
//! so a receiver can serve it or answer it as unsupported. An empty
//! request and the invalid tag are wire violations; each body's own
//! layout refuses what it cannot frame.

use super::bytes::{Reader, Writer};
use super::{tag, Call, DecodeError, Run};

/// A request the host sends the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Downcall {
    /// Run the user source, UTF-8, as one invocation.
    Eval(Vec<u8>),
    /// Call the entrypoint the Run names.
    Run(Run),
    /// Re-enter the block bound to the active dispatch; the bytes are
    /// its arguments, codec-encoded.
    Yield(Vec<u8>),
    /// A tag outside this registry, with the body it carried.
    Unknown { tag: u8, body: Vec<u8> },
}

impl Downcall {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        let tag = read_tag(&mut reader)?;
        let body = reader.remaining();
        Ok(match tag {
            tag::EVAL => Downcall::Eval(body.to_vec()),
            tag::RUN => Downcall::Run(Run::decode(body)?),
            tag::YIELD => Downcall::Yield(body.to_vec()),
            tag => Downcall::Unknown {
                tag,
                body: body.to_vec(),
            },
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Downcall::Eval(source) => writer.u8(tag::EVAL).remainder(source),
            Downcall::Run(run) => writer.u8(tag::RUN).remainder(&run.encode()),
            Downcall::Yield(args) => writer.u8(tag::YIELD).remainder(args),
            Downcall::Unknown { tag, body } => writer.u8(*tag).remainder(body),
        };
        writer.into_bytes()
    }
}

/// A request the guest sends the host.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Upcall<'a> {
    /// Dispatch the Call to a Receiver.
    Call(Call<'a>),
    /// A tag outside this registry, with the body it carried.
    Unknown { tag: u8, body: &'a [u8] },
}

impl<'a> Upcall<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        let tag = read_tag(&mut reader)?;
        let body = reader.remaining();
        Ok(match tag {
            tag::CALL => Upcall::Call(Call::decode(body)?),
            tag => Upcall::Unknown { tag, body },
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Upcall::Call(call) => writer.u8(tag::CALL).remainder(&call.encode()),
            Upcall::Unknown { tag, body } => writer.u8(*tag).remainder(body),
        };
        writer.into_bytes()
    }
}

fn read_tag(reader: &mut Reader<'_>) -> Result<u8, DecodeError> {
    match reader.u8()? {
        tag::INVALID => Err(DecodeError::new("request tag 0x00 is invalid")),
        tag => Ok(tag),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Target;

    // @behavior WE-071
    #[test]
    fn golden_layout_pins_each_request_tag() {
        let run = Run {
            entrypoint: "M".into(),
            payload: vec![0x90],
        };
        let call = Call {
            target: Target::Path("K"),
            method: "m",
            block_given: false,
            payload: &[0x90],
        };
        assert_eq!(
            (
                Downcall::Eval(b"1".to_vec()).encode(),
                Downcall::Run(run).encode(),
                Downcall::Yield(vec![0x90]).encode(),
                Upcall::Call(call).encode(),
            ),
            (
                vec![0x01, b'1'],
                vec![0x02, 0, 0, 0, 1, b'M', 0x90],
                vec![0x03, 0x90],
                vec![
                    0x04, // tag: call
                    0,    // kind: path
                    0, 0, 0, 1, b'K', // target
                    0, 0, 0, 1, b'm', // method
                    0,    // block_given
                    0x90, // payload
                ],
            ),
            "each request through Downcall and Upcall must open with its operation's tag \
             followed by its body alone"
        );
    }

    #[test]
    fn every_downcall_round_trips() {
        for request in [
            Downcall::Eval(b"1 + 1".to_vec()),
            Downcall::Run(Run {
                entrypoint: "Main".into(),
                payload: vec![0x90],
            }),
            Downcall::Yield(vec![0x91, 0x01]),
            Downcall::Unknown {
                tag: 0xf0,
                body: vec![0xaa, 0xbb],
            },
        ] {
            assert_eq!(
                Downcall::decode(&request.encode()),
                Ok(request),
                "a downcall request must survive an encode and decode unchanged"
            );
        }
    }

    // @behavior WE-072
    #[test]
    fn a_tag_outside_the_registry_reads_as_unknown_with_its_body() {
        assert_eq!(
            (
                Downcall::decode(&[0x05, 0xff]),
                Upcall::decode(&[0xf0, 0x01, 0x02])
            ),
            (
                Ok(Downcall::Unknown {
                    tag: 0x05,
                    body: vec![0xff]
                }),
                Ok(Upcall::Unknown {
                    tag: 0xf0,
                    body: &[0x01, 0x02]
                })
            ),
            "a request under a tag outside the registry through Downcall and Upcall must \
             read as unknown with its body intact"
        );
    }

    // @behavior WE-073
    #[test]
    fn an_empty_request_is_refused() {
        assert!(
            Downcall::decode(&[]).is_err() && Upcall::decode(&[]).is_err(),
            "a request of no length through Downcall and Upcall must be refused"
        );
    }

    // @behavior WE-074
    #[test]
    fn a_request_under_the_invalid_tag_is_refused() {
        assert!(
            Downcall::decode(&[0x00, 0x01]).is_err() && Upcall::decode(&[0x00]).is_err(),
            "a request under tag 0x00 through Downcall and Upcall must be refused"
        );
    }
}
