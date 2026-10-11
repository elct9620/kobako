//! The requests each channel carries: a tag naming the operation, then
//! that operation's body.
//!
//! A tag the reader does not serve reads as `Unserved` rather than as a
//! refusal, so the receiver can answer it as unsupported and the sender
//! can fall back. Only an empty request is a wire violation here; each
//! body's own layout refuses what it cannot frame.

use super::bytes::{Reader, Writer};
use super::{tag, Call, DecodeError, Run};

/// A request the host sends the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Downcall {
    /// Run the user source, UTF-8, as one invocation.
    Eval(Vec<u8>),
    /// Call the entrypoint the Run names.
    Run(Run),
    /// Re-enter the block bound to the active dispatch; the bytes are
    /// its arguments, codec-encoded.
    Yield(Vec<u8>),
    /// A tag this reader does not serve.
    Unserved(u8),
}

impl Downcall {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        let tag = reader.u8()?;
        let body = reader.remaining();
        Ok(match tag {
            tag::EVAL => Downcall::Eval(body.to_vec()),
            tag::RUN => Downcall::Run(Run::decode(body)?),
            tag::YIELD => Downcall::Yield(body.to_vec()),
            other => Downcall::Unserved(other),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Downcall::Eval(source) => writer.u8(tag::EVAL).remainder(source),
            Downcall::Run(run) => writer.u8(tag::RUN).remainder(&run.encode()),
            Downcall::Yield(args) => writer.u8(tag::YIELD).remainder(args),
            Downcall::Unserved(other) => writer.u8(*other),
        };
        writer.into_bytes()
    }
}

/// A request the guest sends the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upcall<'a> {
    /// Dispatch the Call to a Receiver.
    Call(Call<'a>),
    /// A tag this reader does not serve.
    Unserved(u8),
}

impl<'a> Upcall<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        let tag = reader.u8()?;
        Ok(match tag {
            tag::CALL => Upcall::Call(Call::decode(reader.remaining())?),
            other => Upcall::Unserved(other),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Upcall::Call(call) => writer.u8(tag::CALL).remainder(&call.encode()),
            Upcall::Unserved(other) => writer.u8(*other),
        };
        writer.into_bytes()
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
    fn a_tag_this_reader_does_not_serve_reads_as_unserved() {
        assert_eq!(
            (Downcall::decode(&[0x05, 0xff]), Upcall::decode(&[0xf0])),
            (Ok(Downcall::Unserved(0x05)), Ok(Upcall::Unserved(0xf0))),
            "a request under an unserved tag through Downcall and Upcall must read as \
             unserved so the receiver can answer it"
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
}
