//! The parts of the library check (6.4) that need only git's listing: where the objects are,
//! verifying an object from its listing (7.4), the rules on `.gitattributes` and `.gitmodules`
//! (6.1), the names outside objects and the path budget (6.2), and sidecar names (4).

use crate::hash::{self, Algorithm};
use crate::name::{self, Form};
use crate::problem::Problem;
use crate::tree::{self, Entry, Kind, Source};

/// One entry of a recursive listing of a commit (`git ls-tree -r -t -l`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The mode as git prints it: `100644`, `100755`, `120000`, `040000`, `160000`.
    pub mode: String,
    /// `blob`, `tree` or `commit`.
    pub kind: String,
    pub id: String,
    /// The size of a blob.
    pub size: Option<u64>,
    /// The path from the root, `/` between levels. A name that is not UTF-8 is replaced lossily,
    /// and `utf8` is false.
    pub path: String,
    pub utf8: bool,
}

impl Listed {
    pub fn is_tree(&self) -> bool {
        self.kind == "tree"
    }
    pub fn is_blob(&self) -> bool {
        self.kind == "blob"
    }
    /// A regular file: a blob with mode `100644`.
    pub fn is_regular_file(&self) -> bool {
        self.is_blob() && self.mode == "100644"
    }
    pub fn leaf(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or("")
    }
    pub fn parent(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(p, _)| p)
    }
}

/// The path budget of 6.2.
pub const MAX_BEFORE_OBJECT: usize = 60;
pub const MAX_IN_LIBRARY: usize = 190;

/// A file at the root of a library.
pub const ATTRIBUTES: &str = ".gitattributes";
pub const MODULES: &str = ".gitmodules";
pub const LIBRARY_YAML: &str = "library.yaml";

/// The kinds of sidecar file (4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sidecar {
    Notes,
    SourceList,
    Removal,
    /// Timestamp proofs (4.3).
    Stamps,
}

pub const NOTES_SUFFIX: &str = ".notes";
pub const REMOVED_SUFFIX: &str = ".removed.json";
pub const STAMPS_SUFFIX: &str = ".stamps";
pub const MAX_NOTE: u64 = 256 * 1024;
pub const MAX_REMOVAL: u64 = 1024 * 1024;
pub const MAX_REASON: usize = 1000;

/// A name that starts with an object name or a draft name and a dot: the name, and the kind of
/// sidecar when the rest is one of the four (4).
pub fn sidecar_of(leaf: &str) -> Option<(&str, Option<Sidecar>)> {
    let dot = leaf.find('.')?;
    let (base, rest) = leaf.split_at(dot);
    if !matches!(name::form(base), Form::Object | Form::Draft) {
        return None;
    }
    let kind = match rest {
        NOTES_SUFFIX => Some(Sidecar::Notes),
        crate::source::SUFFIX => Some(Sidecar::SourceList),
        REMOVED_SUFFIX => Some(Sidecar::Removal),
        STAMPS_SUFFIX => Some(Sidecar::Stamps),
        _ => None,
    };
    Some((base, kind))
}

/// Whether a path is inside a `.notes` or `.stamps` directory (at any depth): nothing in one is
/// read as an object, a draft's sidecar or a sidecar (6.4 item 5, decisions 11-25), so `<object
/// name>.ots` may be a proof there. Whether the path is inside an object is for the caller.
pub fn inside_sidecar_dir(path: &str) -> bool {
    let Some((dirs, _)) = path.rsplit_once('/') else { return false };
    dirs.split('/').any(|c| matches!(sidecar_of(c), Some((_, Some(Sidecar::Notes | Sidecar::Stamps)))))
}

/// An object directory found in a listing.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: String,
    pub name: String,
    pub tree: String,
    /// Indexes into the listing of everything under it.
    pub members: Vec<usize>,
}

/// The object directories of a listing: trees whose name is an object name, not inside another
/// object (an object inside an object is the outer object's problem, 3.1 item 5).
pub fn objects(listing: &[Listed]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut by_path: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (i, e) in listing.iter().enumerate() {
        // inside an object found already?
        let mut p = e.parent();
        let mut owner = None;
        while !p.is_empty() {
            if let Some(&k) = by_path.get(p) {
                owner = Some(k);
            }
            p = p.rsplit_once('/').map_or("", |(a, _)| a);
        }
        if let Some(k) = owner {
            out[k].members.push(i);
            continue;
        }
        // a directory of a sidecar directory named as an object is not one (6.4 item 5)
        if e.is_tree() && name::is_object_name(e.leaf()) && !inside_sidecar_dir(&e.path) {
            by_path.insert(e.path.clone(), out.len());
            out.push(Found {
                path: e.path.clone(),
                name: e.leaf().to_string(),
                tree: e.id.clone(),
                members: Vec::new(),
            });
        }
    }
    out
}

/// Whether a listing path is inside one of the object directories.
pub fn inside_object(path: &str, objects: &[Found]) -> bool {
    objects
        .iter()
        .any(|o| path.len() > o.path.len() && path.starts_with(&o.path) && path.as_bytes()[o.path.len()] == b'/')
}

/// Verify one object from the listing (7.4; 6.4 item 2): the stored tree is the hash in the
/// name; the tree rebuilt from the listed files has that hash too; the tree is well-formed.
/// Problems are given with the path of the object in the library.
pub fn verify_listing(found: &Found, listing: &[Listed]) -> Vec<Problem> {
    let mut out = Vec::new();
    let at = |rel: &str| if rel.is_empty() { found.path.clone() } else { format!("{}/{rel}", found.path) };
    let want = name::split(&found.name).map(|(_, h)| h).unwrap_or("");
    if found.tree != want {
        out.push(Problem::error(
            "E_LIB_TREE_HASH",
            "6.4",
            at(""),
            format!("the stored tree is {}, not the hash in the name", found.tree),
        ));
    }
    let strip = found.path.len() + 1;
    let mut entries = Vec::new();
    let mut rebuilt = hash::Dir::new();
    let mut rebuilt_ok = true;
    for &i in &found.members {
        let e = &listing[i];
        let rel = e.path.get(strip..).unwrap_or("").to_string();
        if !e.utf8 {
            out.push(Problem::error("E_TREE_NAME", "3.1", at(&rel), "a name that is not UTF-8"));
        }
        let entry = match (e.kind.as_str(), e.mode.as_str()) {
            ("tree", _) => Entry::dir(rel.clone()),
            ("blob", "100644" | "100755") => {
                let mut f = Entry::file(rel.clone(), e.size.unwrap_or(0));
                f.executable = Some(e.mode == "100755");
                f
            }
            _ => Entry { path: rel.clone(), kind: Kind::Other, size: 0, executable: None },
        };
        entries.push(entry);
        if e.is_blob() {
            let inserted = hash::from_hex(&e.id).map(|id| rebuilt.insert_file(&rel, id).is_ok());
            if inserted != Some(true) {
                rebuilt_ok = false;
            }
        }
    }
    for mut p in tree::check(&entries, Source::Listing) {
        p.path = at(&p.path);
        out.push(p);
    }
    let rebuilt = if rebuilt_ok { rebuilt.id(Algorithm::Sha1).ok().map(|id| hash::hex(&id)) } else { None };
    if rebuilt.as_deref() != Some(found.tree.as_str()) {
        out.push(Problem::error(
            "E_LIB_TREE_FORM",
            "7.4",
            at(""),
            "the tree rebuilt from the listed files does not have the stored hash: the stored tree is not in \
             canonical form (modes with a leading 0, entries out of order or twice, names with '/', empty \
             directories)",
        ));
    }
    out
}

/// The problems of the content of `.gitattributes` (6.1): it has a line `* -text`, and no line
/// sets `text`, `eol`, `crlf`, `filter`, `ident` or `working-tree-encoding` otherwise, or
/// defines a macro.
pub fn attributes_problems(bytes: &[u8]) -> Vec<String> {
    const SET: [&str; 6] = ["text", "eol", "crlf", "filter", "ident", "working-tree-encoding"];
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut has = false;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("[attr]") {
            out.push(format!("line {}: defines a macro", n + 1));
            continue;
        }
        let mut words = line.split_ascii_whitespace();
        let pattern = words.next().unwrap_or("");
        let attrs: Vec<&str> = words.collect();
        if pattern == "*" && attrs == ["-text"] {
            has = true;
            continue;
        }
        for a in attrs {
            let bare = a.trim_start_matches(['-', '!']);
            let bare = bare.split('=').next().unwrap_or("");
            if SET.contains(&bare) {
                out.push(format!("line {}: sets '{bare}'", n + 1));
            }
        }
    }
    if !has {
        out.push("there is no line '* -text'".into());
    }
    out
}

/// The path budget (6.2) of an object directory in a library: the path before it.
pub fn budget_before_object(object_path: &str) -> Option<String> {
    let before = object_path.rsplit_once('/').map_or(0, |(p, _)| p.len() + 1);
    (before > MAX_BEFORE_OBJECT).then(|| {
        format!("the path before the object directory has {before} characters; at most {MAX_BEFORE_OBJECT} (6.2)")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(mode: &str, kind: &str, id: &str, path: &str) -> Listed {
        Listed {
            mode: mode.into(),
            kind: kind.into(),
            id: id.into(),
            size: (kind == "blob").then_some(1),
            path: path.into(),
            utf8: true,
        }
    }

    #[test]
    fn sidecar_names() {
        let n = "imp_1ec6249c1ec6249c1ec6249c1ec6249c1ec6249c";
        assert_eq!(sidecar_of(&format!("{n}.notes")), Some((n, Some(Sidecar::Notes))));
        assert_eq!(sidecar_of(&format!("{n}.source.jsonl")), Some((n, Some(Sidecar::SourceList))));
        assert_eq!(sidecar_of(&format!("{n}.zip")), Some((n, None)));
        assert_eq!(sidecar_of("imp_draft-a.notes"), Some(("imp_draft-a", Some(Sidecar::Notes))));
        assert_eq!(sidecar_of("README.md"), None);
    }

    #[test]
    fn attributes() {
        assert!(attributes_problems(b"* -text\n").is_empty());
        assert!(attributes_problems(b"# x\n* -text\n*.png binary\n").is_empty());
        // the line is not '* -text', and it sets text and eol
        assert_eq!(attributes_problems(b"* -text eol=lf\n").len(), 3);
        assert_eq!(attributes_problems(b"* -text\n*.v text\n[attr]x -text\n").len(), 2);
    }

    #[test]
    fn objects_are_the_outermost() {
        let h = "ea44863a8d2f242bc651caf709d45a27e7531d9d";
        let o = format!("doc_{h}");
        let l = vec![
            listed("040000", "tree", "0", "doc"),
            listed("040000", "tree", h, &format!("doc/{o}")),
            listed("100644", "blob", "1", &format!("doc/{o}/toa.yaml")),
            listed("040000", "tree", h, &format!("doc/{o}/{o}")),
        ];
        let found = objects(&l);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members, vec![2, 3]);
        assert!(inside_object(&format!("doc/{o}/x"), &found));
        assert!(!inside_object(&format!("doc/{o}x"), &found));
    }
}
