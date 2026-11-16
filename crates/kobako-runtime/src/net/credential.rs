//! A credential's swap into a request on its way out: the placeholder the
//! guest wrote replaced by the secret, in the one header its binding names, as
//! OpenShell's egress proxy swaps one.

use crate::network::Secret;

/// The scheme an `authorization` header names a basic pair by.
const BASIC: &[u8] = b"Basic ";

/// RFC 4648's standard alphabet, which a basic pair is encoded in.
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, byte)| n | u32::from(*byte) << (16 - 8 * i));
        for i in 0..4 {
            encoded.push(match i <= chunk.len() {
                true => BASE64[(n >> (18 - 6 * i) & 63) as usize],
                false => b'=',
            });
        }
    }
    encoded
}

/// The bytes `text` encodes, `None` for text that is no padded base64.
fn decode_base64(text: &[u8]) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(text.len() / 4 * 3);
    for chunk in text.chunks(4) {
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 {
            return None;
        }
        let mut n = 0u32;
        for (i, byte) in chunk[..4 - padding].iter().enumerate() {
            let value = BASE64.iter().position(|b| b == byte)? as u32;
            n |= value << (18 - 6 * i);
        }
        decoded.extend(n.to_be_bytes()[1..4 - padding].iter());
    }
    Some(decoded)
}

/// One placeholder to replace with a secret in one named header's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Swap {
    pub(crate) header: String,
    pub(crate) placeholder: Vec<u8>,
    pub(crate) secret: Secret,
}

impl Swap {
    /// `value`, the named header's, with the placeholder replaced. A `Basic`
    /// credential carries its pair encoded, so the placeholder is replaced in
    /// the decoded pair, which is encoded again, as OpenShell's does.
    pub(crate) fn apply(&self, name: &str, value: &[u8]) -> Vec<u8> {
        if !name.eq_ignore_ascii_case(&self.header) || self.placeholder.is_empty() {
            return value.to_vec();
        }
        if let Some(pair) =
            basic_pair_of(value).filter(|pair| position_of(&self.placeholder, pair).is_some())
        {
            let mut swapped = BASIC.to_vec();
            swapped.extend(encode_base64(&self.replace(&pair)));
            return swapped;
        }
        self.replace(value)
    }

    /// `value` with its first placeholder replaced by the secret, so a swap
    /// grows a value by one secret at most.
    fn replace(&self, value: &[u8]) -> Vec<u8> {
        match position_of(&self.placeholder, value) {
            Some(at) => [
                &value[..at],
                self.secret.expose(),
                &value[at + self.placeholder.len()..],
            ]
            .concat(),
            None => value.to_vec(),
        }
    }
}

/// How many times a header's `value` carries `placeholder`, as sent or inside
/// the pair a `Basic` value encodes, as OpenShell finds one.
pub(crate) fn occurrences_of(placeholder: &[u8], value: &[u8]) -> usize {
    if placeholder.is_empty() {
        return 0;
    }
    match basic_pair_of(value).map(|pair| count_of(placeholder, &pair)) {
        Some(inside) if inside > 0 => inside,
        _ => count_of(placeholder, value),
    }
}

/// The pair a `Basic` value encodes.
fn basic_pair_of(value: &[u8]) -> Option<Vec<u8>> {
    value
        .get(..BASIC.len())
        .filter(|scheme| scheme.eq_ignore_ascii_case(BASIC))
        .and_then(|_| decode_base64(&value[BASIC.len()..]))
}

fn position_of(part: &[u8], value: &[u8]) -> Option<usize> {
    value.windows(part.len()).position(|window| window == part)
}

/// How many times `part` stands in `value`, each after the last one's end.
fn count_of(part: &[u8], value: &[u8]) -> usize {
    let mut rest = value;
    let mut found = 0;
    while let Some(at) = position_of(part, rest) {
        found += 1;
        rest = &rest[at + part.len()..];
    }
    found
}
