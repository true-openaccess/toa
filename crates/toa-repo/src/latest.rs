//! `toa latest`: mark objects of a library `latest` in its library list, or take the mark away
//! (5.2; object structure 14). `toa publish` gives and moves the marks of objects the list has
//! not had yet; this is for the others, such as a library published before it did.
//!
//! Marking an object takes the mark from the other objects of its series in the same lineage
//! (one in the other's `history`), which the library check would find (6.4); objects of the
//! series on other branches keep theirs, and the user is told. A series is the type and the
//! `name`; for a type without a name, the lineage alone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toa_core::manifest::{self, Node};
use toa_core::source::{self, Kind, Line};
use toa_core::{history, name, tree, Problem};

use crate::git::Git;
use crate::library;
use crate::publish::{commit_and_check, CommitError};
use crate::seal::{self, shown, Journal, Stop};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Take the mark away instead of giving it.
    pub off: bool,
    /// Mark, in every series of the library that has no mark yet, the objects no other object of
    /// the series builds on (its newest generations); types without a name are left out.
    pub all: bool,
    pub dry_run: bool,
    /// Commit the lists changed, signed as git is set (or must be, with `sign`), and run the
    /// library check on the commit.
    pub commit: bool,
    pub sign: bool,
    /// The publication branch, for the base of the library check.
    pub branch: String,
}

#[derive(Debug, Default)]
pub struct Marked {
    /// The objects marked, and those whose mark was taken away.
    pub marked: Vec<String>,
    pub unmarked: Vec<String>,
    /// Lists written, relative to the root.
    pub changed: Vec<String>,
    pub notices: Vec<String>,
    pub commit: Option<String>,
    pub signature: Option<crate::git::SignatureFormat>,
    pub check: Vec<Problem>,
}

fn problem(code: &'static str, path: impl Into<String>, msg: impl Into<String>) -> Problem {
    Problem::error(code, "5.2", path, msg)
}

/// What the tool knows of an object on disk: its series and its history.
struct Known {
    series: Option<(String, String)>,
    history: Vec<String>,
}

pub fn latest(git: &dyn Git, dir: &Path, wanted: &[String], opts: &Options) -> Result<Marked, Stop> {
    let root = match git.toplevel(dir) {
        Ok(Some(r)) => r,
        Ok(None) => dir.to_path_buf(),
        Err(e) => return Err(Stop::Git(e.to_string())),
    };
    let io = |e: std::io::Error| Stop::Io { message: e.to_string(), restored: true };

    // the library list and its volumes, as they are
    let main = root.join(source::LIBRARY_LIST);
    if !main.is_file() {
        return Err(Stop::Problems(vec![problem(
            "E_LATEST_NO_LIST",
            source::LIBRARY_LIST,
            "this library has no library list yet: run toa publish first",
        )]));
    }
    let mut files: Vec<PathBuf> = vec![main];
    if let Ok(rd) = std::fs::read_dir(root.join(source::LIBRARY_VOLUMES)) {
        let mut v: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.to_string_lossy().ends_with(source::SUFFIX))
            .collect();
        v.sort();
        files.extend(v);
    }
    let mut lists: Vec<(PathBuf, Vec<u8>, Vec<Line>)> = Vec::new();
    for f in files {
        let b = std::fs::read(&f).map_err(io)?;
        let lines = source::read(&b, false, Kind::Library, source::MAX_VOLUME, "").lines;
        lists.push((f, b, lines));
    }
    let listed: BTreeMap<String, bool> = {
        let all: Vec<Line> = lists.iter().flat_map(|(_, _, l)| l.iter().cloned()).collect();
        source::merged_objects(&all).into_iter().filter(|o| !o.removed).map(|o| (o.object, o.latest)).collect()
    };

    // the objects on disk: their series and histories
    let sc = seal::scan(&root).map_err(io)?;
    let mut known: BTreeMap<String, Known> = BTreeMap::new();
    for o in &sc.objects {
        let Some(n) = o.file_name().and_then(|x| x.to_str()).map(str::to_string) else { continue };
        if known.contains_key(&n) {
            continue;
        }
        let m = std::fs::read(o.join(tree::MANIFEST)).ok().and_then(|b| manifest::parse(&b).ok());
        let own = m.as_ref().and_then(|m| m.get("name")).and_then(Node::as_str);
        let series = match (name::prefix_of(&n), own) {
            (Some(p), Some(s)) => Some((p.to_string(), s.to_string())),
            _ => None,
        };
        let history = std::fs::read(o.join(tree::HISTORY))
            .ok()
            .and_then(|b| history::read(&b).ok())
            .map(|h| h.history)
            .unwrap_or_default();
        known.insert(n, Known { series, history });
    }

    // what the user names: object names, or paths of object directories; or, with `all`, the
    // newest of every series not marked yet
    let mut names: Vec<String> = Vec::new();
    // one in the history of the other
    let lineage = |a: &str, b: &str| {
        known.get(a).is_some_and(|k| k.history.iter().any(|h| h == b))
            || known.get(b).is_some_and(|k| k.history.iter().any(|h| h == a))
    };
    // one series (3.4): one type and the same name (for a type without name, none), and linked by
    // base: one in the history of the other, or both from one ancestor. Objects that only share
    // a name, such as circuits of different interfaces, are not
    let same_series = |a: &str, b: &str| {
        let (Some(ka), Some(kb)) = (known.get(a), known.get(b)) else { return false };
        name::prefix_of(a) == name::prefix_of(b)
            && ka.series == kb.series
            && (lineage(a, b) || ka.history.iter().any(|h| kb.history.contains(h)))
    };
    if opts.all {
        let members: Vec<&String> = known
            .iter()
            .filter(|(n, k)| listed.contains_key(n.as_str()) && k.series.is_some())
            .map(|(n, _)| n)
            .collect();
        // the series: groups of objects linked to each other
        let mut group: Vec<usize> = (0..members.len()).collect();
        fn root(g: &mut [usize], mut i: usize) -> usize {
            while g[i] != i {
                g[i] = g[g[i]];
                i = g[i];
            }
            i
        }
        for i in 0..members.len() {
            for j in i + 1..members.len() {
                if same_series(members[i], members[j]) {
                    let (a, b) = (root(&mut group, i), root(&mut group, j));
                    group[a] = b;
                }
            }
        }
        let mut series: BTreeMap<usize, Vec<&String>> = BTreeMap::new();
        for (i, n) in members.iter().enumerate() {
            let r = root(&mut group, i);
            series.entry(r).or_default().push(n);
        }
        for s in series.values() {
            if s.iter().any(|n| listed[n.as_str()]) {
                continue; // the maintainer marked this series already
            }
            for n in s {
                if !s.iter().any(|o| o != n && known[o.as_str()].history.contains(n)) {
                    names.push((*n).clone());
                }
            }
        }
    }
    let mut problems = Vec::new();
    for w in wanted {
        let leaf = w.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").to_string();
        if !name::is_object_name(&leaf) {
            problems.push(problem("E_LATEST_NAME", w.as_str(), "not an object name, nor the path of an object"));
        } else if !listed.contains_key(&leaf) {
            problems.push(problem(
                "E_LATEST_NAME",
                w.as_str(),
                format!("{leaf} has no line in the library list: publish it first (toa publish)"),
            ));
        } else if !names.contains(&leaf) {
            names.push(leaf);
        }
    }
    if !problems.is_empty() {
        return Err(Stop::Problems(problems));
    }

    // the marks after this
    let mut marks: BTreeSet<String> = listed.iter().filter(|(_, m)| **m).map(|(n, _)| n.clone()).collect();
    let before = marks.clone();
    let mut out = Marked::default();
    for n in &names {
        if opts.off {
            marks.remove(n);
            continue;
        }
        if !known.contains_key(n) {
            out.notices.push(format!("{n}: its directory is not here, so its series cannot be told"));
        }
        let others: Vec<String> = marks.iter().filter(|o| *o != n && same_series(n, o)).cloned().collect();
        for o in others {
            if lineage(n, &o) {
                marks.remove(&o);
            } else {
                out.notices.push(format!(
                    "{o} of the same series stays marked: it is on another branch of the lineage (6.4 only warns)"
                ));
            }
        }
        marks.insert(n.clone());
    }
    out.marked = marks.difference(&before).cloned().collect();
    out.unmarked = before.difference(&marks).cloned().collect();

    // write the lines that change
    let mut journal = Journal::new(opts.dry_run);
    for (path, old, lines) in &mut lists {
        let mut changed = false;
        for l in lines.iter_mut() {
            if let Line::Object(o) = l {
                let want = marks.contains(&o.object) && !o.removed;
                if o.latest != want {
                    o.latest = want;
                    changed = true;
                }
            }
        }
        if !changed {
            continue;
        }
        let text = source::write(lines);
        if text.as_bytes() == old.as_slice() {
            continue;
        }
        if let Err(e) = journal.write(path, text.as_bytes()) {
            return Err(journal.fail(e.to_string()));
        }
        out.changed.push(shown(path.strip_prefix(&root).unwrap_or(path)));
    }

    if opts.commit && !opts.dry_run && !out.changed.is_empty() {
        let head = library::publication_head(git, &root, &opts.branch).ok().flatten().map(|h| h.commit);
        let msg = format!(
            "Mark {} and unmark {} object(s) latest in the library list (toa latest)",
            out.marked.len(),
            out.unmarked.len()
        );
        match commit_and_check(git, &root, &out.changed, &msg, opts.sign, head.as_deref(), &mut out.notices) {
            Ok(c) => {
                out.commit = Some(c.commit);
                out.signature = c.signature;
                out.check = c.check;
            }
            Err(CommitError::Commit(e)) => return Err(journal.fail(format!("the commit failed: {e}"))),
            Err(CommitError::After(e)) => return Err(Stop::Git(e)),
        }
    }
    Ok(out)
}
