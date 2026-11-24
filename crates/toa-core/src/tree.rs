//! Well-formed object trees (3.1).
//!
//! The rules look only at names, kinds and sizes, never at contents, so the same check runs on a
//! directory read from disk and on a git listing. A listing needs two more checks (3.1): no path
//! twice, and every directory on the way to a path listed as a directory, which is how a git tree
//! member whose name holds a `/` is found.

use std::collections::{BTreeMap, BTreeSet};

use crate::name;
use crate::problem::Problem;

/// The name of the manifest of every object (3.2).
pub const MANIFEST: &str = "toa.yaml";
/// The lineage file at the top of an object (3.4).
pub const HISTORY: &str = "history.json";
/// The lock file at the top of an object (3.3).
pub const LOCK: &str = "lock.json";
/// The list of members at the top of an object (3.3).
pub const MEMBERS: &str = "members.source.jsonl";

/// Limits of 3.1.
pub const MAX_COMPONENT: usize = 64;
pub const MAX_DEPTH: usize = 6;
pub const MAX_PATH: usize = 80;
pub const MAX_FILES: usize = 2000;
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_OBJECT_BYTES: u64 = 50 * 1024 * 1024;

/// What a member of a tree is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// A symbolic link, a junction, a submodule, a device: never allowed in an object.
    Other,
}

/// One member of a tree, with its path relative to the object directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub kind: Kind,
    /// Size in bytes; only read for files.
    pub size: u64,
    /// `Some(true)` for a file with the executable bit, `None` where the file system cannot say
    /// (Windows): then the bit is left to the git pass of sealing and the library check (3.1).
    pub executable: Option<bool>,
}

impl Entry {
    pub fn file(path: impl Into<String>, size: u64) -> Self {
        Entry { path: path.into(), kind: Kind::File, size, executable: Some(false) }
    }

    pub fn dir(path: impl Into<String>) -> Self {
        Entry { path: path.into(), kind: Kind::Dir, size: 0, executable: Some(false) }
    }
}

/// Where the entries come from: a listing gets the two extra checks of 3.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    FileSystem,
    Listing,
}

const RESERVED_WINDOWS: [&str; 4] = ["con", "prn", "aux", "nul"];

/// Why a single name breaks 3.1 item 3, or `None` when it is fine. `leading_dot` allows the
/// names outside objects that may start with a dot (6.2), such as `.gitattributes`.
pub fn component_problem(name: &str, leading_dot: bool) -> Option<String> {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > MAX_COMPONENT {
        return Some(format!("a name has 1 to {MAX_COMPONENT} characters"));
    }
    if let Some(c) = b.iter().find(|c| !(c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))) {
        return Some(format!("the character {:?} is not allowed in a name", *c as char));
    }
    let first_ok = b[0].is_ascii_alphanumeric() || b[0] == b'_' || (leading_dot && b[0] == b'.');
    if !first_ok {
        return Some("a name starts with a letter, a digit or an underscore".into());
    }
    if b[b.len() - 1] == b'.' {
        return Some("a name does not end with a dot".into());
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = RESERVED_WINDOWS.contains(&stem.as_str())
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Some(format!("'{stem}' is a reserved name on Windows"));
    }
    None
}

/// The problems of a set of entries as an object tree (3.1). An empty result means the tree is
/// well-formed. Every problem names the path it is about.
pub fn check(entries: &[Entry], source: Source) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut err = |code, path: &str, msg: String| problems.push(Problem::error(code, "3.1", path, msg));

    let mut seen = BTreeSet::new();
    let mut dirs: BTreeSet<&str> = BTreeSet::new();
    let mut nonempty: BTreeSet<&str> = BTreeSet::new();
    // per directory: lowercased name -> first spelling, for item 4
    let mut folded: BTreeMap<(&str, String), &str> = BTreeMap::new();
    let mut files = 0usize;
    let mut total: u64 = 0;
    let mut has_manifest = false;

    for e in entries {
        let path = e.path.as_str();
        if !seen.insert(path) {
            err("E_TREE_DUPLICATE_PATH", path, "the path is listed twice".into());
            continue;
        }
        if path.is_empty() || path.split('/').any(str::is_empty) {
            err("E_TREE_NAME", path, "a path has no empty levels".into());
            continue;
        }
        let (parent, leaf) = path.rsplit_once('/').unwrap_or(("", path));
        nonempty.insert(parent);

        // item 3 and item 5, on the last level; the levels above are their own entries
        if let Some(why) = component_problem(leaf, false) {
            err("E_TREE_NAME", path, why);
        }
        if matches!(name::form(leaf), name::Form::Object | name::Form::Draft | name::Form::Reserved) {
            err("E_TREE_NESTED_OBJECT", path, "a name inside an object is an object name or a draft name".into());
        }
        // item 4
        let key = (parent, leaf.to_ascii_lowercase());
        if let Some(other) = folded.get(&key) {
            err("E_TREE_CASE", path, format!("'{leaf}' and '{other}' differ only in case"));
        } else {
            folded.insert(key, leaf);
        }
        // item 6: depth and length
        if path.matches('/').count() > MAX_DEPTH {
            err("E_TREE_DEPTH", path, format!("more than {MAX_DEPTH} levels of directories"));
        }
        if path.len() > MAX_PATH {
            err("E_TREE_PATH_LENGTH", path, format!("a path inside an object has at most {MAX_PATH} characters"));
        }

        match e.kind {
            Kind::Dir => {
                dirs.insert(path);
            }
            Kind::File => {
                files += 1;
                total = total.saturating_add(e.size);
                if e.size > MAX_FILE_BYTES {
                    err("E_TREE_FILE_SIZE", path, format!("a file has at most {MAX_FILE_BYTES} bytes"));
                }
                if e.executable == Some(true) {
                    err("E_TREE_EXECUTABLE", path, "a file in an object has no executable bit".into());
                }
                if path == MANIFEST {
                    has_manifest = true;
                }
            }
            Kind::Other => {
                err("E_TREE_KIND", path, "only plain files and directories are allowed in an object".into());
            }
        }
        // item 2: the files with a fixed meaning at the top are plain files
        if (path == MANIFEST || path == HISTORY || path == LOCK || path == MEMBERS) && e.kind != Kind::File {
            err("E_TREE_KIND", path, format!("'{path}' at the top of an object is a plain file"));
        }
    }

    if !has_manifest {
        err("E_TREE_NO_MANIFEST", "", format!("an object has a '{MANIFEST}' at its top"));
    }
    for d in &dirs {
        if !nonempty.contains(d) {
            err("E_TREE_EMPTY_DIR", d, "an object has no empty directories".into());
        }
    }
    if source == Source::Listing {
        for e in entries {
            let mut p = e.path.as_str();
            while let Some((parent, _)) = p.rsplit_once('/') {
                if !dirs.contains(parent) {
                    err(
                        "E_TREE_UNLISTED_DIR",
                        &e.path,
                        format!("'{parent}' is not listed as a directory: a member's name may hold a '/'"),
                    );
                    break;
                }
                p = parent;
            }
        }
    }
    if files > MAX_FILES {
        err("E_TREE_FILE_COUNT", "", format!("an object has at most {MAX_FILES} files"));
    }
    if total > MAX_OBJECT_BYTES {
        err("E_TREE_OBJECT_SIZE", "", format!("an object has at most {MAX_OBJECT_BYTES} bytes"));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(entries: &[Entry], source: Source) -> Vec<&'static str> {
        check(entries, source).into_iter().map(|p| p.code).collect()
    }

    #[test]
    fn a_small_object_is_well_formed() {
        let e = [Entry::file("toa.yaml", 20), Entry::dir("net"), Entry::file("net/a.scs", 10)];
        assert!(check(&e, Source::FileSystem).is_empty());
        assert!(check(&e, Source::Listing).is_empty());
    }

    #[test]
    fn names() {
        assert!(component_problem("aux.scs", false).is_some());
        assert!(component_problem("AUX", false).is_some());
        assert!(component_problem("com1.txt", false).is_some());
        assert!(component_problem("com10", false).is_none());
        assert!(component_problem("auxiliary.scs", false).is_none());
        assert!(component_problem(".gitignore", false).is_some());
        assert!(component_problem(".gitignore", true).is_none());
        assert!(component_problem("a.", false).is_some());
        assert!(component_problem("a b", false).is_some());
        assert!(component_problem("_x", false).is_none());
        assert!(component_problem(&"a".repeat(65), false).is_some());
    }

    #[test]
    fn problems_found() {
        let e = [Entry::file("toa.yaml", 1), Entry::file("README.md", 1), Entry::file("readme.md", 1)];
        assert_eq!(codes(&e, Source::FileSystem), ["E_TREE_CASE"]);
        let e = [Entry::file("toa.yaml", 1), Entry::dir("empty")];
        assert_eq!(codes(&e, Source::FileSystem), ["E_TREE_EMPTY_DIR"]);
        let e = [Entry::file("toa.yaml", 1), Entry::file("a/b", 1)];
        assert_eq!(codes(&e, Source::Listing), ["E_TREE_UNLISTED_DIR"]);
        assert!(codes(&e, Source::FileSystem).is_empty());
        let e = [Entry::file("README.md", 1)];
        assert_eq!(codes(&e, Source::FileSystem), ["E_TREE_NO_MANIFEST"]);
        let e = [Entry::file("toa.yaml", 1), Entry::dir("imp_draft-x"), Entry::file("imp_draft-x/a", 1)];
        assert_eq!(codes(&e, Source::FileSystem), ["E_TREE_NESTED_OBJECT"]);
        let mut x = Entry::file("run.sh", 1);
        x.executable = Some(true);
        assert_eq!(codes(&[Entry::file("toa.yaml", 1), x], Source::FileSystem), ["E_TREE_EXECUTABLE"]);
        let deep = "a/b/c/d/e/f/g/h.txt";
        let mut e = vec![Entry::file("toa.yaml", 1), Entry::file(deep, 1)];
        for d in ["a", "a/b", "a/b/c", "a/b/c/d", "a/b/c/d/e", "a/b/c/d/e/f", "a/b/c/d/e/f/g"] {
            e.push(Entry::dir(d));
        }
        assert_eq!(codes(&e, Source::Listing), ["E_TREE_DEPTH"]);
    }
}
