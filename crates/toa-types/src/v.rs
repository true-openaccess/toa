//! Small checks of values, the shapes the type specifications ask for (`spec/types/*.schema.json`),
//! without a JSON Schema engine.

use toa_core::manifest::{Node, Value};
use toa_core::name::{self, Form};
use toa_core::Problem;

/// Problems of one object, each at `<file>:<keys and indexes>`.
pub(crate) struct V<'a> {
    pub out: &'a mut Vec<Problem>,
    /// `types/ckt` and the like.
    pub section: &'static str,
    /// `toa.yaml` or `lock.json`.
    pub file: &'static str,
}

impl V<'_> {
    pub fn err(&mut self, code: &'static str, at: &str, msg: impl Into<String>) {
        let path = if at.is_empty() { self.file.to_string() } else { format!("{}:{at}", self.file) };
        self.out.push(Problem::error(code, self.section, path, msg));
    }

    /// A mapping with only `allowed` keys and every `required` one. False when it is no mapping.
    pub fn fields(&mut self, n: &Node, at: &str, allowed: &[&str], required: &[&str]) -> bool {
        let Some(m) = n.as_map() else {
            self.err("E_FIELD", at, format!("{} is a mapping", shown(at)));
            return false;
        };
        for (k, _) in m {
            if !allowed.contains(&k.text.as_str()) {
                self.err("E_FIELD_UNKNOWN", &join(at, &k.text), format!("'{}' is not a field here", k.text));
            }
        }
        for r in required {
            if n.get(r).is_none() {
                self.err("E_FIELD_MISSING", &join(at, r), format!("{r} is required"));
            }
        }
        true
    }

    /// A string; the problem when it is not.
    pub fn string<'n>(&mut self, n: &'n Node, at: &str) -> Option<&'n str> {
        let s = n.as_str();
        if s.is_none() {
            self.err("E_FIELD", at, format!("{} is a string", shown(at)));
        }
        s
    }

    /// A string of a given shape.
    pub fn shaped(&mut self, n: &Node, at: &str, ok: fn(&str) -> bool, what: &str) -> Option<String> {
        let s = self.string(n, at)?;
        if !ok(s) {
            self.err("E_FIELD", at, format!("{} is {what}", shown(at)));
            return None;
        }
        Some(s.to_string())
    }

    /// An object name (or, before sealing, a draft name) with this prefix.
    pub fn name(&mut self, n: &Node, at: &str, prefix: &str) -> Option<String> {
        let s = self.string(n, at)?;
        let ok = matches!(name::form(s), Form::Object | Form::Draft) && name::prefix_of(s) == Some(prefix);
        if !ok {
            self.err("E_FIELD", at, format!("{} is an object name with the prefix {prefix}", shown(at)));
            return None;
        }
        Some(s.to_string())
    }

    pub fn number(&mut self, n: &Node, at: &str) -> Option<f64> {
        let x = number(n);
        if x.is_none() {
            self.err("E_FIELD", at, format!("{} is a number", shown(at)));
        }
        x
    }

    /// A whole number from `min` to `max`.
    pub fn int(&mut self, n: &Node, at: &str, min: i64, max: i64) -> Option<i64> {
        match n.value {
            Value::Int(i) if (min..=max).contains(&i) => Some(i),
            _ => {
                let range = if max == i64::MAX { format!("{min} or more") } else { format!("from {min} to {max}") };
                self.err("E_FIELD", at, format!("{} is a whole number, {range}", shown(at)));
                None
            }
        }
    }

    /// A list; the problem when it is not, or is shorter than `min`.
    pub fn list<'n>(&mut self, n: &'n Node, at: &str, min: usize) -> Option<&'n [Node]> {
        match n.as_seq() {
            Some(v) if v.len() >= min => Some(v),
            _ => {
                let what = if min > 0 { format!("a list of at least {min}") } else { "a list".into() };
                self.err("E_FIELD", at, format!("{} is {what}", shown(at)));
                None
            }
        }
    }

    pub fn one_of(&mut self, n: &Node, at: &str, values: &[&str]) {
        if !n.as_str().is_some_and(|s| values.contains(&s)) {
            self.err("E_FIELD", at, format!("{} is one of {}", shown(at), values.join(", ")));
        }
    }

    /// A number, string or boolean.
    pub fn scalar(&mut self, n: &Node, at: &str) {
        if !matches!(n.value, Value::Int(_) | Value::Float(_) | Value::Str(_) | Value::Bool(_)) {
            self.err("E_FIELD", at, format!("{} is a number, string or boolean", shown(at)));
        }
    }
}

pub(crate) fn number(n: &Node) -> Option<f64> {
    match n.value {
        Value::Int(i) => Some(i as f64),
        Value::Float(f) => Some(f),
        _ => None,
    }
}

pub(crate) fn join(at: &str, key: &str) -> String {
    if at.is_empty() {
        key.to_string()
    } else {
        format!("{at}/{key}")
    }
}

fn shown(at: &str) -> &str {
    if at.is_empty() {
        "the manifest"
    } else {
        at
    }
}

// ------------------------------------------------------------------ shapes

/// `first` then `rest` characters, with `min..=max` of them: `[a-z][a-z0-9_-]{1,39}` is
/// `word(s, az, az09_dash, 1, 39)`.
pub(crate) fn word(s: &str, first: fn(char) -> bool, rest: fn(char) -> bool, min: usize, max: usize) -> bool {
    let mut c = s.chars();
    let Some(f) = c.next() else { return false };
    let n = c.clone().count();
    first(f) && (min..=max).contains(&n) && c.all(rest)
}

pub(crate) fn lower(c: char) -> bool {
    c.is_ascii_lowercase()
}
pub(crate) fn letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}
pub(crate) fn lower_digit(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}
pub(crate) fn lower_digit_us_dash(c: char) -> bool {
    lower_digit(c) || c == '_' || c == '-'
}
pub(crate) fn lower_digit_us(c: char) -> bool {
    lower_digit(c) || c == '_'
}
pub(crate) fn alnum_us(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
pub(crate) fn letter_us(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// A short name of a process: `[a-z][a-z0-9_-]{0,39}`.
pub(crate) fn process_name(s: &str) -> bool {
    word(s, lower, lower_digit_us_dash, 0, 39)
}

/// A path of a file in an object: 1 to 7 levels of `[A-Za-z0-9_][A-Za-z0-9._-]{0,63}`, at most 80
/// characters.
pub(crate) fn file_path(s: &str) -> bool {
    let levels: Vec<&str> = s.split('/').collect();
    s.len() <= 80
        && (1..=7).contains(&levels.len())
        && levels
            .iter()
            .all(|l| word(l, alnum_us, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'), 0, 63))
}

/// The name of a testbench: `tb_[a-z0-9_]{1,36}`.
pub(crate) fn tb_name(s: &str) -> bool {
    s.strip_prefix("tb_").is_some_and(|r| (1..=36).contains(&r.len()) && r.chars().all(lower_digit_us))
}

/// The id of a metric: `[a-zA-Z][a-zA-Z0-9_]{0,31}`.
pub(crate) fn metric_id(s: &str) -> bool {
    word(s, letter, alnum_us, 0, 31)
}

/// A full metric name `<testbench name>.<id>`.
pub(crate) fn full_metric(s: &str) -> bool {
    s.split_once('.').is_some_and(|(t, i)| tb_name(t) && metric_id(i))
}

/// A subcircuit name: `[A-Za-z_][A-Za-z0-9_]{0,63}`.
pub(crate) fn subcircuit(s: &str) -> bool {
    word(s, letter_us, alnum_us, 0, 63)
}

pub(crate) fn hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
}

pub(crate) fn hex40(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
}

/// A file name without `/` or `\`.
pub(crate) fn file_name(s: &str) -> bool {
    !s.is_empty() && !s.contains(['/', '\\'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert!(file_path("a/b.scs") && file_path("_x") && !file_path("/a") && !file_path("a//b") && !file_path(".a"));
        assert!(!file_path(&"a/".repeat(7).add_end("b")));
        assert!(tb_name("tb_offset") && !tb_name("tb_") && !tb_name("tb_Off"));
        assert!(full_metric("tb_offset.offset") && !full_metric("tb_offset") && !full_metric("x.offset"));
        assert!(process_name("sky130") && process_name("s") && !process_name("Sky130") && !process_name("1x"));
        assert!(subcircuit("dyn_latch_cmp") && !subcircuit("1a"));
    }

    trait AddEnd {
        fn add_end(self, s: &str) -> String;
    }
    impl AddEnd for String {
        fn add_end(mut self, s: &str) -> String {
            self.push_str(s);
            self
        }
    }
}
