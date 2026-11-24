//! Object names, the reserved form and draft names (2.1, 2.4).
//!
//! Every test is on the whole string: nothing may come before or after the name, not even a line end.

/// Number of hexadecimal digits of the hash in an object name of TOA 0.1.
pub const HASH_LEN: usize = 40;
/// Number of hexadecimal digits of the form reserved for a later hash algorithm.
pub const RESERVED_HASH_LEN: usize = 64;
/// Longest object name: a prefix of 8, the underscore and 40 digits.
pub const MAX_NAME_LEN: usize = 8 + 1 + HASH_LEN;

/// What form a string has. At most one form applies to any string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `prefix_` and 40 lowercase hexadecimal digits (2.1).
    Object,
    /// `prefix_` and 64 lowercase hexadecimal digits: not an object name in 0.1 (2.1).
    Reserved,
    /// `prefix_draft-tag` (2.4).
    Draft,
    /// Anything else: an ordinary string.
    Other,
}

/// A prefix: a lowercase letter, then 1 to 7 lowercase letters or digits (2.1).
pub fn is_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=8).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// A draft tag: 1 to 40 lowercase letters, digits or hyphens, starting with a letter or digit (2.4).
pub fn is_tag(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=40).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// The form of a whole string.
pub fn form(s: &str) -> Form {
    let Some((prefix, rest)) = s.split_once('_') else {
        return Form::Other;
    };
    if !is_prefix(prefix) {
        return Form::Other;
    }
    if is_lower_hex(rest, HASH_LEN) {
        Form::Object
    } else if is_lower_hex(rest, RESERVED_HASH_LEN) {
        Form::Reserved
    } else if rest.strip_prefix("draft-").is_some_and(is_tag) {
        Form::Draft
    } else {
        Form::Other
    }
}

/// Whether the whole string is an object name (2.1).
pub fn is_object_name(s: &str) -> bool {
    form(s) == Form::Object
}

/// Whether the whole string has the reserved form (2.1).
pub fn is_reserved(s: &str) -> bool {
    form(s) == Form::Reserved
}

/// Whether the whole string is a draft name (2.4).
pub fn is_draft_name(s: &str) -> bool {
    form(s) == Form::Draft
}

/// The prefix and the hash of an object name, or `None` when the string is not one.
pub fn split(name: &str) -> Option<(&str, &str)> {
    if is_object_name(name) {
        name.split_once('_')
    } else {
        None
    }
}

/// The prefix of an object name or a draft name.
pub fn prefix_of(name: &str) -> Option<&str> {
    match form(name) {
        Form::Object | Form::Draft | Form::Reserved => name.split_once('_').map(|p| p.0),
        Form::Other => None,
    }
}

/// The object name of a prefix and a hash.
pub fn object_name(prefix: &str, hash: &str) -> String {
    format!("{prefix}_{hash}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "ea44863a8d2f242bc651caf709d45a27e7531d9d";

    #[test]
    fn forms() {
        assert_eq!(form(&format!("ip_{H}")), Form::Object);
        assert_eq!(form(&format!("ip_{H}\n")), Form::Other);
        assert_eq!(form("tb_ac"), Form::Other);
        assert_eq!(form("imp_draft-x-"), Form::Draft);
        assert_eq!(form("imp_draft-a.b"), Form::Other);
        assert_eq!(form(&format!("ip_{}", "a".repeat(64))), Form::Reserved);
        assert_eq!(split(&format!("imp_{H}")), Some(("imp", H)));
        assert_eq!(prefix_of("ver_draft-a"), Some("ver"));
    }
}
