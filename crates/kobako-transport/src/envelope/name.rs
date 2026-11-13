//! The form every name a guest interns takes: a Snippet's name, a Run's
//! entrypoint, and each segment of a Binding's path. The guest reaches each
//! one as a Ruby constant, so every side refuses one outside that form
//! before it is written.

/// The module the guest keeps for itself, where no Binding may stand.
pub const RESERVED_NAMESPACE: &str = "Kobako";

/// The guest interns every constant name, and mruby holds a symbol of at
/// most this many bytes.
const MAX_SYMBOL_LEN: usize = 65_534;

/// Whether `name` is a Ruby constant name the guest can intern: an ASCII
/// capital, then ASCII letters, digits, or `_`, within mruby's symbol length.
pub fn is_constant_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_SYMBOL_LEN
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `path` is constant names joined by `::`.
pub fn is_constant_path(path: &str) -> bool {
    path.split("::").all(is_constant_name)
}

/// Whether `path` stands in the guest's own `RESERVED_NAMESPACE`.
pub fn is_reserved(path: &str) -> bool {
    path.split("::").next() == Some(RESERVED_NAMESPACE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constant_name_is_an_ascii_capital_then_word_characters_within_a_symbol() {
        let longest = format!("A{}", "b".repeat(MAX_SYMBOL_LEN - 1));
        assert_eq!(
            [
                is_constant_name(&longest),
                is_constant_name(&format!("{longest}b")),
                is_constant_name("Aé"),
                is_constant_name("A\n"),
                is_constant_name("lower"),
                is_constant_name(""),
            ],
            [true, false, false, false, false, false],
            "is_constant_name must take an ASCII capital and word characters up to 65534 bytes, \
             and refuse a longer name, a non-ASCII or newline character, a lowercase start, \
             and an empty name"
        );
    }

    #[test]
    fn a_path_is_constant_names_joined_and_the_guests_own_namespace_is_reserved() {
        assert_eq!(
            [
                is_constant_path("MyService::KV"),
                is_constant_path("MyService::"),
                is_constant_path("MyService:KV"),
                is_reserved("Kobako"),
                is_reserved("Kobako::KV"),
                is_reserved("KobakoKV"),
            ],
            [true, false, false, true, true, false],
            "is_constant_path must join constant names by '::' only, and is_reserved must hold \
             a path whose first segment is Kobako and no other"
        );
    }
}
