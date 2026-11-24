//! Putting names in place of names, as sealing and unsealing do (7.1, step 3.1): only the
//! characters of each value are replaced; every other byte of the file stays as it is.

use std::collections::BTreeMap;

use crate::manifest::{Node, Style, Value};
use crate::name::{self, Form};

/// One replacement: bytes `start..end` become `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

fn apply(bytes: &[u8], mut edits: Vec<Edit>) -> Vec<u8> {
    edits.sort_by_key(|e| e.start);
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    for e in edits {
        out.extend_from_slice(&bytes[at..e.start]);
        out.extend_from_slice(e.text.as_bytes());
        at = e.end;
    }
    out.extend_from_slice(&bytes[at..]);
    out
}

/// Replace, in every value of a manifest the names that `map` has.
/// `root` is the parsed `bytes`. Returns the new bytes, or `None` when nothing changes.
///
/// A name in `map` written other than as one-line scalar without escapes is an error, and so,
/// when `drafts_left_are_errors`, is any value of the form of a draft name that `map` lacks.
pub fn replace_in_manifest(
    bytes: &[u8],
    root: &Node,
    map: &BTreeMap<String, String>,
    drafts_left_are_errors: bool,
) -> Result<Option<Vec<u8>>, Vec<String>> {
    let mut edits = Vec::new();
    let mut errors = Vec::new();
    root.walk(&mut Vec::new(), &mut |path, n| {
        let Value::Str(s) = &n.value else { return };
        let at = path.join("/");
        match map.get(s) {
            Some(to) => {
                let raw = &bytes[n.span.start..n.span.end];
                let inner = match n.style {
                    Style::Plain => Some(raw),
                    Style::SingleQuoted | Style::DoubleQuoted if raw.len() >= 2 => Some(&raw[1..raw.len() - 1]),
                    _ => None,
                };
                match inner {
                    Some(text) if text == s.as_bytes() => {
                        let start = n.span.start + if n.style == Style::Plain { 0 } else { 1 };
                        edits.push(Edit { start, end: start + s.len(), text: to.clone() });
                    }
                    _ => errors.push(format!(
                        "toa.yaml:{at}: '{s}' must be written as it is, a scalar on one line without escapes (7.1)"
                    )),
                }
            }
            None if drafts_left_are_errors && name::is_draft_name(s) => {
                errors.push(format!("toa.yaml:{at}: '{s}' names no draft being sealed"))
            }
            None => {}
        }
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok((!edits.is_empty()).then(|| apply(bytes, edits)))
}

/// A string of a JSON text: where it is (with its quotes), and whether it is a key.
struct JsonString {
    start: usize,
    end: usize,
    is_key: bool,
}

/// The strings of a JSON text, in order. The text must already be known to be JSON.
fn json_strings(bytes: &[u8]) -> Vec<JsonString> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < bytes.len() && bytes[i] != b'"' {
            i += if bytes[i] == b'\\' { 2 } else { 1 };
        }
        i += 1;
        let end = i.min(bytes.len());
        let mut j = end;
        while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\r' | b'\n') {
            j += 1;
        }
        out.push(JsonString { start, end, is_key: j < bytes.len() && bytes[j] == b':' });
    }
    out
}

/// The same as [`replace_in_manifest`] for the values of a `lock.json`: a name in `map` must be
/// a JSON string without escapes.
pub fn replace_in_json(
    bytes: &[u8],
    map: &BTreeMap<String, String>,
    drafts_left_are_errors: bool,
) -> Result<Option<Vec<u8>>, Vec<String>> {
    if serde_json::from_slice::<serde_json::Value>(bytes).is_err() {
        return Err(vec!["lock.json is not JSON".into()]);
    }
    replace_strings(bytes, map, drafts_left_are_errors, "lock.json")
}

/// The same for the values of a `members.source.jsonl` (7.1, step 3.1): every line that is not
/// empty is a JSON value, and a name in `map` is a JSON string without escapes.
pub fn replace_in_members(
    bytes: &[u8],
    map: &BTreeMap<String, String>,
    drafts_left_are_errors: bool,
) -> Result<Option<Vec<u8>>, Vec<String>> {
    let file = crate::members::FILE;
    let text = std::str::from_utf8(bytes).map_err(|_| vec![format!("{file} is not UTF-8")])?;
    for (i, line) in text.split('\n').enumerate() {
        if !line.trim().is_empty() && serde_json::from_str::<serde_json::Value>(line).is_err() {
            return Err(vec![format!("{file}: line {} is not JSON", i + 1)]);
        }
    }
    replace_strings(bytes, map, drafts_left_are_errors, file)
}

fn replace_strings(
    bytes: &[u8],
    map: &BTreeMap<String, String>,
    drafts_left_are_errors: bool,
    file: &str,
) -> Result<Option<Vec<u8>>, Vec<String>> {
    let mut edits = Vec::new();
    let mut errors = Vec::new();
    for s in json_strings(bytes) {
        if s.is_key {
            continue;
        }
        let token = &bytes[s.start..s.end];
        let Ok(value) = serde_json::from_slice::<String>(token) else { continue };
        let raw = &token[1..token.len() - 1];
        match map.get(&value) {
            Some(to) if raw == value.as_bytes() => {
                edits.push(Edit { start: s.start + 1, end: s.end - 1, text: to.clone() })
            }
            Some(_) => errors.push(format!("{file}: '{value}' must be a JSON string without escapes (7.1)")),
            None if drafts_left_are_errors && name::is_draft_name(&value) => {
                errors.push(format!("{file}: '{value}' names no draft being sealed"))
            }
            None => {}
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok((!edits.is_empty()).then(|| apply(bytes, edits)))
}

/// Remove a top-level `base: []` line (7.1, step 3.2). `None` when there is none; an error
/// when `base` is an empty list written any other way.
pub fn remove_empty_base(bytes: &[u8], root: &Node) -> Result<Option<Vec<u8>>, String> {
    let Some(entries) = root.as_map() else { return Ok(None) };
    let Some((key, value)) = entries.iter().find(|(k, _)| k.text == "base") else { return Ok(None) };
    if !matches!(&value.value, Value::Seq(v) if v.is_empty()) {
        return Ok(None);
    }
    let start = key.span.start;
    let line_start = bytes[..start].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let line_end = bytes[start..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| start + i + 1);
    let line = std::str::from_utf8(&bytes[line_start..line_end]).unwrap_or("");
    let text = line.trim_end_matches('\n');
    let text = match text.find(" #") {
        Some(i) => &text[..i],
        None => text,
    };
    if line_start != start || text.trim_end() != "base: []" {
        return Err("toa.yaml: an empty base is written as a line of its own, 'base: []' (7.1)".into());
    }
    let mut out = bytes[..line_start].to_vec();
    out.extend_from_slice(&bytes[line_end..]);
    Ok(Some(out))
}

/// Make the top-level `base` of a manifest `[name]`, as deriving a draft from an object does: the
/// lines of a `base` written are replaced by one line `base: [name]`; without one, that line is
/// added at the end. Every other byte stays as it is. The result is read again, and must be the
/// same manifest but for `base`.
pub fn set_base(bytes: &[u8], root: &Node, name: &str) -> Result<Vec<u8>, String> {
    fn end_of(n: &Node) -> usize {
        match &n.value {
            Value::Seq(v) => v.iter().map(end_of).fold(n.span.end, usize::max),
            Value::Map(m) => m.iter().map(|(k, v)| k.span.end.max(end_of(v))).fold(n.span.end, usize::max),
            _ => n.span.end,
        }
    }
    let entries = root.as_map().ok_or("toa.yaml: the manifest is not a mapping")?;
    let line = format!("base: [{name}]\n");
    let out = match entries.iter().find(|(k, _)| k.text == "base") {
        Some((key, value)) => {
            let start = key.span.start;
            let line_start = bytes[..start].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
            if line_start != start {
                return Err("toa.yaml: base does not start a line".into());
            }
            let last = end_of(value).max(key.span.end).min(bytes.len());
            let line_end = bytes[last..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| last + i + 1);
            let mut out = bytes[..line_start].to_vec();
            out.extend_from_slice(line.as_bytes());
            out.extend_from_slice(&bytes[line_end..]);
            out
        }
        None => {
            let mut out = bytes.to_vec();
            if !out.is_empty() && !out.ends_with(b"\n") {
                out.push(b'\n');
            }
            out.extend_from_slice(line.as_bytes());
            out
        }
    };
    // the same manifest but for base, which is now [name]
    let new = crate::manifest::parse(&out).map_err(|_| "toa.yaml: base could not be rewritten in place".to_string())?;
    let without = |n: &Node| {
        let mut v = crate::manifest::to_json(n);
        let base = v.as_object_mut().and_then(|m| m.remove("base"));
        (v, base)
    };
    let (old_rest, _) = without(root);
    let (new_rest, new_base) = without(&new);
    if old_rest != new_rest || new_base != Some(serde_json::json!([name])) {
        return Err("toa.yaml: base could not be rewritten without changing anything else".into());
    }
    Ok(out)
}

/// The draft names a manifest names anywhere (2.4).
pub fn drafts_in_manifest(root: &Node) -> Vec<String> {
    let mut out = Vec::new();
    root.walk(&mut Vec::new(), &mut |_, n| {
        if let Value::Str(s) = &n.value {
            if name::form(s) == Form::Draft && !out.contains(s) {
                out.push(s.clone());
            }
        }
    });
    out
}

/// The draft names that are values of a JSON text (2.4).
pub fn drafts_in_json(v: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) if name::is_draft_name(s) && !out.contains(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(m) => m.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    const OBJ: &str = "ckt_1ec6249c1ec6249c1ec6249c1ec6249c1ec6249c";

    fn map() -> BTreeMap<String, String> {
        [("ckt_draft-a".to_string(), OBJ.to_string())].into()
    }

    #[test]
    fn only_the_characters_of_the_value_change() {
        let text = "type: evd   # a comment\ndut: ckt_draft-a\ntbs: ['ckt_draft-a', \"ckt_draft-a\"]\nkey: x\n";
        let root = manifest::parse(text.as_bytes()).unwrap();
        let out = replace_in_manifest(text.as_bytes(), &root, &map(), true).unwrap().unwrap();
        let want = text.replace("ckt_draft-a", OBJ);
        assert_eq!(String::from_utf8(out).unwrap(), want);
        // nothing to change: no new bytes
        let root = manifest::parse(want.as_bytes()).unwrap();
        assert_eq!(replace_in_manifest(want.as_bytes(), &root, &map(), true).unwrap(), None);
    }

    #[test]
    fn escapes_and_unknown_drafts_are_errors() {
        let text = "type: evd\ndut: \"imp_draft\\x2da\"\nother: tb_draft-z\n";
        let root = manifest::parse(text.as_bytes()).unwrap();
        let e = replace_in_manifest(text.as_bytes(), &root, &map(), true).unwrap_err();
        assert_eq!(e.len(), 2, "{e:?}");
    }

    #[test]
    fn json_values_not_keys() {
        let text = "{\"ckt_draft-a\": \"ckt_draft-a\", \"t\": [\"ckt_draft-a\"]}\n";
        let out = replace_in_json(text.as_bytes(), &map(), true).unwrap().unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), format!("{{\"ckt_draft-a\": \"{OBJ}\", \"t\": [\"{OBJ}\"]}}\n"));
        let escaped = "{\"dut\": \"imp_draft\\u002da\"}";
        assert!(replace_in_json(escaped.as_bytes(), &map(), true).is_err());
    }

    #[test]
    fn empty_base_line() {
        let text = "type: ckt\nbase: []\nlicense: MIT\n";
        let root = manifest::parse(text.as_bytes()).unwrap();
        let out = remove_empty_base(text.as_bytes(), &root).unwrap().unwrap();
        assert_eq!(out, b"type: ckt\nlicense: MIT\n");
        let inline = "type: ckt\nbase:\n  []\n";
        let root = manifest::parse(inline.as_bytes()).unwrap();
        assert!(remove_empty_base(inline.as_bytes(), &root).is_err());
    }

    #[test]
    fn members_lines() {
        let text = "{\"toa\": \"source/1\"}\n{\"object\": \"ckt_draft-a\", \"label\": \"ckt_draft-a\"}\n";
        let out = replace_in_members(text.as_bytes(), &map(), true).unwrap().unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), text.replace("ckt_draft-a", OBJ));
        assert!(replace_in_members(b"{\"toa\": \"source/1\"}\nnot json\n", &map(), true).is_err());
        assert!(replace_in_members(b"{\"object\": \"tb_draft-z\"}\n", &map(), true).is_err(), "a draft left");
    }

    #[test]
    fn base_set_in_place() {
        let set = |text: &str| {
            let root = manifest::parse(text.as_bytes()).unwrap();
            set_base(text.as_bytes(), &root, OBJ).map(|b| String::from_utf8(b).unwrap())
        };
        // added at the end
        assert_eq!(set("type: ckt\nname: x\n").unwrap(), format!("type: ckt\nname: x\nbase: [{OBJ}]\n"));
        // a flow list, an empty one, and a block list replaced; the comments and lines around kept
        let other = "ckt_2222222222222222222222222222222222222222";
        assert_eq!(
            set(&format!("type: ckt # c\nbase: [{other}, {other}]\nname: x\n")).unwrap(),
            format!("type: ckt # c\nbase: [{OBJ}]\nname: x\n")
        );
        assert_eq!(set("type: ckt\nbase: []\nname: x\n").unwrap(), format!("type: ckt\nbase: [{OBJ}]\nname: x\n"));
        assert_eq!(
            set(&format!("type: ckt\nbase:\n- {other}\n- {other}\n\n# kept\nname: x\n")).unwrap(),
            format!("type: ckt\nbase: [{OBJ}]\n\n# kept\nname: x\n")
        );
        assert_eq!(set(&format!("name: x\nbase:\n  - {other}\n")).unwrap(), format!("name: x\nbase: [{OBJ}]\n"));
    }
}
