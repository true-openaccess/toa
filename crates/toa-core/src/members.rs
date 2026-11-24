//! The list of members, `members.source.jsonl` at the top of an object (3.3): a source list frozen
//! with the object. Its meaning is fixed in any object; only the frozen library (`lib`) requires it.

use crate::name::{self, Form};
use crate::object::Reference;
use crate::problem::Problem;
use crate::source::{self, Kind, Line, ObjectLine};
use crate::tree;

/// The file, at the top of an object.
pub const FILE: &str = tree::MEMBERS;
/// It keeps the limits of an object (3.1) instead of those of a source list.
pub const MAX_BYTES: usize = tree::MAX_FILE_BYTES as usize;

/// The JSON objects of its lines after the header, each with its line number from 1; lines that
/// are not JSON objects are left out (the strict reading reports them).
fn lines(bytes: &[u8]) -> Vec<(usize, serde_json::Map<String, serde_json::Value>)> {
    let text = std::str::from_utf8(bytes).unwrap_or("");
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        if let Ok(serde_json::Value::Object(m)) = serde_json::from_str(raw.trim_end_matches('\r')) {
            if !(i == 0 && m.contains_key("toa")) {
                out.push((i + 1, m));
            }
        }
    }
    out
}

/// Its references (3.3): the `object` of every line that is an object name or a draft name. `at`
/// is `<line>/object`. The names in cached fields such as `refs` are not references.
pub fn references(bytes: &[u8]) -> Vec<Reference> {
    let mut out = Vec::new();
    for (n, m) in lines(bytes) {
        let Some(s) = m.get("object").and_then(|v| v.as_str()) else { continue };
        let draft = match name::form(s) {
            Form::Object => false,
            Form::Draft => true,
            _ => continue,
        };
        out.push(Reference { at: format!("{n}/object"), name: s.to_string(), draft });
    }
    out
}

/// The draft names anywhere in its values (2.4), for ordering drafts (7.1, step 2).
pub fn drafts(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for (_, m) in lines(bytes) {
        let v = serde_json::Value::Object(m);
        for d in crate::draft::drafts_in_json(&v) {
            if !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

/// Read it strictly (3.3): a source list valid when read strictly (5.1), within the limits of an
/// object; the header and nothing more; object lines only, without `removed` and `latest`; only
/// `git` sources. The object lines, and the problems found.
pub fn read(bytes: &[u8]) -> (Vec<ObjectLine>, Vec<Problem>) {
    let r = source::read(bytes, true, Kind::Members, MAX_BYTES, FILE);
    let objects = r
        .lines
        .into_iter()
        .filter_map(|l| match l {
            Line::Object(o) => Some(o),
            Line::Include(_) => None,
        })
        .collect();
    let problems = r
        .problems
        .into_iter()
        .map(|mut p| {
            p.code = "E_MEMBERS";
            p.section = "3.3";
            p
        })
        .collect();
    (objects, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "ckt_1ec6249cf01c000f19632bde9801c5af4cca1698";
    const B: &str = "ifc_945cdda745e8f44886e3aa17d7752db313d91136";

    #[test]
    fn references_are_the_objects_of_the_lines() {
        let text = format!(
            "{{\"toa\": \"source/1\"}}\n{{\"object\": \"{A}\", \"refs\": [\"{B}\"]}}\n{{\"object\": \"tb_draft-x\"}}\n"
        );
        let r = references(text.as_bytes());
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].at.as_str(), r[0].name.as_str(), r[0].draft), ("2/object", A, false));
        assert_eq!((r[1].at.as_str(), r[1].draft), ("3/object", true));
        assert_eq!(drafts(text.as_bytes()), ["tb_draft-x"]);
    }

    #[test]
    fn the_strict_rules() {
        let good = format!("{{\"toa\": \"source/1\"}}\n{{\"object\": \"{A}\", \"sources\": [{{\"git\": \"https://example.org/l.git\"}}]}}\n");
        let (objects, problems) = read(good.as_bytes());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(objects.len(), 1);
        let bad = format!(
            "{{\"toa\": \"source/1\", \"x\": 1}}\n{{\"include\": \"a.source.jsonl\"}}\n\
             {{\"object\": \"{A}\", \"removed\": true, \"latest\": true, \"sources\": [{{\"local\": \"a\"}}]}}\n"
        );
        let (_, problems) = read(bad.as_bytes());
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(problems.iter().all(|p| p.code == "E_MEMBERS"));
    }
}
