//! An object as a whole: its common fields (3.2), its references (3.3) and its external
//! dependencies (3.5), and verifying a set of files against an object name (7.4).

use crate::hash::{self, Algorithm};
use crate::manifest::{self, Node, Value};
use crate::name::{self, Form};
use crate::problem::Problem;
use crate::spdx;
use crate::tree::{self, Entry, Source};

/// One reference: where in the manifest or lock file, and the name it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Keys and indexes from the top, such as `uses/dp/impl`.
    pub at: String,
    pub name: String,
    /// True for a draft name (2.4), false for an object name.
    pub draft: bool,
}

/// The references of a manifest (3.3): every value anywhere that is, as a whole string, an object
/// name or a draft name. (A name meant only as a mention is written in `README.md`.)
pub fn references(root: &Node) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    root.walk(&mut path, &mut |p, n| {
        if let Value::Str(s) = &n.value {
            match name::form(s) {
                Form::Object => out.push(Reference { at: p.join("/"), name: s.clone(), draft: false }),
                Form::Draft => out.push(Reference { at: p.join("/"), name: s.clone(), draft: true }),
                _ => {}
            }
        }
    });
    out
}

/// The references in a `lock.json` (3.3): every string anywhere that is a name.
pub fn lock_references(lock: &serde_json::Value) -> Vec<Reference> {
    fn walk(v: &serde_json::Value, path: &mut Vec<String>, out: &mut Vec<Reference>) {
        match v {
            serde_json::Value::String(s) => match name::form(s) {
                Form::Object => out.push(Reference { at: path.join("/"), name: s.clone(), draft: false }),
                Form::Draft => out.push(Reference { at: path.join("/"), name: s.clone(), draft: true }),
                _ => {}
            },
            serde_json::Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    path.push(i.to_string());
                    walk(x, path, out);
                    path.pop();
                }
            }
            serde_json::Value::Object(m) => {
                for (k, x) in m {
                    path.push(k.clone());
                    walk(x, path, out);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(lock, &mut Vec::new(), &mut out);
    out
}

fn err(code: &'static str, at: &str, msg: impl Into<String>) -> Problem {
    Problem::error(code, "3.2", format!("toa.yaml:{at}"), msg)
}

/// The longest `title` (3.2.1).
pub const MAX_TITLE: usize = 200;

/// The common fields of a manifest (3.2.1), and `external` (3.5) where it is written. `prefix` is the prefix of the object's name or
/// draft name; `files` are the paths of the object's files, for the license texts of
/// `LicenseRef-`.
pub fn check_common(prefix: &str, root: &Node, files: &[&str]) -> Vec<Problem> {
    let mut out = Vec::new();
    match root.get("type").map(|n| &n.value) {
        Some(Value::Str(s)) if s == prefix => {}
        Some(Value::Str(s)) => {
            out.push(err("E_TYPE_PREFIX", "type", format!("type '{s}' is not the prefix '{prefix}'")))
        }
        Some(_) => out.push(err("E_TYPE_PREFIX", "type", "type is a string: the prefix")),
        None => out.push(err("E_FIELD_MISSING", "type", "type is required")),
    }
    match root.get("license").map(|n| &n.value) {
        Some(Value::Str(s)) => match spdx::parse(s) {
            Ok(expr) => {
                for r in &expr.refs {
                    let has_text = files.iter().any(|f| !f.contains('/') && f.starts_with("LICENSE"));
                    if !has_text {
                        out.push(err(
                            "E_LICENSE_TEXT",
                            "license",
                            format!("{r} needs the license text in a file whose name starts with LICENSE"),
                        ));
                        break;
                    }
                }
            }
            Err(m) => out.push(err("E_LICENSE", "license", m)),
        },
        Some(_) => out.push(err("E_LICENSE", "license", "license is a string: an SPDX license expression")),
        None => out.push(err("E_FIELD_MISSING", "license", "license is required")),
    }
    if let Some(n) = root.get("author") {
        if n.as_str().is_none() {
            out.push(err("E_FIELD", "author", "author is a string"));
        }
    }
    if let Some(n) = root.get("base") {
        match n.as_seq() {
            Some(items) if !items.is_empty() => {
                for (i, it) in items.iter().enumerate() {
                    match it.as_str().filter(|s| matches!(name::form(s), Form::Object | Form::Draft)) {
                        None => out.push(err("E_FIELD", &format!("base/{i}"), "base lists object names")),
                        // an object is derived only from objects of its own type (3.4)
                        Some(s) if name::prefix_of(s) != Some(prefix) => out.push(err(
                            "E_BASE_PREFIX",
                            &format!("base/{i}"),
                            format!("{s} is not of the type '{prefix}': base names objects of the object's own type"),
                        )),
                        Some(_) => {}
                    }
                }
            }
            _ => out.push(err("E_FIELD", "base", "base is a list of object names that is not empty")),
        }
    }
    if let Some(n) = root.get("title") {
        if n.as_str().is_none_or(|t| t.is_empty() || t.contains('\n') || t.chars().count() > MAX_TITLE) {
            out.push(err("E_FIELD", "title", "title is one line of at most 200 characters"));
        }
    }
    if let Some(n) = root.get("description") {
        if n.as_str().is_none() {
            out.push(err("E_FIELD", "description", "description is a string"));
        }
    }
    if let Some(n) = root.get("tags") {
        match n.as_seq() {
            Some(items) => {
                for (i, it) in items.iter().enumerate() {
                    if it.as_str().is_none_or(str::is_empty) {
                        out.push(err("E_FIELD", &format!("tags/{i}"), "tags lists words"));
                    }
                }
            }
            None => out.push(err("E_FIELD", "tags", "tags is a list of words")),
        }
    }
    if let Some(n) = root.get("external") {
        check_external(n, &mut out);
    }
    out
}

fn check_external(n: &Node, out: &mut Vec<Problem>) {
    let e = |at: String, m: &str| Problem::error("E_EXTERNAL", "3.5", format!("toa.yaml:external/{at}"), m);
    let Some(items) = n.as_seq() else {
        out.push(Problem::error("E_EXTERNAL", "3.5", "toa.yaml:external", "external is a list"));
        return;
    };
    let mut names = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if it.as_map().is_none() {
            out.push(e(i.to_string(), "each external file is a mapping"));
            continue;
        }
        match it.get("name").and_then(Node::as_str) {
            Some(s) if !s.is_empty() => {
                if names.contains(&s) {
                    out.push(e(format!("{i}/name"), "the names of external files differ within an object"));
                }
                names.push(s);
            }
            _ => out.push(e(format!("{i}/name"), "name is required")),
        }
        // an open list of words, such as modelfile, techfile or other (3.5)
        match it.get("role").and_then(Node::as_str) {
            Some(r) if is_role(r) => {}
            _ => out.push(e(
                format!("{i}/role"),
                "role is a word such as modelfile, techfile or other: [a-z][a-z0-9_-]{0,31}",
            )),
        }
        for key in ["process", "version"] {
            if it.get(key).is_some_and(|v| v.as_str().is_none()) {
                out.push(e(format!("{i}/{key}"), "a string"));
            }
        }
        if let Some(f) = it.get("file") {
            if f.as_str().is_none_or(|s| s.is_empty() || s.contains(['/', '\\'])) {
                out.push(e(format!("{i}/file"), "file is a file name without '/' or '\\'"));
            }
        }
        if let Some(m) = it.get("match") {
            if m.as_seq().is_none_or(|v| v.iter().any(|x| x.as_str().is_none())) {
                out.push(e(format!("{i}/match"), "match is a list of strings"));
            }
        }
        if let Some(h) = it.get("sha256") {
            let ok = h.as_str().is_some_and(|s| s.len() == 64 && hash::from_hex(s).is_some());
            if !ok {
                out.push(e(format!("{i}/sha256"), "sha256 is 64 lowercase hexadecimal digits"));
            }
        }
    }
}

/// A role of an external file (3.5): `[a-z][a-z0-9_-]{0,31}`.
fn is_role(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_lowercase())
        && s.len() <= 32
        && c.all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == '_' || x == '-')
}

/// The result of verifying a set of files.
#[derive(Debug, Clone)]
pub struct Verified {
    /// The hash of the files, when the tree is well-formed and the hash could be computed.
    pub hash: Option<String>,
    /// The manifest, when it could be read.
    pub manifest: Option<Node>,
    pub problems: Vec<Problem>,
}

/// Verify from the files (7.4): the tree is well-formed (3.1), the manifest is written by the
/// rules and has valid common fields (3.2), `type` is the prefix, and the hash is the hash in
/// `name` when `name` is an object name. `name` may also be a draft name, which skips the last
/// test. `files` are paths and bytes; `dirs` the directories, so that empty ones are seen.
pub fn verify_files(name: &str, files: &[(&str, &[u8])], dirs: &[&str], executable: &[&str]) -> Verified {
    let mut problems = Vec::new();
    let Some(prefix) = name::prefix_of(name).filter(|_| !name::is_reserved(name)) else {
        problems.push(Problem::error("E_NAME", "2.1", name, "not an object name or a draft name"));
        return Verified { hash: None, manifest: None, problems };
    };
    let mut entries: Vec<Entry> = dirs.iter().map(|d| Entry::dir(*d)).collect();
    for (p, b) in files {
        let mut e = Entry::file(*p, b.len() as u64);
        e.executable = Some(executable.contains(p));
        entries.push(e);
    }
    problems.extend(tree::check(&entries, Source::FileSystem));
    let well_formed = problems.is_empty();

    let manifest = files.iter().find(|(p, _)| *p == tree::MANIFEST).and_then(|(_, b)| match manifest::parse(b) {
        Ok(n) => Some(n),
        Err(ps) => {
            problems.extend(ps);
            None
        }
    });
    if let Some(m) = &manifest {
        let paths: Vec<&str> = files.iter().map(|(p, _)| *p).collect();
        problems.extend(check_common(prefix, m, &paths));
    }
    if let Some((_, b)) = files.iter().find(|(p, _)| *p == tree::LOCK) {
        if b.len() > 1024 * 1024 || !serde_json::from_slice::<serde_json::Value>(b).is_ok_and(|v| v.is_object()) {
            problems.push(Problem::error("E_LOCK", "3.3", tree::LOCK, "lock.json is a JSON object of at most 1 MiB"));
        }
    }
    if let Some((_, b)) = files.iter().find(|(p, _)| *p == tree::MEMBERS) {
        // draft names are put in place when sealing; a draft is checked afterwards
        if name::is_object_name(name) {
            problems.extend(crate::members::read(b).1);
        }
    }

    // the hash exists only for a well-formed tree (2.2)
    let hash = if well_formed {
        match hash::hash_files(Algorithm::Sha1, files) {
            Ok(h) => Some(h),
            Err(e) => {
                problems.push(Problem::error("E_HASH", "2.2", "", e.to_string()));
                None
            }
        }
    } else {
        None
    };
    if let (Some(h), Some((_, want))) = (&hash, name::split(name)) {
        if h != want {
            problems.push(Problem::error(
                "E_NAME_MISMATCH",
                "7.4",
                "",
                format!("the files hash to {h}, not to the hash in the name"),
            ));
        }
    }
    Verified { hash, manifest, problems }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJ: &str = "doc_ea44863a8d2f242bc651caf709d45a27e7531d9d";

    fn files() -> Vec<(&'static str, &'static [u8])> {
        vec![("toa.yaml", b"type: doc\nlicense: MIT\n"), ("README.md", b"hello\n")]
    }

    #[test]
    fn the_object_vector_verifies() {
        let v = verify_files(OBJ, &files(), &[], &[]);
        assert!(v.problems.is_empty(), "{:?}", v.problems);
        assert_eq!(v.hash.as_deref(), Some("ea44863a8d2f242bc651caf709d45a27e7531d9d"));
        let v = verify_files("doc_draft-x", &files(), &[], &[]);
        assert!(v.problems.is_empty());
    }

    #[test]
    fn a_wrong_prefix_or_hash_is_found() {
        let other = "ckt_ea44863a8d2f242bc651caf709d45a27e7531d9d";
        let codes: Vec<_> = verify_files(other, &files(), &[], &[]).problems.iter().map(|p| p.code).collect();
        assert_eq!(codes, ["E_TYPE_PREFIX"]);
        let wrong = "doc_ea44863a8d2f242bc651caf709d45a27e7531d90";
        let codes: Vec<_> = verify_files(wrong, &files(), &[], &[]).problems.iter().map(|p| p.code).collect();
        assert_eq!(codes, ["E_NAME_MISMATCH"]);
    }

    #[test]
    fn references_are_every_name() {
        let m = manifest::parse(
            format!("type: evd\nlicense: MIT\ndut: {OBJ}\nother:\n- {OBJ}\ntbs:\n- tb_draft-a\nnote: tb_ac\n")
                .as_bytes(),
        )
        .unwrap();
        let r = references(&m);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].at, "dut");
        assert_eq!(r[1].at, "other/0");
        assert!(r[2].draft);
    }

    #[test]
    fn common_fields() {
        let text = format!(
            "type: doc\nlicense: LicenseRef-acme\nbase: []\ntitle: {}\ntags: [a, {{b: c}}]\n\
             external:\n- name: m\n  role: X\n  file: a/b\n",
            "x".repeat(201)
        );
        let m = manifest::parse(text.as_bytes()).unwrap();
        let codes: Vec<_> = check_common("doc", &m, &["toa.yaml"]).iter().map(|p| p.code).collect();
        assert_eq!(codes, ["E_LICENSE_TEXT", "E_FIELD", "E_FIELD", "E_FIELD", "E_EXTERNAL", "E_EXTERNAL"]);
        assert!(check_common("doc", &m, &["LICENSE.acme"]).iter().all(|p| p.code != "E_LICENSE_TEXT"));
        let m =
            manifest::parse(format!("type: doc\nlicense: MIT\nbase: [{OBJ}, ckt_draft-a, doc_draft-b]\n").as_bytes())
                .unwrap();
        let codes: Vec<_> = check_common("doc", &m, &[]).iter().map(|p| p.code).collect();
        assert_eq!(codes, ["E_BASE_PREFIX"]);
    }
}
