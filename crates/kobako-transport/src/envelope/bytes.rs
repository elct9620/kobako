//! Byte primitives for the core envelope: unsigned big-endian integers,
//! length-prefixed byte strings, counted lists, and the trailing remainder.
//!
//! `Reader` borrows rather than copies, so a decoded envelope's payload is
//! a view into the caller's buffer and reaches a frontend without a second
//! allocation.

use super::Error;

const MALFORMED: Error = Error("core envelope ended before its declared length");

/// Cursor over one message. Every read either advances past a complete
/// field or fails, so a truncated message can never be mistaken for a
/// short one.
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        let byte = *self.bytes.get(self.pos).ok_or(MALFORMED)?;
        self.pos += 1;
        Ok(byte)
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        let end = self.pos.checked_add(4).ok_or(MALFORMED)?;
        let slice = self.bytes.get(self.pos..end).ok_or(MALFORMED)?;
        self.pos = end;
        // The slice is exactly 4 bytes, so the conversion cannot fail.
        Ok(u32::from_be_bytes(slice.try_into().unwrap()))
    }

    /// A `u32` length followed by that many bytes.
    pub fn bytes(&mut self) -> Result<&'a [u8], Error> {
        let len = self.u32()? as usize;
        let end = self.pos.checked_add(len).ok_or(MALFORMED)?;
        let slice = self.bytes.get(self.pos..end).ok_or(MALFORMED)?;
        self.pos = end;
        Ok(slice)
    }

    /// A byte string carrying UTF-8. Every text field in the envelope is
    /// validated at decode so a frontend receives `&str` without repeating
    /// the check.
    pub fn text(&mut self) -> Result<&'a str, Error> {
        core::str::from_utf8(self.bytes()?)
            .map_err(|_| Error("core envelope text field is not valid UTF-8"))
    }

    /// A `u32` count followed by that many byte strings, each carrying
    /// UTF-8. Every list the envelope defines is a list of names, so this
    /// is the only list shape a reader needs.
    ///
    /// The elements are copied rather than borrowed: a preamble's paths and
    /// a panic's `available` names outlive the frame buffer they arrive in.
    pub fn text_list(&mut self) -> Result<Vec<String>, Error> {
        let count = self.u32()? as usize;
        // A count larger than the bytes left cannot be satisfied; refusing
        // it here bounds the allocation below by the message size.
        if count > self.remaining().len() {
            return Err(MALFORMED);
        }
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.text()?.to_owned());
        }
        Ok(out)
    }

    /// Everything not yet consumed. The trailing field of an envelope that
    /// carries one, so its extent comes from the transport rather than a
    /// repeated length.
    pub fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.pos..]
    }

    /// Refuse anything left over. Used by envelopes whose last field is
    /// self-delimiting, where trailing bytes signal a framing desync.
    pub fn finish(self) -> Result<(), Error> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(Error("core envelope carries bytes past its last field"))
        }
    }
}

/// Accumulator for one message.
#[derive(Default)]
pub struct Writer {
    out: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Writer::default()
    }

    pub fn u8(&mut self, byte: u8) -> &mut Self {
        self.out.push(byte);
        self
    }

    pub fn u32(&mut self, value: u32) -> &mut Self {
        self.out.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.u32(bytes.len() as u32);
        self.out.extend_from_slice(bytes);
        self
    }

    /// The trailing field: written without a length, since the transport
    /// already carries the message's extent.
    pub fn remainder(&mut self, bytes: &[u8]) -> &mut Self {
        self.out.extend_from_slice(bytes);
        self
    }

    pub fn list<S: AsRef<[u8]>>(&mut self, items: &[S]) -> &mut Self {
        self.u32(items.len() as u32);
        for item in items {
            self.bytes(item.as_ref());
        }
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_primitive() {
        let mut w = Writer::new();
        w.u8(7)
            .u32(0xdead_beef)
            .bytes(b"kv")
            .list(&[&b"a"[..], &b"bc"[..]]);
        w.remainder(b"tail");
        let encoded = w.into_bytes();

        let mut r = Reader::new(&encoded);
        assert_eq!(r.u8(), Ok(7), "a u8 must round-trip through the writer");
        assert_eq!(r.u32(), Ok(0xdead_beef), "a u32 must round-trip big-endian");
        assert_eq!(
            r.bytes(),
            Ok(&b"kv"[..]),
            "a length-prefixed byte string must round-trip"
        );
        assert_eq!(
            r.text_list(),
            Ok(vec!["a".to_owned(), "bc".to_owned()]),
            "a counted list must round-trip element for element"
        );
        assert_eq!(
            r.remaining(),
            b"tail",
            "the trailing field must be everything the prefixed fields left"
        );
    }

    #[test]
    fn u32_is_big_endian_on_the_wire() {
        let mut w = Writer::new();
        w.u32(1);
        assert_eq!(
            w.into_bytes(),
            vec![0, 0, 0, 1],
            "a u32 must encode big-endian so both peers read the same number"
        );
    }

    #[test]
    fn a_length_running_past_the_end_is_refused() {
        // Declares 8 bytes but supplies 2.
        let bytes = [0, 0, 0, 8, b'h', b'i'];
        let mut r = Reader::new(&bytes);
        assert!(
            r.bytes().is_err(),
            "a byte string whose length overruns the message must be rejected, not truncated"
        );
    }

    #[test]
    fn a_count_larger_than_the_message_is_refused() {
        // Declares 0xffff_ffff elements in a 4-byte message.
        let bytes = [0xff, 0xff, 0xff, 0xff];
        let mut r = Reader::new(&bytes);
        assert!(
            r.text_list().is_err(),
            "a list count the message cannot satisfy must be rejected before any allocation"
        );
    }

    #[test]
    fn non_utf8_in_a_text_field_is_refused() {
        let mut w = Writer::new();
        w.bytes(&[0xff, 0xfe]);
        let encoded = w.into_bytes();
        let mut r = Reader::new(&encoded);
        assert!(
            r.text().is_err(),
            "a text field carrying non-UTF-8 bytes must be rejected at decode"
        );
    }

    #[test]
    fn trailing_bytes_after_a_self_delimiting_field_are_refused() {
        let mut w = Writer::new();
        w.bytes(b"ok").remainder(b"!");
        let encoded = w.into_bytes();
        let mut r = Reader::new(&encoded);
        r.bytes().unwrap();
        assert!(
            r.finish().is_err(),
            "bytes past an envelope's last field must fail loudly as a framing desync"
        );
    }
}
