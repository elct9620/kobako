//! Byte primitives for the core envelope, guest side.
//!
//! Free functions over an explicit cursor rather than a reader type: the
//! guest decodes each envelope in one straight-line pass, so the cursor is
//! all the state a pass needs.

use super::Error;

const SHORT: Error = Error("core envelope ended before its declared length");

/// Read one byte and advance.
pub fn take_u8(bytes: &[u8], at: &mut usize) -> Result<u8, Error> {
    let byte = *bytes.get(*at).ok_or(SHORT)?;
    *at += 1;
    Ok(byte)
}

/// Read a big-endian u32 and advance.
pub fn take_u32(bytes: &[u8], at: &mut usize) -> Result<u32, Error> {
    let end = at.checked_add(4).ok_or(SHORT)?;
    let window = bytes.get(*at..end).ok_or(SHORT)?;
    *at = end;
    let mut quad = [0u8; 4];
    quad.copy_from_slice(window);
    Ok(u32::from_be_bytes(quad))
}

/// Read a `u32`-prefixed byte string and advance.
pub fn take_bytes<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a [u8], Error> {
    let len = take_u32(bytes, at)? as usize;
    let end = at.checked_add(len).ok_or(SHORT)?;
    let window = bytes.get(*at..end).ok_or(SHORT)?;
    *at = end;
    Ok(window)
}

/// Read a byte string carrying UTF-8 and advance. Every text field is
/// validated at decode so callers hold `&str` without repeating the check.
pub fn take_text<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a str, Error> {
    core::str::from_utf8(take_bytes(bytes, at)?)
        .map_err(|_| Error("core envelope text field is not valid UTF-8"))
}

/// Read a `u32`-counted list of byte strings and advance. A count the
/// remaining bytes cannot satisfy is refused before allocating, since each
/// element costs at least its own length prefix.
pub fn take_list<'a>(bytes: &'a [u8], at: &mut usize) -> Result<Vec<&'a [u8]>, Error> {
    let count = take_u32(bytes, at)? as usize;
    if count > bytes.len().saturating_sub(*at) {
        return Err(SHORT);
    }
    let mut items = Vec::with_capacity(count);
    while items.len() < count {
        items.push(take_bytes(bytes, at)?);
    }
    Ok(items)
}

/// Read a counted list whose elements carry UTF-8 and advance.
pub fn take_text_list(bytes: &[u8], at: &mut usize) -> Result<Vec<String>, Error> {
    let count = take_u32(bytes, at)? as usize;
    if count > bytes.len().saturating_sub(*at) {
        return Err(SHORT);
    }
    let mut items = Vec::with_capacity(count);
    while items.len() < count {
        items.push(take_text(bytes, at)?.to_owned());
    }
    Ok(items)
}

/// Everything from the cursor to the end — the trailing field of an
/// envelope that carries one.
pub fn rest<'a>(bytes: &'a [u8], at: &usize) -> &'a [u8] {
    &bytes[*at..]
}

/// Refuse anything past the cursor. Used where the last field is
/// self-delimiting and leftovers signal a framing desync.
pub fn expect_end(bytes: &[u8], at: &usize) -> Result<(), Error> {
    if *at == bytes.len() {
        Ok(())
    } else {
        Err(Error("core envelope carries bytes past its last field"))
    }
}

/// Append a big-endian u32.
pub fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Append a `u32`-prefixed byte string.
pub fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

/// Append a `u32`-counted list of byte strings.
pub fn put_list<S: AsRef<[u8]>>(out: &mut Vec<u8>, items: &[S]) {
    put_u32(out, items.len() as u32);
    for item in items {
        put_bytes(out, item.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_primitive() {
        let mut out = vec![7u8];
        put_u32(&mut out, 0xdead_beef);
        put_bytes(&mut out, b"kv");
        put_list(&mut out, &[&b"a"[..], &b"bc"[..]]);
        out.extend_from_slice(b"tail");

        let mut at = 0;
        assert_eq!(
            take_u8(&out, &mut at),
            Ok(7),
            "a u8 must round-trip through the guest writer"
        );
        assert_eq!(
            take_u32(&out, &mut at),
            Ok(0xdead_beef),
            "a u32 must round-trip big-endian"
        );
        assert_eq!(
            take_bytes(&out, &mut at),
            Ok(&b"kv"[..]),
            "a length-prefixed byte string must round-trip"
        );
        assert_eq!(
            take_list(&out, &mut at),
            Ok(vec![&b"a"[..], &b"bc"[..]]),
            "a counted list must round-trip element for element"
        );
        assert_eq!(
            rest(&out, &at),
            b"tail",
            "the trailing field must be everything the prefixed fields left"
        );
    }

    #[test]
    fn u32_is_big_endian_on_the_wire() {
        let mut out = Vec::new();
        put_u32(&mut out, 1);
        assert_eq!(
            out,
            vec![0, 0, 0, 1],
            "a u32 must encode big-endian so both peers read the same number"
        );
    }

    #[test]
    fn a_length_running_past_the_end_is_refused() {
        let bytes = [0, 0, 0, 8, b'h', b'i'];
        let mut at = 0;
        assert!(
            take_bytes(&bytes, &mut at).is_err(),
            "a byte string whose length overruns the message must be rejected, not truncated"
        );
    }

    #[test]
    fn a_count_larger_than_the_message_is_refused() {
        let bytes = [0xff, 0xff, 0xff, 0xff];
        let mut at = 0;
        assert!(
            take_list(&bytes, &mut at).is_err(),
            "a list count the message cannot satisfy must be rejected before any allocation"
        );
    }

    #[test]
    fn non_utf8_in_a_text_field_is_refused() {
        let mut out = Vec::new();
        put_bytes(&mut out, &[0xff, 0xfe]);
        let mut at = 0;
        assert!(
            take_text(&out, &mut at).is_err(),
            "a text field carrying non-UTF-8 bytes must be rejected at decode"
        );
    }

    #[test]
    fn trailing_bytes_after_a_self_delimiting_field_are_refused() {
        let mut out = Vec::new();
        put_bytes(&mut out, b"ok");
        out.push(b'!');
        let mut at = 0;
        take_bytes(&out, &mut at).unwrap();
        assert!(
            expect_end(&out, &at).is_err(),
            "bytes past an envelope's last field must fail loudly as a framing desync"
        );
    }
}
