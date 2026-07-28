//! The yield position: a block's arguments and answer as value trees.

use kobako_codec::msgpack::codec::{Decoder, Encoder, Value};

use crate::yielder::{YieldError, Yielder};

impl Yielder<'_> {
    /// The bundled codec's spelling of `call_payload`: encode the
    /// positional arguments as one msgpack array and decode what the
    /// block answered.
    pub fn call_values(&mut self, args: &[Value]) -> Result<Value, YieldError> {
        let body = self.call_payload(&encode_args(args)?)?;
        decode_body(&body)
    }
}

/// Positional yield arguments ride as one msgpack array, the same
/// shape the Ruby Yielder encodes.
fn encode_args(args: &[Value]) -> Result<Vec<u8>, YieldError> {
    let mut encoder = Encoder::new();
    encoder
        .write_value(&Value::Array(args.to_vec()))
        .map_err(|err| YieldError::Aborted(format!("yield arguments are not encodable: {err}")))?;
    Ok(encoder.into_bytes())
}

/// Decode a value-carrying arm's payload. The envelope framed it, so a
/// fault here is the codec's — the guest answered with bytes this
/// endpoint's schema cannot read.
fn decode_body(body: &[u8]) -> Result<Value, YieldError> {
    Decoder::new(body)
        .read_only_value()
        .map_err(|err| YieldError::Aborted(format!("malformed Yield Reply payload: {err}")))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use kobako_runtime::error::Trap;
    use kobako_runtime::yielder::Yielder as RawYielder;
    use kobako_transport::envelope::YieldReply;

    use super::*;

    /// A raw yield channel answering from a canned script, recording
    /// what the Yielder sent into the guest.
    struct Scripted {
        responses: VecDeque<Result<Vec<u8>, Trap>>,
        sent: Vec<Vec<u8>>,
    }

    impl RawYielder for Scripted {
        fn yield_block(&mut self, args: &[u8]) -> Result<Vec<u8>, Trap> {
            self.sent.push(args.to_vec());
            self.responses.pop_front().expect("script exhausted")
        }
    }

    fn scripted(replies: Vec<Vec<u8>>) -> Scripted {
        Scripted {
            responses: replies.into_iter().map(Ok).collect(),
            sent: Vec::new(),
        }
    }

    #[test]
    fn call_ships_the_args_as_one_msgpack_array_and_reads_the_answer_back() {
        let reply = YieldReply::Ok(Encoder::encode(&Value::Int(42)).unwrap()).encode();
        let mut channel = scripted(vec![reply]);

        let answer = Yielder::new(&mut channel)
            .call_values(&[Value::Int(21)])
            .unwrap();

        assert_eq!(
            (answer, channel.sent),
            // msgpack fixarray of one element (0x91) holding int 21 (0x15).
            (Value::Int(42), vec![vec![0x91, 0x15]]),
            "positional yield arguments through Yielder::call must ride as one msgpack \
             array, and the block's answer must read back as this schema's value"
        );
    }

    #[test]
    fn an_ok_arm_this_schema_cannot_read_aborts() {
        let mut channel = scripted(vec![YieldReply::Ok(vec![0xc1]).encode()]);

        let answer = Yielder::new(&mut channel).call_values(&[]);

        assert!(
            matches!(answer, Err(YieldError::Aborted(_))),
            "a well-framed reply whose payload this endpoint's schema cannot read must \
             abort the yield rather than answer it"
        );
    }
}
