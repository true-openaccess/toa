//! `toa derive`: copy an object into a new draft whose `base` points back to it (3.4, 2.4), the
//! first step of "copy, write base, change, seal".
//!
//! The object is verified from its files first (7.4). The draft gets the files of the object
//! directory but `history.json`, which sealing writes again from `base`; nothing beside the
//! object (notes, timestamp proofs, source list, removal record) is copied, as it is not in the
//! object directory. In `toa.yaml` the top-level `base` becomes `[<the object>]`: the lines of a
//! `base` written are replaced, else the line is added at the end; every other byte stays.
//!
//! The draft is `<prefix>_draft-<tag>`: the tag given, else the manifest's `name` with `_` as
//! `-` when that is a tag (2.4), else `from-` and the first 8 hex digits of the object's hash. It
//! is written into a directory given, else the current one. A directory of that name there, or a
//! draft of that name anywhere in the repository it lands in, stops it. It is written beside the
//! place under a temporary name and renamed into place: it completes, or leaves nothing.

use std::path::{Path, PathBuf};

use toa_core::problem::{has_errors, Problem};
use toa_core::{draft, manifest, name, tree};

/// How to derive.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub tag: Option<String>,
    /// The directory the draft is written into.
    pub into: PathBuf,
    pub dry_run: bool,
}

/// What was (or would be) written.
#[derive(Debug, Clone)]
pub struct Derived {
    pub object: String,
    pub draft: String,
    pub path: PathBuf,
    /// Whether `base` was written before (and replaced) or added.
    pub base_replaced: bool,
    pub files: usize,
}

/// Why nothing was written.
#[derive(Debug)]
pub enum DeriveError {
    /// A tag that is not one (2.4): a usage error.
    BadTag(String),
    /// The object does not verify, or the draft cannot be made: problems found.
    Problems(Vec<Problem>),
    /// Files could not be read or written.
    Io(String),
}

impl std::fmt::Display for DeriveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeriveError::BadTag(t) => write!(f, "'{t}' is not a draft tag (2.4)"),
            DeriveError::Problems(ps) => {
                write!(f, "{}", ps.iter().map(|p| p.message.clone()).collect::<Vec<_>>().join("; "))
            }
            DeriveError::Io(m) => write!(f, "{m}"),
        }
    }
}

/// The tag a draft of this object gets when none is given.
pub fn default_tag(object: &str, root: &manifest::Node) -> String {
    let from_name = root.get("name").and_then(manifest::Node::as_str).map(|n| n.replace('_', "-"));
    match from_name.filter(|t| name::is_tag(t)) {
        Some(t) => t,
        None => format!("from-{}", name::split(object).map(|(_, h)| &h[..8]).unwrap_or("object")),
    }
}

/// The top of the repository a directory is in, by its `.git`, without git.
fn repository_of(dir: &Path) -> Option<PathBuf> {
    let mut d = std::path::absolute(dir).ok()?;
    loop {
        if d.join(".git").exists() {
            return Some(d);
        }
        if !d.pop() {
            return None;
        }
    }
}

/// Derive a draft from the object directory `from`.
pub fn derive(from: &Path, opts: &Options) -> Result<Derived, DeriveError> {
    let object = from.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let err = |code: &'static str, section: &'static str, m: String| {
        DeriveError::Problems(vec![Problem::error(code, section, object.clone(), m)])
    };
    if !name::is_object_name(&object) {
        return Err(err("E_DERIVE_NAME", "2.1", format!("{} is not an object directory", from.display())));
    }
    if let Some(t) = &opts.tag {
        if !name::is_tag(t) {
            return Err(DeriveError::BadTag(t.clone()));
        }
    }
    // 7.4: the object as its files say
    let v = crate::fs::verify_dir(from).map_err(|e| DeriveError::Io(format!("{}: {e}", from.display())))?;
    if has_errors(&v.problems) {
        return Err(DeriveError::Problems(v.problems));
    }
    let contents = crate::fs::read_dir(from).map_err(|e| DeriveError::Io(format!("{}: {e}", from.display())))?;
    let Some((_, bytes)) = contents.files.iter().find(|(p, _)| p == tree::MANIFEST) else {
        return Err(err("E_DERIVE_MANIFEST", "3.2", "the object has no toa.yaml".into()));
    };
    let root = manifest::parse(bytes).map_err(DeriveError::Problems)?;
    let base_replaced = root.get("base").is_some();
    let manifest_bytes = draft::set_base(bytes, &root, &object).map_err(|m| err("E_DERIVE_BASE", "3.4", m))?;

    let prefix = name::prefix_of(&object).unwrap_or("");
    let tag = opts.tag.clone().unwrap_or_else(|| default_tag(&object, &root));
    let draft_name = format!("{prefix}_draft-{tag}");
    let target = opts.into.join(&draft_name);
    if std::fs::symlink_metadata(&target).is_ok() {
        return Err(err(
            "E_DERIVE_EXISTS",
            "2.4",
            format!("{} is there already; give another tag with --tag", target.display()),
        ));
    }
    if let Some(repo) = repository_of(&opts.into) {
        if let Ok(scan) = crate::seal::scan(&repo) {
            if let Some(d) = scan.drafts.iter().find(|d| d.file_name().and_then(|n| n.to_str()) == Some(&draft_name)) {
                return Err(err(
                    "E_DERIVE_EXISTS",
                    "2.4",
                    format!(
                        "the repository has a draft {draft_name} already, at {}; give another tag with --tag",
                        d.display()
                    ),
                ));
            }
        }
    }

    let files: Vec<(&String, &[u8])> = contents
        .files
        .iter()
        .filter(|(p, _)| p != tree::HISTORY)
        .map(|(p, b)| (p, if p == tree::MANIFEST { manifest_bytes.as_slice() } else { b.as_slice() }))
        .collect();
    let derived = Derived {
        object: object.clone(),
        draft: draft_name.clone(),
        path: target.clone(),
        base_replaced,
        files: files.len(),
    };
    if opts.dry_run {
        return Ok(derived);
    }

    // written under a temporary name beside the place, then renamed: all or nothing
    let created = !opts.into.exists();
    let tmp = opts.into.join(format!(".toa-derive-{}-{draft_name}", std::process::id()));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&opts.into)?;
        std::fs::create_dir(&tmp)?;
        for d in &contents.dirs {
            std::fs::create_dir_all(tmp.join(d))?;
        }
        for (rel, b) in &files {
            std::fs::write(tmp.join(rel.as_str()), b)?;
        }
        std::fs::rename(&tmp, &target)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&tmp);
        if created {
            let _ = std::fs::remove_dir_all(&opts.into);
        }
        return Err(DeriveError::Io(format!("writing {}: {e}", target.display())));
    }
    Ok(derived)
}
