//! The same files without a declared lineage (8.3): a hint for people, never a reason to judge an
//! object invalid. One computation serves `toa index`, which writes it into each object of the
//! index (`same_files`, doc/index-format.md), and `toa check`, which turns it into `I_SAME_FILES`
//! hints for the library checked.
//!
//! An object O holds a file byte for byte the same (the same git blob) as a file of an earlier
//! object P. The four conditions of 8.3, as this tool applies them:
//!
//! - **P is earlier than O:** when one library holds both, the first of them in the order of the
//!   libraries read decides: the order in which the two first appeared along first parents of
//!   its publication branch (`basis` `library`). Otherwise the committer dates of the commits that
//!   first held them, the earliest of each over its libraries (`basis` `dates`); those dates are
//!   written by their committers, as the hint says. First held by the same commit, or an order or
//!   a date missing: the order cannot be told, and there is no hint.
//! - **The authors differ:** the two `author` fields with the white space at both ends removed,
//!   each run of white space inside taken as one space, and the case of letters ignored. Only one
//!   written: they differ. Neither written: no hint. The `authors` of a circuit's `origin` take no
//!   part.
//! - **The two are not related:** neither is in the `base` or the `history` of the other, and
//!   neither refers to the other (3.3: an object name in the manifest, in `lock.json`, or an
//!   object of `members.source.jsonl`).
//! - **The file is not left out.** Not compared:
//!   - licence texts: names starting with `LICENSE` or `COPYING`, and files under a directory
//!     `LICENSES/`;
//!   - `history.json` and `lock.json` at the top of the object;
//!   - files shorter than [`MIN_SIZE`] (256) bytes;
//!   - a file of O that an object of O's lineage (its `base` and `history`) found here holds too;
//!   - **a common file**, such as a template or a common include: the number 8.3 leaves to the
//!     tool. The objects holding the file fall into groups: two objects related as above (by
//!     lineage or by a reference, either way) are in one group. The file is common when it is
//!     held by objects of more than [`COMMON_GROUPS`] (5) groups, or of more than
//!     [`COMMON_AUTHORS`] (5) authors (an `author` not written counts as one more author), or by
//!     [`COMMON_CAP`] (200) objects or more, without grouping them. At most 5 objects: never
//!     common. The generations and ports of one design holding the same file are one group, so
//!     they do not make a file common. The same algorithm as gazetteer's `authorship.py`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toa_core::library::{self as lib, Listed};
use toa_core::manifest::Node;
use toa_core::problem::Problem;
use toa_core::source::{Line, Source};
use toa_core::{history, manifest, object, tree};

use crate::git::Git;

/// The code of the hint (8.3 gives none).
pub const CODE: &str = "I_SAME_FILES";
/// Files smaller than this are left out: they match by chance.
pub const MIN_SIZE: u64 = 256;
/// A file held by objects of more groups than this (objects related to each other are one
/// group) is a common file and left out.
pub const COMMON_GROUPS: usize = 5;
/// A file held by objects of more authors than this is a common file.
pub const COMMON_AUTHORS: usize = 5;
/// A file held by this many objects or more is a common file, without grouping them.
pub const COMMON_CAP: usize = 200;
/// The earlier and the later objects written into the index per object, each way.
pub const LISTED: usize = 50;

/// Where an object was found: a library (an index into [`Found::libraries`]), its path there, the
/// position along first parents of the commit that first held it, and that commit's date.
#[derive(Debug, Clone)]
pub struct Place {
    pub library: usize,
    pub path: String,
    pub order: Option<usize>,
    pub date: Option<i64>,
}

/// One object, with every library it was found in.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub name: String,
    pub author: Option<String>,
    /// Every object name it refers to or descends from: its manifest, `history.json`, `lock.json`
    /// and `members.source.jsonl`.
    pub named: BTreeSet<String>,
    /// Its lineage: `base` and `history`.
    pub lineage: BTreeSet<String>,
    /// (path in the object, blob id, size).
    pub files: Vec<(String, String, u64)>,
    pub places: Vec<Place>,
}

/// The objects read, from libraries in order: for `toa check`, the library checked first.
#[derive(Debug, Default)]
pub struct Found {
    pub libraries: Vec<String>,
    pub objects: Vec<Candidate>,
    by_name: BTreeMap<String, usize>,
}

impl Found {
    /// A library read, by its label; its index.
    pub fn library(&mut self, label: &str) -> usize {
        self.libraries.push(label.to_string());
        self.libraries.len() - 1
    }

    /// The index of an object found, by its name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    /// An object found; one found before by the same name gains its places.
    pub fn add(&mut self, c: Candidate) {
        if let Some(&i) = self.by_name.get(&c.name) {
            self.objects[i].places.extend(c.places);
        } else {
            self.by_name.insert(c.name.clone(), self.objects.len());
            self.objects.push(c);
        }
    }
}

/// How P is known to be earlier than O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Along first parents of this library, which holds both.
    Library(usize),
    /// By the committer dates.
    Dates,
}

/// The other object of a pair, how the order was told, and the files: (path here, path there).
#[derive(Debug, Clone)]
pub struct Pair {
    pub object: usize,
    pub basis: Basis,
    pub files: Vec<(String, String)>,
}

/// What 8.3 says of one object: the earlier objects with the same files, earliest first; the
/// later objects that hold its files; and the paths of its files that are common files.
#[derive(Debug, Clone, Default)]
pub struct Same {
    pub earlier: Vec<Pair>,
    pub later: Vec<Pair>,
    pub common: BTreeSet<String>,
}

/// Read the objects of a library at a commit. Problems reading it leave objects out; this is
/// only a hint.
pub fn read_library(git: &dyn Git, repo: &Path, commit: &str, label: &str, into: &mut Found) {
    let idx = into.library(label);
    let Ok(listing) = git.list_tree(repo, commit, crate::library::Limits::default().max_entries) else { return };
    let mut first: BTreeMap<String, (usize, Option<i64>)> = BTreeMap::new();
    if let Ok(adds) = git.additions(repo, commit, None, &["*/toa.yaml"]) {
        for (i, a) in adds.iter().enumerate() {
            for p in &a.paths {
                first.entry(p.clone()).or_insert((i, crate::index::epoch(&a.date)));
            }
        }
    }
    for f in lib::objects(&listing) {
        let members: Vec<&Listed> = f.members.iter().map(|&i| &listing[i]).collect();
        let rel = |e: &Listed| e.path[f.path.len() + 1..].to_string();
        let read = |file: &str, max: u64| {
            members.iter().find(|e| e.is_blob() && rel(e) == file).and_then(|e| git.read_blob(repo, &e.id, max).ok())
        };
        let Some(manifest) =
            read(tree::MANIFEST, toa_core::manifest::MAX_BYTES as u64).and_then(|b| manifest::parse(&b).ok())
        else {
            continue;
        };
        let mut named: BTreeSet<String> =
            object::references(&manifest).into_iter().filter(|r| !r.draft).map(|r| r.name).collect();
        let mut lineage: BTreeSet<String> = manifest
            .get("base")
            .and_then(Node::as_seq)
            .unwrap_or(&[])
            .iter()
            .filter_map(Node::as_str)
            .map(str::to_string)
            .collect();
        if let Some(h) = read(tree::HISTORY, 1 << 20).and_then(|b| history::read(&b).ok()) {
            lineage.extend(h.history);
        }
        named.extend(lineage.iter().cloned());
        if let Some(lock) =
            read(tree::LOCK, 16 << 20).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        {
            named.extend(object::lock_references(&lock).into_iter().filter(|r| !r.draft).map(|r| r.name));
        }
        if let Some(b) = read(tree::MEMBERS, toa_core::members::MAX_BYTES as u64) {
            named.extend(toa_core::members::references(&b).into_iter().filter(|r| !r.draft).map(|r| r.name));
        }
        let author = manifest.get("author").and_then(|n| n.as_str()).map(str::to_string);
        let files =
            members.iter().filter(|e| e.is_blob()).map(|e| (rel(e), e.id.clone(), e.size.unwrap_or(0))).collect();
        let (order, date) = first.get(&format!("{}/{}", f.path, tree::MANIFEST)).copied().unzip();
        into.add(Candidate {
            name: f.name.clone(),
            author,
            named,
            lineage,
            files,
            places: vec![Place { library: idx, path: f.path.clone(), order, date: date.flatten() }],
        });
    }
}

/// The libraries of a workspace list that are on this machine (its include lines with `local`
/// sources), with a label and their publication branch. A library is a checkout and a branch
/// (5.1): the same pair written twice is one library, under the first label; two branches of one
/// checkout are two, and their labels differ.
pub fn local_libraries(workspace: &Path) -> Vec<(PathBuf, String, String)> {
    let Ok(w) = crate::fetch::read_workspace(workspace) else { return Vec::new() };
    let base = workspace.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out: Vec<(PathBuf, String, String)> = Vec::new();
    let mut seen: Vec<(PathBuf, String)> = Vec::new();
    for l in &w.lines {
        let Line::Include(i) = l else { continue };
        for s in &i.sources {
            if let Source::Local(p) = s {
                let d = if toa_core::source::is_absolute_local(p) { PathBuf::from(p) } else { base.join(p) };
                let branch = i.branch.clone().unwrap_or_else(|| crate::library::DEFAULT_BRANCH.to_string());
                let real = std::fs::canonicalize(&d).unwrap_or_else(|_| d.clone());
                if seen.iter().any(|(r, b)| *r == real && *b == branch) {
                    continue;
                }
                seen.push((real, branch.clone()));
                let mut label = i.group.clone().unwrap_or_else(|| p.clone());
                if out.iter().any(|(_, l, _)| *l == label) {
                    label = format!("{label} ({branch})");
                }
                while out.iter().any(|(_, l, _)| *l == label) {
                    label.push('\'');
                }
                out.push((d, label, branch));
            }
        }
    }
    out
}

/// Whether a file of an object is compared (8.3): not a licence text, not `history.json` or
/// `lock.json`, and at least [`MIN_SIZE`] bytes.
fn counts(path: &str, size: u64) -> bool {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    let under_licenses = path.split('/').rev().skip(1).any(|d| d == "LICENSES");
    size >= MIN_SIZE
        && !leaf.starts_with("LICENSE")
        && !leaf.starts_with("COPYING")
        && !under_licenses
        && !matches!(path, tree::HISTORY | tree::LOCK)
}

/// An `author` as 8.3 compares it: no white space at the ends, each run inside one space, and
/// letters in lower case.
fn author_key(a: Option<&str>) -> Option<String> {
    a.map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase())
}

/// Whether two objects are related: one in the lineage of the other, or one refers to the other.
fn related(a: &Candidate, b: &Candidate) -> bool {
    a.named.contains(&b.name) || b.named.contains(&a.name)
}

/// The date of the commit that first held it, the earliest over its libraries.
fn first_date(c: &Candidate) -> Option<i64> {
    c.places.iter().filter_map(|p| p.date).min()
}

/// Whether `p` is earlier than `o`, and how that was told.
fn earlier(p: &Candidate, o: &Candidate) -> Option<Basis> {
    let shared = p
        .places
        .iter()
        .filter_map(|x| o.places.iter().find(|y| y.library == x.library).map(|y| (x, y)))
        .min_by_key(|(x, _)| x.library);
    if let Some((x, y)) = shared {
        return match (x.order, y.order) {
            (Some(a), Some(b)) if a < b => Some(Basis::Library(x.library)),
            _ => None,
        };
    }
    match (first_date(p), first_date(o)) {
        (Some(a), Some(b)) if a < b => Some(Basis::Dates),
        _ => None,
    }
}

/// Whether a file held by these objects is a common file (see the module's documentation).
fn common(objects: &[Candidate], holders: &BTreeSet<usize>, keys: &[Option<String>]) -> bool {
    if holders.len() <= COMMON_GROUPS {
        return false;
    }
    if holders.len() >= COMMON_CAP {
        return true;
    }
    if holders.iter().map(|&i| &keys[i]).collect::<BTreeSet<_>>().len() > COMMON_AUTHORS {
        return true;
    }
    let v: Vec<usize> = holders.iter().copied().collect();
    let mut up: Vec<usize> = (0..v.len()).collect();
    fn top(up: &mut [usize], mut x: usize) -> usize {
        while up[x] != x {
            up[x] = up[up[x]];
            x = up[x];
        }
        x
    }
    for a in 0..v.len() {
        for b in a + 1..v.len() {
            let (ra, rb) = (top(&mut up, a), top(&mut up, b));
            if ra != rb && related(&objects[v[a]], &objects[v[b]]) {
                up[ra] = rb;
            }
        }
    }
    (0..v.len()).map(|x| top(&mut up, x)).collect::<BTreeSet<_>>().len() > COMMON_GROUPS
}

/// What 8.3 says of each object found (by its index in [`Found::objects`]); objects of which it
/// says nothing are left out.
pub fn compute(found: &Found) -> BTreeMap<usize, Same> {
    let objects = &found.objects;
    let keys: Vec<Option<String>> = objects.iter().map(|o| author_key(o.author.as_deref())).collect();
    // blob -> the objects holding it, with the path there
    let mut by_blob: BTreeMap<&str, Vec<(usize, &str)>> = BTreeMap::new();
    for (i, o) in objects.iter().enumerate() {
        for (p, id, size) in &o.files {
            if counts(p, *size) {
                by_blob.entry(id.as_str()).or_default().push((i, p.as_str()));
            }
        }
    }
    // the files of each object's lineage found here: it declared where those came from
    let inherited: Vec<BTreeSet<&str>> = objects
        .iter()
        .map(|o| {
            o.lineage
                .iter()
                .filter_map(|n| found.by_name.get(n))
                .flat_map(|&j| objects[j].files.iter().map(|f| f.1.as_str()))
                .collect()
        })
        .collect();
    let mut out: BTreeMap<usize, Same> = BTreeMap::new();
    // (O, P) -> P, how it is earlier, and the files
    let mut pairs: BTreeMap<(usize, usize), Pair> = BTreeMap::new();
    for (blob, holders) in &by_blob {
        let names: BTreeSet<usize> = holders.iter().map(|h| h.0).collect();
        if names.len() < 2 {
            continue;
        }
        if common(objects, &names, &keys) {
            for (i, path) in holders {
                out.entry(*i).or_default().common.insert(path.to_string());
            }
            continue;
        }
        for &(oi, opath) in holders {
            if inherited[oi].contains(blob) {
                continue;
            }
            for &(pi, ppath) in holders {
                if pi == oi || keys[pi] == keys[oi] || related(&objects[oi], &objects[pi]) {
                    continue;
                }
                let Some(basis) = earlier(&objects[pi], &objects[oi]) else { continue };
                pairs
                    .entry((oi, pi))
                    .or_insert(Pair { object: pi, basis, files: Vec::new() })
                    .files
                    .push((opath.to_string(), ppath.to_string()));
            }
        }
    }
    for ((oi, pi), mut pair) in pairs {
        pair.files.sort();
        pair.files.dedup();
        let mut back: Vec<(String, String)> = pair.files.iter().map(|(a, b)| (b.clone(), a.clone())).collect();
        back.sort();
        out.entry(pi).or_default().later.push(Pair { object: oi, basis: pair.basis, files: back });
        out.entry(oi).or_default().earlier.push(pair);
    }
    // earliest first: by the date of the commit that first held them, then along first parents
    // of the first library that holds them, then by name
    let when = |p: &Pair| {
        let o = &objects[p.object];
        let first = o.places.iter().min_by_key(|x| x.library).map(|x| (x.library, x.order.unwrap_or(usize::MAX)));
        (first_date(o).unwrap_or(i64::MAX), first, o.name.clone())
    };
    for s in out.values_mut() {
        s.earlier.sort_by_cached_key(when);
        s.later.sort_by_cached_key(when);
    }
    out
}

/// The place to name for an object of a pair: in the library that told the order, else its first.
fn place_of(c: &Candidate, basis: Basis) -> &Place {
    let l = match basis {
        Basis::Library(l) => Some(l),
        Basis::Dates => None,
    };
    c.places.iter().find(|p| Some(p.library) == l).unwrap_or(&c.places[0])
}

/// The hints for the objects of the first library read (`toa check`): on each, the earliest
/// earlier object with the same files.
pub fn hints(found: &Found) -> Vec<Problem> {
    let mut out = Vec::new();
    for (oi, s) in compute(found) {
        let a = &found.objects[oi];
        let (Some(here), Some(first)) = (a.places.iter().find(|p| p.library == 0), s.earlier.first()) else {
            continue;
        };
        let b = &found.objects[first.object];
        let there = place_of(b, first.basis);
        let how = match first.basis {
            Basis::Library(0) => "first held by an earlier commit along the first parents of this library".to_string(),
            Basis::Library(l) => {
                format!("first held by an earlier commit along the first parents of {}", found.libraries[l])
            }
            Basis::Dates => "by the committer dates, which their committers wrote".to_string(),
        };
        let files: Vec<&str> = first.files.iter().map(|(h, _)| h.as_str()).collect();
        let more = match s.earlier.len() - 1 {
            0 => String::new(),
            n => format!(" ({n} more earlier object(s) hold some of its files too)"),
        };
        out.push(Problem::info(
            CODE,
            "8.3",
            here.path.clone(),
            format!(
                "{} file(s) byte for byte as in {} ({} at {}), earlier ({how}) and with another author, which is \
                 neither in base nor in the history of this object: {}{more}. If this object was derived from it, \
                 write it in base; this is a hint, not an error",
                files.len(),
                b.name,
                found.libraries[there.library],
                there.path,
                files.join(", ")
            ),
        ));
    }
    out
}

/// What [`compute`] says of one object, as written into the index (`same_files`,
/// doc/index-format.md).
pub fn to_json(found: &Found, s: &Same) -> Value {
    let pair = |p: &Pair| {
        let o = &found.objects[p.object];
        let mut v = json!({
            "object": o.name,
            "author": o.author,
            "basis": match p.basis { Basis::Library(_) => "library", Basis::Dates => "dates" },
            "files": p.files.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
        });
        if let Basis::Library(l) = p.basis {
            v["library"] = found.libraries[l].clone().into();
        }
        v
    };
    let mut v = json!({
        "earlier": s.earlier.iter().take(LISTED).map(pair).collect::<Vec<_>>(),
        "later": s.later.iter().take(LISTED).map(pair).collect::<Vec<_>>(),
        "common": s.common,
    });
    if s.earlier.len() > LISTED {
        v["earlier_more"] = (s.earlier.len() - LISTED).into();
    }
    if s.later.len() > LISTED {
        v["later_more"] = (s.later.len() - LISTED).into();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(name: &str, library: usize, author: Option<&str>, order: usize, files: &[(&str, &str, u64)]) -> Candidate {
        Candidate {
            name: name.into(),
            author: author.map(str::to_string),
            named: BTreeSet::new(),
            lineage: BTreeSet::new(),
            files: files.iter().map(|(p, b, s)| (p.to_string(), b.to_string(), *s)).collect(),
            places: vec![Place { library, path: format!("x/{name}"), order: Some(order), date: Some(order as i64) }],
        }
    }

    fn found(libraries: usize, objects: Vec<Candidate>) -> Found {
        let mut f = Found::default();
        for i in 0..libraries {
            f.library(if i == 0 { "here" } else { "there" });
        }
        for o in objects {
            f.add(o);
        }
        f
    }

    const A: &str = "ckt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "ckt_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn name(i: usize) -> String {
        format!("ckt_{:040x}", i + 1)
    }

    #[test]
    fn the_hint_and_what_is_left_out() {
        let net = ("n.scs", "1111", 4000);
        let two = |a: Candidate, b: Candidate| found(2, vec![a, b]);
        // B earlier, another author, not declared: one hint on A, naming B
        let r = hints(&two(cand(A, 0, Some("Bob"), 2, &[net]), cand(B, 0, Some("Alice"), 1, &[net])));
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!((r[0].code, r[0].path.as_str()), (CODE, format!("x/{A}").as_str()));
        assert!(r[0].message.contains(B) && r[0].message.contains("n.scs"), "{}", r[0].message);
        assert_eq!(r[0].level, toa_core::problem::Level::Info);
        // the credit dropped counts as another author
        assert_eq!(hints(&two(cand(A, 0, None, 2, &[net]), cand(B, 0, Some("Alice"), 1, &[net]))).len(), 1);
        // the same author; B later; the same commit; declared in base or history
        assert!(hints(&two(cand(A, 0, Some("Alice"), 2, &[net]), cand(B, 0, Some(" Alice "), 1, &[net]))).is_empty());
        // compared without regard to case and to runs of white space
        assert!(hints(&two(cand(A, 0, Some("Ann  LEE"), 2, &[net]), cand(B, 0, Some("ann lee"), 1, &[net]))).is_empty());
        // neither written: no hint
        assert!(hints(&two(cand(A, 0, None, 2, &[net]), cand(B, 0, None, 1, &[net]))).is_empty());
        let later = hints(&two(cand(A, 0, Some("Bob"), 1, &[net]), cand(B, 0, Some("Alice"), 2, &[net])));
        assert_eq!(later.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), [format!("x/{B}")], "only the later one");
        assert!(hints(&two(cand(A, 0, Some("Bob"), 1, &[net]), cand(B, 0, Some("Alice"), 1, &[net]))).is_empty());
        let mut a = cand(A, 0, Some("Bob"), 2, &[net]);
        a.named.insert(B.into());
        assert!(hints(&two(a, cand(B, 0, Some("Alice"), 1, &[net]))).is_empty());
        // small files, licence texts, history.json and lock.json do not count
        let left_out = [
            ("n.scs", "1111", 100),
            ("LICENSE", "2222", 4000),
            ("COPYING.txt", "4444", 4000),
            ("LICENSES/CC0-1.0.txt", "5555", 4000),
            ("history.json", "6666", 4000),
            ("lock.json", "7777", 4000),
        ];
        assert!(hints(&two(cand(A, 0, Some("Bob"), 2, &left_out), cand(B, 0, Some("Alice"), 1, &left_out))).is_empty());
        // a file its own lineage holds too: declared where it came from
        let c = "ckt_cccccccccccccccccccccccccccccccccccccccc";
        let mut a = cand(A, 0, Some("Bob"), 3, &[net]);
        a.lineage.insert(c.into());
        a.named.insert(c.into());
        let r = hints(&found(1, vec![a, cand(B, 0, Some("Alice"), 2, &[net]), cand(c, 0, Some("Bob"), 1, &[net])]));
        assert!(r.iter().all(|p| p.path != format!("x/{A}")), "{r:?}");
        // in another library, by date; only the objects of the library checked get hints
        let r = hints(&two(cand(A, 0, Some("Bob"), 2, &[net]), cand(B, 1, Some("Alice"), 1, &[net])));
        assert!(r.len() == 1 && r[0].message.contains("committer dates"), "{r:?}");
        assert!(hints(&two(cand(A, 1, Some("Bob"), 2, &[net]), cand(B, 1, Some("Alice"), 1, &[net]))).is_empty());
    }

    #[test]
    fn one_library_that_holds_both_decides_the_order() {
        let net = ("n.scs", "1111", 4000);
        // A is first held earlier by date, but B earlier along first parents of a library holding both
        let mut a = cand(A, 0, Some("Bob"), 5, &[net]);
        a.places[0].date = Some(1);
        let mut b = cand(B, 0, Some("Alice"), 2, &[net]);
        b.places[0].date = Some(9);
        let f = found(1, vec![a, b]);
        let s = compute(&f);
        assert_eq!(s[&0].earlier[0].object, 1);
        assert_eq!(s[&0].earlier[0].basis, Basis::Library(0));
        assert_eq!(s[&1].later[0].object, 0);
        assert_eq!(s[&1].later[0].files, [("n.scs".to_string(), "n.scs".to_string())]);
        // an object found in two libraries is one object with two places
        let mut f = found(2, vec![cand(A, 0, Some("Bob"), 5, &[net]), cand(B, 1, Some("Alice"), 1, &[net])]);
        f.add(cand(B, 0, Some("Alice"), 7, &[net]));
        assert_eq!(f.objects.len(), 2);
        assert!(compute(&f).get(&0).is_none_or(|s| s.earlier.is_empty()), "B later in the library holding both");
    }

    #[test]
    fn a_common_file_does_not_count() {
        let inc = ("inc.scs", "3333", 4000);
        // six unrelated holders of six authors: common
        let mut objects = vec![cand(A, 0, Some("Bob"), 9, &[inc])];
        for i in 0..COMMON_AUTHORS {
            objects.push(cand(&name(i), 0, Some(&format!("Author {i}")), i, &[inc]));
        }
        let f = found(1, objects);
        assert!(hints(&f).is_empty());
        assert!(compute(&f)[&0].common.contains("inc.scs"));
        // six unrelated holders of two authors: six groups, common
        let mut objects = vec![cand(A, 0, Some("Bob"), 9, &[inc])];
        for i in 0..COMMON_GROUPS {
            objects.push(cand(&name(i), 0, Some("Alice"), i, &[inc]));
        }
        assert!(hints(&found(1, objects)).is_empty());
        // at most five holders: never common
        let mut objects = vec![cand(A, 0, Some("Bob"), 9, &[inc])];
        for i in 0..COMMON_GROUPS - 1 {
            objects.push(cand(&name(i), 0, Some(&format!("Author {i}")), i, &[inc]));
        }
        assert_eq!(hints(&found(1, objects)).iter().filter(|p| p.path == format!("x/{A}")).count(), 1);
    }

    #[test]
    fn the_generations_of_one_design_are_one_group() {
        let net = ("n.scs", "1111", 4000);
        // ten generations of Alice's design, each with the one before in its lineage, hold the
        // netlist; Bob's object copies it later: one group of Alice and Bob's object, not common
        let mut objects = Vec::new();
        for i in 0..10 {
            let mut c = cand(&name(i), 0, Some("Alice"), i, &[net]);
            for j in 0..i {
                c.lineage.insert(name(j));
                c.named.insert(name(j));
            }
            objects.push(c);
        }
        objects.push(cand(A, 0, Some("Bob"), 20, &[net]));
        let f = found(1, objects);
        let s = compute(&f);
        let a = f.objects.iter().position(|o| o.name == A).unwrap();
        assert!(s[&a].common.is_empty());
        assert_eq!(f.objects[s[&a].earlier[0].object].name, name(0), "the earliest first");
        assert_eq!(s[&a].earlier.len(), 10);
        let r = hints(&f);
        assert_eq!(r.len(), 1, "{r:?}");
        assert!(r[0].message.contains(&name(0)) && r[0].message.contains("9 more"), "{}", r[0].message);
        // the later generations hold it through their lineage: no hint on them
        assert!(r.iter().all(|p| p.path == format!("x/{A}")));
        // two hundred holders: common without grouping them
        let mut objects = Vec::new();
        for i in 0..COMMON_CAP {
            let mut c = cand(&name(i), 0, Some("Alice"), i, &[net]);
            if i > 0 {
                c.lineage.insert(name(i - 1));
                c.named.insert(name(i - 1));
            }
            objects.push(c);
        }
        objects.push(cand(A, 0, Some("Bob"), 500, &[net]));
        assert!(hints(&found(1, objects)).is_empty());
    }

    #[test]
    fn written_into_the_index() {
        let net = ("n.scs", "1111", 4000);
        let f = found(2, vec![cand(A, 0, Some("Bob"), 2, &[net]), cand(B, 1, Some("Alice"), 1, &[net])]);
        let s = compute(&f);
        assert_eq!(
            to_json(&f, &s[&0]),
            json!({"earlier": [{"object": B, "author": "Alice", "basis": "dates", "files": [["n.scs", "n.scs"]]}],
                "later": [], "common": []})
        );
        assert_eq!(to_json(&f, &s[&1])["later"][0]["object"], A);
    }
}
