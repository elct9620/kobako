//! The Wire's rules a Ruby frontend asks before it writes a name the guest
//! interns, so both frontends judge a name by the one rule the Envelope tier
//! keeps.

use magnus::{function, prelude::*, Error as MagnusError, RModule, RString};

use kobako_transport::envelope::name::{is_constant_name, is_constant_path, is_reserved};

pub(crate) fn register(kobako: RModule) -> Result<(), MagnusError> {
    kobako.define_singleton_method(
        "constant_name?",
        function!(|name: RString| holds(name, is_constant_name), 1),
    )?;
    kobako.define_singleton_method(
        "constant_path?",
        function!(|path: RString| holds(path, is_constant_path), 1),
    )?;
    kobako.define_singleton_method(
        "reserved_path?",
        function!(|path: RString| holds(path, is_reserved), 1),
    )?;
    Ok(())
}

/// Whether `rule` holds for the text `s` spells; bytes that spell no text
/// are no name at all.
fn holds(s: RString, rule: fn(&str) -> bool) -> bool {
    std::str::from_utf8(&crate::to_bytes(s)).is_ok_and(rule)
}
