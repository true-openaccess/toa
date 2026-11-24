//! `history.json` (3.4): written by sealing, in exactly one way.

use crate::name;

/// What `history.json` holds: the ancestors along the first item of `base`, most recent first,
/// and the generation of the object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct History {
    pub history: Vec<String>,
    pub generation: u64,
}

/// The generation of an object without `base`, or of one whose `history.json` cannot be read.
pub const FIRST: u64 = 1;

/// The bytes of `history.json`: what Python's
/// `json.dumps({"history": names, "generation": n}, indent=2)` writes, followed by one LF.
pub fn write(h: &History) -> Vec<u8> {
    let mut s = String::from("{\n  \"history\": [");
    if h.history.is_empty() {
        s.push(']');
    } else {
        for (i, n) in h.history.iter().enumerate() {
            s.push_str(if i == 0 { "\n    " } else { ",\n    " });
            // names hold only letters, digits, '_' and '-', which JSON writes as they are; anything
            // else goes through serde_json, which escapes as Python's default `ensure_ascii` does
            // for the ASCII range
            s.push_str(&serde_json::to_string(n).unwrap_or_default());
        }
        s.push_str("\n  ]");
    }
    s.push_str(&format!(",\n  \"generation\": {}\n}}\n", h.generation));
    s.into_bytes()
}

/// Read `history.json`: its content, when the file is written the one way, holds only object
/// names and a generation from 1; otherwise why not.
pub fn read(bytes: &[u8]) -> Result<History, String> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("history.json is not JSON: {e}"))?;
    let obj = v
        .as_object()
        .filter(|m| m.len() == 2)
        .ok_or("history.json is an object with the two keys history and generation")?;
    let list = obj.get("history").and_then(|h| h.as_array()).ok_or("history is a list of object names")?;
    let mut names = Vec::new();
    for n in list {
        match n.as_str() {
            Some(s) if name::is_object_name(s) => names.push(s.to_string()),
            _ => return Err("history lists object names".into()),
        }
    }
    if names.is_empty() {
        return Err("history is not empty: an object without base has no history.json".into());
    }
    let generation =
        obj.get("generation").and_then(|g| g.as_u64()).filter(|&g| g >= 1).ok_or("generation is an integer from 1")?;
    let h = History { history: names, generation };
    if write(&h) != bytes {
        return Err("history.json is not written the one way: two spaces of indentation, one name per line, \
                    history before generation, ending with LF (3.4)"
            .into());
    }
    Ok(h)
}

/// Whether a parent and its child are in one series (3.4): objects of one type with the same
/// `name`, or, for a type without `name`, any. The prefixes agree already (`base` keeps the
/// prefix of the object), so only the names are compared.
pub fn same_series(child_name: Option<&str>, parent_name: Option<&str>) -> bool {
    child_name == parent_name
}

/// The `history.json` a sealed object gets: the first item of `base`, then that item's history;
/// its generation is the parent's plus one in one series, else 1. `parent` is `None` for a parent
/// without `history.json`, which is of generation 1.
pub fn of(first_parent: &str, parent: Option<&History>, same_series: bool) -> History {
    let mut history = vec![first_parent.to_string()];
    history.extend(parent.map(|p| p.history.clone()).unwrap_or_default());
    let parent_generation = parent.map_or(FIRST, |p| p.generation);
    History { history, generation: if same_series { parent_generation + 1 } else { FIRST } }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "ckt_cd15342bb4ed1cc1137937195f7cf3c3b8b03af5";
    const B: &str = "ckt_f1ede73af6ba50ae0621e34b2b6f6b946dbf1ff1";

    #[test]
    fn written_as_the_standard_shows() {
        let want = format!("{{\n  \"history\": [\n    \"{A}\",\n    \"{B}\"\n  ],\n  \"generation\": 3\n}}\n");
        let h = History { history: vec![A.to_string(), B.to_string()], generation: 3 };
        assert_eq!(String::from_utf8(write(&h)).unwrap(), want);
        assert_eq!(read(want.as_bytes()).unwrap(), h);
    }

    #[test]
    fn other_ways_of_writing_are_refused() {
        let compact = format!("{{\"history\": [\"{A}\"], \"generation\": 2}}\n");
        assert!(read(compact.as_bytes()).is_err());
        let no_lf = format!("{{\n  \"history\": [\n    \"{A}\"\n  ],\n  \"generation\": 2\n}}");
        assert!(read(no_lf.as_bytes()).is_err());
        let draft = "{\n  \"history\": [\n    \"ckt_draft-a\"\n  ],\n  \"generation\": 2\n}\n";
        assert!(read(draft.as_bytes()).is_err());
        let old = format!("{{\n  \"history\": [\n    \"{A}\"\n  ]\n}}\n");
        assert!(read(old.as_bytes()).is_err(), "without generation");
        let zero = format!("{{\n  \"history\": [\n    \"{A}\"\n  ],\n  \"generation\": 0\n}}\n");
        assert!(read(zero.as_bytes()).is_err());
        let swapped = format!("{{\n  \"generation\": 2,\n  \"history\": [\n    \"{A}\"\n  ]\n}}\n");
        assert!(read(swapped.as_bytes()).is_err(), "keys in their order");
    }

    #[test]
    fn generations() {
        let p = History { history: vec![B.to_string()], generation: 2 };
        assert_eq!(of(A, Some(&p), true), History { history: vec![A.into(), B.into()], generation: 3 });
        assert_eq!(of(A, Some(&p), false).generation, 1, "a port starts a series of its own");
        assert_eq!(of(A, None, true).generation, 2, "a parent without history.json is of generation 1");
        assert!(same_series(Some("x"), Some("x")) && same_series(None, None) && !same_series(Some("x"), Some("y")));
    }
}
