//! Removing an object from a library (6.5), with a removal record when asked (4.2).
//!
//! What is removed is one copy in the working directory of the library: the object directory,
//! the source list beside it, and its notes unless they are kept. A removal record is filled from
//! git, never from the working directory: the object as the publication branch holds it. Every
//! change is undone when a later step fails; the directories are first moved aside and deleted
//! only when everything else succeeded.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use toa_core::library::{MAX_REASON, NOTES_SUFFIX, REMOVED_SUFFIX, STAMPS_SUFFIX};
use toa_core::manifest::{self, Node};
use toa_core::source::{self, Source};
use toa_core::{history, name, tree, Problem};

use crate::git::Git;
use crate::library;
use crate::seal::{scan, shown, Journal, Stop};

fn problem(code: &'static str, path: impl Into<String>, msg: impl Into<String>) -> Problem {
    Problem::error(code, "6.5", path, msg)
}

/// How to remove.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Leave a removal record where the object was (4.2).
    pub record: bool,
    /// The record's `reason`.
    pub reason: Option<String>,
    /// Keep the notes and the stamps directories (6.5 lets them stay or go).
    pub keep_notes: bool,
    /// The publication branch (6.3), which the record's `commit` is taken from.
    pub branch: String,
    /// Only tell what would be done.
    pub dry_run: bool,
    /// This library's address, as `toa publish --to` gives it; by default the remote `origin`.
    /// The record's `sources` leave it out: `commit` and `path` say where the object was here.
    pub to: Option<String>,
}

/// What was removed, paths relative to the root of the library, `/` between levels.
#[derive(Debug, Default)]
pub struct Removed {
    pub object: String,
    /// The object directory that was removed.
    pub path: String,
    /// The removal record written, if any.
    pub record: Option<String>,
    /// Everything removed: the object directory, its source list, its notes directory.
    pub removed: Vec<String>,
    pub notices: Vec<String>,
}

/// Remove one copy of an object from the library that holds `dir`. `target` is an object name,
/// or the path of an object directory (needed when the library keeps the object several times).
pub fn remove(git: &dyn Git, dir: &Path, target: &str, opts: &Options) -> Result<Removed, Stop> {
    let git_err = |e: &dyn std::fmt::Display| Stop::Git(e.to_string());
    let Some(root) = git.toplevel(dir).map_err(|e| git_err(&e))? else {
        return Err(Stop::Problems(vec![problem(
            "E_REMOVE_REPO",
            shown(dir),
            "not in a git repository: a library is one (6.1)",
        )]));
    };
    let copy = find_copy(&root, dir, target)?;
    let object = copy.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let rel = shown(copy.strip_prefix(&root).unwrap_or(&copy));
    let parent = copy.parent().unwrap_or(&root).to_path_buf();
    let hash = name::split(&object).map(|(_, h)| h.to_string()).unwrap_or_default();
    let mut out = Removed { object: object.clone(), path: rel.clone(), ..Default::default() };

    if let Some(r) = &opts.reason {
        if r.chars().count() > MAX_REASON {
            return Err(Stop::Problems(vec![problem(
                "E_REMOVE_REASON",
                &object,
                "the reason is at most 1,000 characters (4.2)",
            )]));
        }
    }

    // the publication branch, and whether git keeps the object at all: files only in the working
    // directory would be lost for good
    let head = match library::publication_head(git, &root, &opts.branch) {
        Ok(h) => h,
        Err(e) => return Err(git_err(&e)),
    };
    let published: Vec<String> = match &head {
        Some(h) => library::objects_in(git, &root, &h.commit, 2_000_000)
            .map_err(|e| git_err(&e))?
            .remove(&object)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let in_head = match git.resolve_commit(&root, "HEAD").map_err(|e| git_err(&e))? {
        Some(c) => git.object_at(&root, &c, &rel).map_err(|e| git_err(&e))?.is_some_and(|id| id == hash),
        None => false,
    };
    if published.is_empty() && !in_head {
        return Err(Stop::Problems(vec![problem(
            "E_REMOVE_UNCOMMITTED",
            &rel,
            "the object is in no commit of this repository, so its files would be lost for good; commit it first, or \
             delete the directory yourself",
        )]));
    }

    // the removal record, from the object as the publication branch holds it
    let record = if opts.record {
        let Some(h) = &head else {
            return Err(Stop::Problems(vec![problem(
                "E_REMOVE_UNPUBLISHED",
                &object,
                format!(
                    "the repository has no commits on '{}', so there is no commit for the record (4.2)",
                    opts.branch
                ),
            )]));
        };
        let Some(at) = published.iter().find(|p| **p == rel).or(published.first()).cloned() else {
            return Err(Stop::Problems(vec![problem(
                "E_REMOVE_UNPUBLISHED",
                &object,
                format!(
                    "not on the publication branch '{}' ({}), so no commit of it holds the object (4.2); remove it \
                     without --record, or publish it first",
                    opts.branch, h.from
                ),
            )]));
        };
        Some(record(git, &root, &object, &h.commit, &at, &copy, opts)?)
    } else {
        None
    };

    // the changes, each undone if a later one fails
    let mut journal = Journal::new(opts.dry_run);
    let io = |j: &mut Journal, e: std::io::Error| j.fail(e.to_string());
    if let Some(bytes) = &record {
        let p = parent.join(format!("{object}{REMOVED_SUFFIX}"));
        if let Err(e) = journal.write(&p, bytes) {
            return Err(io(&mut journal, e));
        }
        out.record = Some(shown(p.strip_prefix(&root).unwrap_or(&p)));
    }
    let list = parent.join(format!("{object}{}", source::SUFFIX));
    if list.is_file() {
        if let Err(e) = journal.remove(&list) {
            return Err(io(&mut journal, e));
        }
        out.removed.push(shown(list.strip_prefix(&root).unwrap_or(&list)));
    }
    // the notes and the timestamp proofs (4.1, 4.3) may stay or go
    let mut aside = vec![copy.clone()];
    for (suffix, what) in [(NOTES_SUFFIX, "notes"), (STAMPS_SUFFIX, "timestamp proofs")] {
        let dir = parent.join(format!("{object}{suffix}"));
        if dir.is_dir() {
            if opts.keep_notes {
                out.notices.push(format!("its {what} stay: {}", shown(dir.strip_prefix(&root).unwrap_or(&dir))));
            } else {
                aside.push(dir);
            }
        }
    }
    let mut moved: Vec<PathBuf> = Vec::new();
    for (i, d) in aside.iter().enumerate() {
        let to = parent.join(format!(".toa-removing-{}-{i}-{object}", std::process::id()));
        if let Err(e) = journal.rename(d, &to) {
            return Err(io(&mut journal, e));
        }
        out.removed.push(shown(d.strip_prefix(&root).unwrap_or(d)));
        moved.push(to);
    }
    out.removed.sort();
    if !opts.dry_run {
        for m in &moved {
            if let Err(e) = std::fs::remove_dir_all(m) {
                out.notices.push(format!("{} could not be deleted ({e}); delete it yourself", shown(m)));
            }
        }
    }
    out.notices.push(format!(
        "run 'toa publish' to update the library list (5.2): {}",
        if record.is_some() { "the object is marked removed there" } else { "its line goes" }
    ));
    Ok(out)
}

/// The object directory meant by `target`: a path, or the one copy of a name under the root.
fn find_copy(root: &Path, dir: &Path, target: &str) -> Result<PathBuf, Stop> {
    let stop = |code, msg: String| Err(Stop::Problems(vec![problem(code, target, msg)]));
    if name::is_object_name(target) {
        let found = scan(root).map_err(|e| Stop::Io { message: e.to_string(), restored: true })?;
        let copies: Vec<PathBuf> =
            found.objects.into_iter().filter(|o| o.file_name().and_then(|n| n.to_str()) == Some(target)).collect();
        return match copies.len() {
            0 => stop("E_REMOVE_MISSING", format!("not found under {}", shown(root))),
            1 => Ok(copies.into_iter().next().unwrap_or_default()),
            n => stop(
                "E_REMOVE_COPIES",
                format!(
                    "kept {n} times ({}); give the path of the copy to remove",
                    copies.iter().map(|p| shown(p.strip_prefix(root).unwrap_or(p))).collect::<Vec<_>>().join(", ")
                ),
            ),
        };
    }
    let p = dir.join(target);
    let leaf = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    if !name::is_object_name(&leaf) || !p.is_dir() {
        return stop("E_REMOVE_MISSING", "neither an object name nor the path of an object directory".into());
    }
    match crate::fs::under(root, &p) {
        Some(q) if q != root => Ok(q),
        _ => stop("E_REMOVE_MISSING", format!("not inside the library {}", shown(root))),
    }
}

/// The removal record (4.2), its fields in the order of the table there.
fn record(
    git: &dyn Git,
    root: &Path,
    object: &str,
    commit: &str,
    path: &str,
    copy: &Path,
    opts: &Options,
) -> Result<Vec<u8>, Stop> {
    let read = |file: &str, max: u64| -> Result<Option<Vec<u8>>, Stop> {
        let id = git.object_at(root, commit, &format!("{path}/{file}")).map_err(|e| Stop::Git(e.to_string()))?;
        match id {
            Some(id) => git.read_blob(root, &id, max).map(Some).map_err(|e| Stop::Git(e.to_string())),
            None => Ok(None),
        }
    };
    let bad = |m: String| Stop::Problems(vec![problem("E_REMOVE_READ", object, m)]);
    let mb = read(tree::MANIFEST, manifest::MAX_BYTES as u64)?
        .ok_or_else(|| bad(format!("no toa.yaml at {path} in {commit}")))?;
    let m = manifest::parse(&mb).map_err(|e| bad(format!("its toa.yaml in {commit} cannot be read: {e:?}")))?;
    let hist = match read(tree::HISTORY, 1024 * 1024)? {
        Some(b) => Some(history::read(&b).map_err(|e| bad(format!("its history.json: {e}")))?),
        None => None,
    };
    let names = |n: &Node| -> Option<Vec<Value>> {
        let v: Vec<Value> = n.as_seq()?.iter().filter_map(|x| x.as_str().map(|s| Value::from(s.to_string()))).collect();
        (!v.is_empty()).then_some(v)
    };

    // the label and the git sources of other places, from the lists beside the copy and of the
    // library; this library's own address is left out, as commit and path say where it was here
    let beside = sidecar_line(copy, object);
    let label = beside.as_ref().and_then(|l| l.label.clone()).or_else(|| library_label(root, object)).or_else(|| {
        let generation = toa_types::readable::generation(hist.as_ref());
        toa_types::readable::label(object, Some(&m), generation, path, &|_| None)
    });
    let own = opts
        .to
        .clone()
        .or_else(|| git.remote_url(root, "origin").ok().flatten())
        .map(|u| crate::publish::library_address(&u).unwrap_or(u));
    let sources: Vec<Value> = beside
        .map(|l| l.sources)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| match s {
            Source::Git { git: a, commit, path } if !own.as_deref().is_some_and(|o| source::same_address(o, &a)) => {
                let mut v = Map::new();
                v.insert("git".into(), a.into());
                if let Some(c) = commit {
                    v.insert("commit".into(), c.into());
                }
                if let Some(p) = path {
                    v.insert("path".into(), p.into());
                }
                Some(Value::Object(v))
            }
            _ => None,
        })
        .take(source::MAX_SOURCES)
        .collect();

    let mut r = Map::new();
    r.insert("toa".into(), "removed/1".into());
    r.insert("object".into(), object.into());
    r.insert("commit".into(), commit.into());
    r.insert("path".into(), path.into());
    if let Some(l) = m.get("license").and_then(Node::as_str) {
        r.insert("license".into(), l.into());
    }
    if let Some(l) = label {
        r.insert("label".into(), l.into());
    }
    if let Some(b) = m.get("base").and_then(names) {
        r.insert("base".into(), Value::Array(b));
    }
    if let Some(h) = hist {
        r.insert("history".into(), h.history.into());
    }
    if let Some(t) = &opts.reason {
        r.insert("reason".into(), t.as_str().into());
    }
    if !sources.is_empty() {
        r.insert("sources".into(), Value::Array(sources));
    }
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(r)).unwrap_or_default();
    bytes.push(b'\n');
    if let Err(ms) = library::read_removal(&bytes, object) {
        return Err(bad(format!("the record would not keep 4.2: {}", ms.join("; "))));
    }
    Ok(bytes)
}

/// The object's own line in the source list beside a copy.
fn sidecar_line(copy: &Path, object: &str) -> Option<source::ObjectLine> {
    let bytes = std::fs::read(copy.with_file_name(format!("{object}{}", source::SUFFIX))).ok()?;
    let r = source::read(&bytes, false, source::Kind::Sidecar, source::MAX_SIDECAR_LIST, "");
    source::merged_objects(&r.lines).into_iter().find(|l| l.object == object)
}

/// The object's label in the library list or its volumes.
fn library_label(root: &Path, object: &str) -> Option<String> {
    let mut files = vec![root.join(source::LIBRARY_LIST)];
    if let Ok(rd) = std::fs::read_dir(root.join(source::LIBRARY_VOLUMES)) {
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        files.extend(v);
    }
    files.iter().find_map(|f| {
        let bytes = std::fs::read(f).ok()?;
        let r = source::read(&bytes, false, source::Kind::Library, source::MAX_VOLUME, "");
        source::merged_objects(&r.lines).into_iter().find(|l| l.object == object)?.label
    })
}
