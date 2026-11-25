//! `toa index`: read the libraries a workspace list includes, and write the index of
//! `doc/index-format.md` (format 1).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use toa_core::hash::{self, Algorithm};
use toa_core::library::{self as lib, Sidecar};
use toa_core::source::{self, Kind, Line, Source};
use toa_core::{history, manifest, name, object, tree, Problem};

use crate::git::{Git, GitError};
use crate::library;

/// The version of the format written (2: `base`, `base_by`, generations and `latest` marks, after
/// the field changes of B32 and B33).
pub const FORMAT: u64 = 2;
/// Bumped whenever `state.json` keeps more of a library, or keeps it otherwise (2: the `label` of
/// each location; 3: no `mentions`, whose names are now references; 4: the `title` of each object;
/// 5: `base`, the generation, the members of `members.source.jsonl`, `latest` marks; 8: the order
/// of commits along first parents, for the same files of 8.3).
const KEPT: u64 = 8;
/// Notes at most this large are written into the index as they are.
pub const MAX_INLINE_NOTE: u64 = 64 * 1024;

/// What to index, and how.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub workspace: PathBuf,
    pub out: PathBuf,
    pub cache: PathBuf,
    pub full: bool,
    pub follow_includes: bool,
    pub offline: bool,
    /// Hosts allowed besides those written in the workspace list.
    pub allowed: BTreeSet<String>,
    /// The tool, for `index.json`.
    pub tool: String,
}

/// What the run did.
#[derive(Debug, Default)]
pub struct Outcome {
    pub libraries: usize,
    pub unreadable: Vec<String>,
    /// Some library could not be fetched (offline, a host not allowed, the network): exit 4.
    pub not_fetched: bool,
    pub objects: usize,
    pub valid: usize,
    pub problems: Vec<Value>,
}

// ------------------------------------------------------------------ what one library holds

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Note {
    file: String,
    commit: Option<String>,
    date: Option<String>,
    size: u64,
    text: Option<String>,
}

/// A file of the timestamp proofs beside a copy (4.3): only its path, kind and size; the proof is
/// not checked, and what the directory holds is not restricted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stamp {
    /// Relative to the `.stamps/` directory, `/` between levels.
    file: String,
    /// A hint guessed from the extension of its name: `opentimestamps` (`.ots`), `rfc3161`
    /// (`.tsr`), else `other`.
    kind: String,
    size: u64,
    /// The commit that added the file, and its committer date.
    commit: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Loc {
    name: String,
    path: String,
    first_commit: Option<String>,
    first_date: Option<String>,
    /// The kind of signature of `first_commit` (`ssh`, `openpgp`, `x509`, `other`), read from its
    /// `gpgsig` header and not verified; `None` when it is not signed (6.3, 8.3).
    #[serde(default)]
    first_signature: Option<String>,
    notes: Vec<Note>,
    #[serde(default)]
    stamps: Vec<Stamp>,
    source_list: Option<String>,
    /// The `label` of the object's line: in the library list or its volumes first, else in the
    /// source list beside it (only shown, never used to find anything; 5.1).
    #[serde(default)]
    label: Option<String>,
    /// Whether the library list (or a volume) marks the object `latest` (5.2).
    #[serde(default)]
    latest: bool,
    problems: Vec<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Removal {
    name: String,
    path: String,
    commit: Option<String>,
    date: Option<String>,
    valid: bool,
    record: Value,
    history: Vec<String>,
    base: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct FileRow {
    path: String,
    size: u64,
    blob: String,
    sha256: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct Reference {
    name: String,
    at: String,
}

/// What reading an object's files gives; fixed by its name in one store.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Analysis {
    sha256: Option<String>,
    manifest: Option<Value>,
    /// The `name` of the manifest, which marks a series (3.4).
    #[serde(default)]
    series: Option<String>,
    /// The title to show: `title`, or for a document the first heading of its `README.md`.
    #[serde(default)]
    title: Option<String>,
    references: Vec<Reference>,
    base: Vec<String>,
    history: Vec<String>,
    /// The generation `history.json` gives; 1 without it.
    #[serde(default)]
    generation: u64,
    files: Vec<FileRow>,
}

/// One library as read, kept in `state.json` for the next run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LibData {
    key: String,
    groups: Vec<String>,
    address: Option<String>,
    branch: String,
    commit: Option<String>,
    date: Option<String>,
    previous: Option<Value>,
    readable: bool,
    library_yaml: Option<Value>,
    includes: Vec<Value>,
    problems: Vec<Value>,
    locations: Vec<Loc>,
    removals: Vec<Removal>,
    analysis: BTreeMap<String, Analysis>,
    /// Path -> (commit, date) of the first commit along first parents that added it.
    first: BTreeMap<String, (String, String)>,
    /// Commit -> its position along first parents among the commits that added a path read (0
    /// the oldest): which of two objects one library held first (8.3).
    #[serde(default)]
    order: BTreeMap<String, usize>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(rename = "toa-index")]
    format: u64,
    /// What this tool keeps of each library; a previous run that kept less is not reused.
    #[serde(default)]
    kept: u64,
    libraries: BTreeMap<String, LibData>,
}

fn pv(p: &Problem) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

fn is_error(v: &Value) -> bool {
    v["level"] == "error"
}

// ------------------------------------------------------------------ which libraries

/// A library to read: from an include line of the workspace list (or of a library list).
#[derive(Debug, Clone)]
struct Spec {
    key: String,
    group: Option<String>,
    address: Option<String>,
    local: Option<PathBuf>,
    branch: String,
    depth: usize,
}

/// A file-name-safe key from an address: its host and path, other characters made `-`.
fn key_of_address(a: &str) -> String {
    let rest = a.split_once("://").map_or(a, |(_, r)| r);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
    let mut k: String = rest
        .trim_end_matches('/')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    k = k.trim_matches('-').to_string();
    if k.is_empty() {
        "library".into()
    } else {
        k.chars().take(64).collect()
    }
}

fn specs_of(lines: &[Line], base: &Path, parent: Option<&Spec>) -> Vec<Spec> {
    let mut out = Vec::new();
    for l in lines {
        let Line::Include(inc) = l else { continue };
        if inc.sources.is_empty() {
            continue;
        }
        let address = inc.sources.iter().find_map(|s| match s {
            Source::Git { git, .. } => Some(git.clone()),
            _ => None,
        });
        // a local checkout only for the user's own list (5.3: in a library's list a local path
        // is a place inside that library)
        let local = if parent.is_none() {
            inc.sources.iter().find_map(|s| match s {
                Source::Local(p) => Some(if source::is_absolute_local(p) { PathBuf::from(p) } else { base.join(p) }),
                _ => None,
            })
        } else {
            None
        };
        if address.is_none() && local.is_none() {
            continue;
        }
        let group = inc.group.clone();
        let own = group.clone().unwrap_or_else(|| key_of_address(address.as_deref().unwrap_or("library")));
        let key = match parent {
            Some(p) => format!("{}/{own}", p.key),
            None => own,
        };
        out.push(Spec {
            key,
            group,
            address,
            local,
            branch: inc.branch.clone().unwrap_or_else(|| library::DEFAULT_BRANCH.into()),
            depth: parent.map_or(0, |p| p.depth + 1),
        });
    }
    out
}

/// Where a library's git store is, and its publication head: a local checkout, or the cache.
enum Opened {
    Ready { repo: PathBuf, commit: String },
    Empty,
    Unreadable { why: String, not_fetched: bool },
}

struct Indexer<'a> {
    git: &'a dyn Git,
    opts: &'a Options,
    own_hosts: BTreeSet<String>,
    out: Outcome,
}

impl Indexer<'_> {
    fn may_connect(&self, address: &str) -> bool {
        !self.opts.offline
            && source::host_of(address).is_some_and(|h| self.opts.allowed.contains(&h) || self.own_hosts.contains(&h))
    }

    fn open(&mut self, spec: &Spec) -> Opened {
        if let Some(dir) = &spec.local {
            if let Ok(Some(top)) = self.git.toplevel(dir) {
                let same = std::fs::canonicalize(&top).ok() == std::fs::canonicalize(dir).ok();
                if same {
                    return match library::publication_head(self.git, &top, &spec.branch) {
                        Ok(Some(h)) => Opened::Ready { repo: top, commit: h.commit },
                        Ok(None) => Opened::Empty,
                        Err(e) => Opened::Unreadable { why: e.to_string(), not_fetched: false },
                    };
                }
            }
        }
        let Some(address) = &spec.address else {
            return Opened::Unreadable { why: "no local checkout and no git address".into(), not_fetched: true };
        };
        if let Some(w) = source::address_problem(address) {
            return Opened::Unreadable { why: format!("{address}: {w}"), not_fetched: false };
        }
        let store = self.opts.cache.join("git");
        let refname = format!("refs/toa/libs/{}/{}", crate::fetch::short_key(address), spec.branch);
        if self.may_connect(address) {
            if !store.join("HEAD").is_file() {
                if let Err(e) =
                    std::fs::create_dir_all(&store).map_err(GitError::Io).and_then(|_| self.git.init_bare(&store))
                {
                    return Opened::Unreadable { why: e.to_string(), not_fetched: false };
                }
            }
            let want = format!("refs/heads/{}:{refname}", spec.branch);
            if let Err(e) = self.git.fetch(&store, address, &[&want], false) {
                // what the cache has from an earlier run is still used
                if let Ok(Some(c)) = self.git.resolve_commit(&store, &refname) {
                    self.out.problems.push(json!({"kind": "diag", "level": "warning", "code": "W_INDEX_STALE",
                        "section": "7.3", "library": spec.key, "path": "", "message": format!("fetching failed ({e}); the commit read last time is used")}));
                    self.out.not_fetched = true;
                    return Opened::Ready { repo: store, commit: c };
                }
                return Opened::Unreadable { why: format!("{address}: {e}"), not_fetched: true };
            }
        } else if !store.join("HEAD").is_file() {
            let why = if self.opts.offline {
                "offline".to_string()
            } else {
                format!("not allowed to connect to {address} (use --allow-host)")
            };
            return Opened::Unreadable { why, not_fetched: true };
        }
        match self.git.resolve_commit(&store, &refname) {
            Ok(Some(c)) => {
                if !self.may_connect(address) {
                    self.out.not_fetched = true;
                }
                Opened::Ready { repo: store, commit: c }
            }
            _ => Opened::Unreadable {
                why: if self.opts.offline {
                    "offline, and not in the cache".into()
                } else {
                    format!("not allowed to connect to {address} (use --allow-host)")
                },
                not_fetched: true,
            },
        }
    }

    /// Read one library, reusing what the previous run read where it may.
    fn read(&mut self, spec: &Spec, prev: Option<&LibData>) -> LibData {
        let mut d = LibData {
            key: spec.key.clone(),
            groups: spec.group.iter().cloned().collect(),
            address: spec.address.clone(),
            branch: spec.branch.clone(),
            ..Default::default()
        };
        let (repo, commit) = match self.open(spec) {
            Opened::Ready { repo, commit } => (repo, commit),
            Opened::Empty => {
                d.readable = true;
                return d;
            }
            Opened::Unreadable { why, not_fetched } => {
                self.out.not_fetched |= not_fetched;
                self.out.unreadable.push(spec.key.clone());
                let mut keep = prev.cloned().unwrap_or(d);
                keep.readable = false;
                keep.problems = vec![
                    json!({"level": "error", "code": "E_INDEX_LIBRARY", "section": "8.3", "path": "", "message": why}),
                ];
                return keep;
            }
        };
        if let Some(p) = prev {
            if p.commit.as_deref() == Some(commit.as_str()) && p.readable && !self.opts.full {
                let mut same = p.clone();
                same.groups = d.groups;
                same.address = d.address;
                return same;
            }
        }
        d.readable = true;
        d.date = self.git.commit_date(&repo, &commit).ok();
        // incremental: the previous commit is an ancestor, or history was rewritten (7.5)
        let mut since = None;
        if let Some(p) = prev.filter(|_| !self.opts.full) {
            if let Some(old) = &p.commit {
                let ancestor = self.git.is_ancestor(&repo, old, &commit).unwrap_or(false);
                d.previous = Some(json!({"commit": old, "ancestor": ancestor}));
                if ancestor {
                    since = Some(old.clone());
                    d.first = p.first.clone();
                    d.order = p.order.clone();
                } else {
                    d.problems.push(json!({"level": "warning", "code": "W_INDEX_REWRITTEN", "section": "7.5", "path": "",
                        "message": format!("the commit read last time, {old}, is not an ancestor of {commit}: history was rewritten; the library was read again whole")}));
                }
            }
        }
        d.commit = Some(commit.clone());
        let patterns = ["*/toa.yaml", "*.notes/*", "*.removed.json", "*.stamps/*"];
        match self.git.additions(&repo, &commit, since.as_deref(), &patterns) {
            Ok(adds) => {
                for a in adds {
                    let next = d.order.len();
                    d.order.entry(a.commit.clone()).or_insert(next);
                    for p in a.paths {
                        d.first.entry(p).or_insert_with(|| (a.commit.clone(), a.date.clone()));
                    }
                }
            }
            Err(e) => {
                d.problems.push(json!({"level": "warning", "code": "W_INDEX_DATES", "section": "8.3", "path": "",
                "message": format!("the dates could not be read: {e}")}))
            }
        }
        let listing = match self.git.list_tree(&repo, &commit, library::Limits::default().max_entries) {
            Ok(l) => l,
            Err(e) => {
                d.readable = false;
                self.out.unreadable.push(spec.key.clone());
                d.problems.push(json!({"level": "error", "code": "E_LIB_LISTING", "section": "6.4", "path": "", "message": e.to_string()}));
                return d;
            }
        };
        let check = match library::check(self.git, &repo, &commit, None, &library::Limits::default()) {
            Ok(r) => r.problems,
            Err(e) => vec![Problem::error("E_INDEX_LIBRARY", "8.3", "", e.to_string())],
        };
        let found = lib::objects(&listing);
        let by_path: BTreeMap<&str, &lib::Listed> = listing.iter().map(|e| (e.path.as_str(), e)).collect();
        let (labels, marked) = self.list_lines(&repo, &by_path);
        let inside =
            |path: &str, o: &str| path == o || (path.starts_with(o) && path.as_bytes().get(o.len()) == Some(&b'/'));

        // problems: per object copy, or of the library
        for p in &check {
            if !found.iter().any(|f| inside(&p.path, &f.path)) {
                d.problems.push(pv(p));
            }
        }
        let reuse: BTreeMap<String, Analysis> = match prev {
            Some(p) if !self.opts.full => p.analysis.clone(),
            _ => BTreeMap::new(),
        };
        let mut signatures: BTreeMap<String, Option<String>> = BTreeMap::new();
        for f in &found {
            let problems: Vec<Value> = check.iter().filter(|p| inside(&p.path, &f.path)).map(pv).collect();
            let first = d.first.get(&format!("{}/{}", f.path, tree::MANIFEST)).cloned();
            let notes_dir = format!("{}{}/", f.path, lib::NOTES_SUFFIX);
            let mut notes = Vec::new();
            for e in listing.iter().filter(|e| e.is_blob() && e.path.starts_with(&notes_dir)) {
                let size = e.size.unwrap_or(0);
                let text = (size <= MAX_INLINE_NOTE)
                    .then(|| self.git.read_blob(&repo, &e.id, MAX_INLINE_NOTE).ok())
                    .flatten()
                    .map(|b| String::from_utf8_lossy(&b).into_owned());
                let added = d.first.get(&e.path).cloned();
                notes.push(Note {
                    file: e.path[notes_dir.len()..].to_string(),
                    commit: added.as_ref().map(|a| a.0.clone()),
                    date: added.map(|a| a.1),
                    size,
                    text,
                });
            }
            let stamps_dir = format!("{}{}/", f.path, lib::STAMPS_SUFFIX);
            let mut stamps: Vec<Stamp> = listing
                .iter()
                .filter(|e| e.is_blob() && e.path.starts_with(&stamps_dir))
                .map(|e| {
                    let file = e.path[stamps_dir.len()..].to_string();
                    let leaf = file.rsplit('/').next().unwrap_or("");
                    let ext = leaf.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default();
                    let kind = match ext.as_str() {
                        "ots" => "opentimestamps",
                        "tsr" => "rfc3161",
                        _ => "other",
                    };
                    let added = d.first.get(&e.path).cloned();
                    Stamp {
                        file,
                        kind: kind.into(),
                        size: e.size.unwrap_or(0),
                        commit: added.as_ref().map(|a| a.0.clone()),
                        date: added.map(|a| a.1),
                    }
                })
                .collect();
            stamps.sort_by(|a, b| a.file.cmp(&b.file));
            let list = format!("{}{}", f.path, source::SUFFIX);
            let label = labels.get(&f.name).cloned().or_else(|| {
                let e = by_path.get(list.as_str()).filter(|e| e.is_blob())?;
                let b = self.git.read_blob(&repo, &e.id, source::MAX_SIDECAR_LIST as u64).ok()?;
                let r = source::read(&b, false, Kind::Other, source::MAX_SIDECAR_LIST, "");
                source::merged_objects(&r.lines).into_iter().find(|o| o.object == f.name)?.label
            });
            let first_signature = first.as_ref().and_then(|(c, _)| {
                signatures
                    .entry(c.clone())
                    .or_insert_with(|| {
                        self.git.commit_signature(&repo, c).ok().flatten().map(|s| s.as_str().to_string())
                    })
                    .clone()
            });
            d.locations.push(Loc {
                first_signature,
                label,
                latest: marked.contains(&f.name),
                name: f.name.clone(),
                path: f.path.clone(),
                first_commit: first.as_ref().map(|a| a.0.clone()),
                first_date: first.map(|a| a.1),
                notes,
                stamps,
                source_list: by_path.get(list.as_str()).filter(|e| e.is_blob()).map(|_| list),
                problems,
            });
            if !d.analysis.contains_key(&f.name) {
                let a = match reuse.get(&f.name) {
                    Some(a) => a.clone(),
                    None => analyse(self.git, &repo, f, &listing),
                };
                d.analysis.insert(f.name.clone(), a);
            }
        }
        // removal records
        for e in &listing {
            let leaf = e.leaf();
            let Some((base, Some(Sidecar::Removal))) = lib::sidecar_of(leaf) else { continue };
            if !name::is_object_name(base)
                || !e.is_regular_file()
                || lib::inside_sidecar_dir(&e.path)
                || found.iter().any(|f| inside(&e.path, &f.path))
            {
                continue;
            }
            let bytes = self.git.read_blob(&repo, &e.id, lib::MAX_REMOVAL).unwrap_or_default();
            let record: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            let valid = !check.iter().any(|p| p.path == e.path && p.is_error());
            let r = library::read_removal(&bytes, base).ok();
            let added = d.first.get(&e.path).cloned();
            d.removals.push(Removal {
                name: base.to_string(),
                path: e.path.clone(),
                commit: added.as_ref().map(|a| a.0.clone()),
                date: added.map(|a| a.1),
                valid: valid && r.is_some(),
                record,
                history: r.as_ref().and_then(|r| r.history.clone()).unwrap_or_default(),
                base: r.and_then(|r| r.base).unwrap_or_default(),
            });
        }
        // library.yaml and the include lines of the library list
        if let Some(e) = by_path.get(lib::LIBRARY_YAML).filter(|e| e.is_blob()) {
            if let Ok(b) = self.git.read_blob(&repo, &e.id, manifest::MAX_BYTES as u64) {
                match manifest::parse(&b) {
                    Ok(n) => d.library_yaml = Some(manifest::to_json(&n)),
                    Err(ps) => d.problems.extend(ps.iter().map(|p| {
                        let mut v = pv(p);
                        v["path"] = format!("{}:{}", lib::LIBRARY_YAML, p.path.trim_start_matches("toa.yaml:")).into();
                        v
                    })),
                }
            }
        }
        if let Some(e) = by_path.get(source::LIBRARY_LIST).filter(|e| e.is_blob()) {
            if let Ok(b) = self.git.read_blob(&repo, &e.id, source::MAX_VOLUME as u64) {
                for l in source::read(&b, false, Kind::Library, source::MAX_VOLUME, "").lines {
                    if matches!(&l, Line::Include(i) if !i.sources.is_empty()) {
                        let mut v: Value = serde_json::from_str(&source::line_text(&l)).unwrap_or(Value::Null);
                        if let Value::Object(m) = &mut v {
                            let mut with = serde_json::Map::new();
                            with.insert("from".into(), source::LIBRARY_LIST.into());
                            with.extend(std::mem::take(m));
                            *m = with;
                        }
                        d.includes.push(v);
                    }
                }
            }
        }
        d
    }
}

impl Indexer<'_> {
    /// What the library list and the volumes it includes (within 8 levels) say of objects: the
    /// labels, the first line of an object winning, and the objects marked `latest` (5.2).
    fn list_lines(
        &self,
        repo: &Path,
        by_path: &BTreeMap<&str, &lib::Listed>,
    ) -> (BTreeMap<String, String>, BTreeSet<String>) {
        let mut labels = BTreeMap::new();
        let mut marked = BTreeSet::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut todo = vec![(source::LIBRARY_LIST.to_string(), 0usize)];
        while let Some((path, depth)) = todo.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            let Some(e) = by_path.get(path.as_str()).filter(|e| e.is_blob()) else { continue };
            let Ok(b) = self.git.read_blob(repo, &e.id, source::MAX_VOLUME as u64) else { continue };
            let lines = source::read(&b, false, Kind::Library, source::MAX_VOLUME, "").lines;
            for o in source::merged_objects(&lines) {
                if o.latest {
                    marked.insert(o.object.clone());
                }
                if let Some(l) = o.label {
                    labels.entry(o.object).or_insert(l);
                }
            }
            if depth < source::MAX_INCLUDE_DEPTH {
                for l in lines.iter().rev() {
                    if let Line::Include(i) = l {
                        if i.sources.is_empty() {
                            todo.push((i.include.clone(), depth + 1));
                        }
                    }
                }
            }
        }
        (labels, marked)
    }
}

/// Read one object's files from git once: hashes, manifest, references, history.
fn analyse(git: &dyn Git, repo: &Path, f: &lib::Found, listing: &[lib::Listed]) -> Analysis {
    let mut a = Analysis::default();
    let strip = f.path.len() + 1;
    let members: Vec<&lib::Listed> = f.members.iter().map(|&i| &listing[i]).collect();
    let shape_ok = lib::verify_listing(f, listing).is_empty();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for e in members.iter().filter(|e| e.is_blob()) {
        let rel = e.path.get(strip..).unwrap_or("").to_string();
        let mut row = FileRow { path: rel.clone(), size: e.size.unwrap_or(0), blob: e.id.clone(), sha256: None };
        if shape_ok {
            if let Ok(b) = git.read_blob(repo, &e.id, tree::MAX_FILE_BYTES) {
                row.sha256 = hash::digest(Algorithm::Sha256, &b).ok().map(|h| hash::hex(&h));
                files.push((rel, b));
            }
        }
        a.files.push(row);
    }
    a.files.sort_by(|x, y| x.path.cmp(&y.path));
    if shape_ok && files.len() == a.files.len() {
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
        a.sha256 = hash::hash_files(Algorithm::Sha256, &refs).ok();
    }
    let get = |p: &str| files.iter().find(|(r, _)| r == p).map(|(_, b)| b.as_slice());
    if let Some(m) = get(tree::MANIFEST).and_then(|b| manifest::parse(b).ok()) {
        let prefix = name::prefix_of(&f.name).unwrap_or("");
        a.title = toa_types::readable::title(prefix, Some(&m), get("README.md"));
        a.manifest = Some(manifest::to_json(&m));
        a.series = m.get("name").and_then(manifest::Node::as_str).map(str::to_string);
        for r in object::references(&m) {
            a.references.push(Reference { name: r.name.clone(), at: format!("toa.yaml:{}", r.at) });
            if r.at.starts_with("base/") {
                a.base.push(r.name);
            }
        }
    }
    if let Some(v) = get(tree::LOCK).and_then(|b| serde_json::from_slice::<Value>(b).ok()) {
        for r in object::lock_references(&v) {
            a.references.push(Reference { name: r.name, at: format!("lock.json:{}", r.at) });
        }
    }
    if let Some(b) = get(tree::MEMBERS) {
        for r in toa_core::members::references(b) {
            a.references.push(Reference { name: r.name, at: format!("{}:{}", tree::MEMBERS, r.at) });
        }
    }
    a.generation = history::FIRST;
    if let Some(h) = get(tree::HISTORY).and_then(|b| history::read(b).ok()) {
        a.history = h.history;
        a.generation = h.generation;
    }
    a
}

// ------------------------------------------------------------------ across libraries

/// Seconds since 1970 of an ISO 8601 date with an offset (`%cI`), for comparing dates.
pub(crate) fn epoch(d: &str) -> Option<i64> {
    let n = |s: &str| s.parse::<i64>().ok();
    let (date, time) = d.split_once('T')?;
    let mut ymd = date.split('-');
    let (y, m, day) = (n(ymd.next()?)?, n(ymd.next()?)?, n(ymd.next()?)?);
    let (clock, offset) = if let Some(c) = time.strip_suffix('Z') {
        (c, 0)
    } else {
        let at = time.rfind(['+', '-'])?;
        let (c, o) = time.split_at(at);
        let sign = if o.starts_with('-') { -1 } else { 1 };
        let (oh, om) = o[1..].split_once(':')?;
        (c, sign * (n(oh)? * 3600 + n(om)? * 60))
    };
    let mut hms = clock.split(':');
    let (h, mi, s) = (n(hms.next()?)?, n(hms.next()?)?, n(hms.next()?.get(..2)?)?);
    // days from civil (Howard Hinnant)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s - offset)
}

struct Linked<'a> {
    history: &'a [String],
    base: &'a [String],
    /// The name and generation of the object, when the object itself is at hand (not only its
    /// removal record).
    series: Option<(Option<&'a str>, u64)>,
}

/// Whether a library read is the one a spec names: the same address and the same publication
/// branch. A library without an address is only ever itself.
fn same_library(l: &LibData, spec: &Spec) -> bool {
    match (&l.address, &spec.address) {
        (Some(a), Some(b)) => l.branch == spec.branch && source::same_address(a, b),
        _ => false,
    }
}

/// A key not taken yet: the one the spec has; else, when the library with that key is another
/// branch of the same repository, the branch added; else the address added; then a number.
fn unique_key(libs: &[LibData], spec: &Spec) -> String {
    let taken = |k: &str| libs.iter().any(|l| l.key == k);
    if !taken(&spec.key) {
        return spec.key.clone();
    }
    let same_repo = libs.iter().any(|l| {
        l.key == spec.key && matches!((&l.address, &spec.address), (Some(a), Some(b)) if source::same_address(a, b))
    });
    let first = if same_repo {
        format!("{}-{}", spec.key, key_of_address(&spec.branch))
    } else {
        format!("{}-{}", spec.key, key_of_address(spec.address.as_deref().unwrap_or("x")))
    };
    if !taken(&first) {
        return first;
    }
    (2..).map(|n| format!("{first}-{n}")).find(|k| !taken(k)).unwrap_or(first)
}

/// Build the index into `opts.out`.
pub fn build(git: &dyn Git, opts: &Options) -> Result<Outcome, String> {
    let ws = crate::fetch::read_workspace(&opts.workspace).map_err(|e| format!("{}: {e}", opts.workspace.display()))?;
    let base = opts.workspace.parent().map(Path::to_path_buf).unwrap_or_default();
    let prev_state: State = std::fs::read(opts.out.join("state.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(|s: &State| s.format == FORMAT && s.kept == KEPT && !opts.full)
        .unwrap_or_default();

    let mut ix = Indexer { git, opts, own_hosts: ws.own_hosts.clone(), out: Outcome::default() };
    let mut queue = specs_of(&ws.lines, &base, None);
    let mut libs: Vec<LibData> = Vec::new();
    let mut i = 0;
    while i < queue.len() {
        let mut spec = queue[i].clone();
        i += 1;
        // one library, one record: the same address and the same publication branch (5.1); two
        // branches of one repository are two libraries
        if let Some(k) = libs.iter().position(|l| same_library(l, &spec)) {
            if let Some(g) = &spec.group {
                if !libs[k].groups.contains(g) {
                    libs[k].groups.push(g.clone());
                }
            }
            continue;
        }
        spec.key = unique_key(&libs, &spec);
        // what the previous run read under this key, when it was the same library
        let prev = prev_state.libraries.get(&spec.key).filter(|p| {
            p.branch == spec.branch
                && match (&p.address, &spec.address) {
                    (Some(a), Some(b)) => source::same_address(a, b),
                    (a, b) => a.is_none() && b.is_none(),
                }
        });
        let d = ix.read(&spec, prev);
        if opts.follow_includes && spec.depth < source::MAX_INCLUDE_DEPTH {
            let lines: Vec<Line> = d
                .includes
                .iter()
                .filter_map(|v| serde_json::to_string(v).ok())
                .flat_map(|t| source::read(format!("{t}\n").as_bytes(), false, Kind::Other, source::MAX_LINE, "").lines)
                .collect();
            queue.extend(specs_of(&lines, &base, Some(&spec)));
        }
        libs.push(d);
    }

    let files = assemble(&libs, &mut ix.out, opts);
    // write into a temporary directory beside the output, then rename into place
    let parent = opts.out.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let tmp = parent.join(format!(".toa-index-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let write = || -> std::io::Result<()> {
        for (rel, v) in &files {
            let p = tmp.join(rel);
            std::fs::create_dir_all(p.parent().unwrap_or(&tmp))?;
            let mut text = match v {
                Value::String(s) => s.clone(),
                other => serde_json::to_string_pretty(other).unwrap_or_default(),
            };
            if !text.ends_with('\n') {
                text.push('\n');
            }
            std::fs::write(&p, text.as_bytes())?;
        }
        let state =
            State { format: FORMAT, kept: KEPT, libraries: libs.iter().map(|l| (l.key.clone(), l.clone())).collect() };
        std::fs::write(tmp.join("state.json"), serde_json::to_string(&state).unwrap_or_default() + "\n")?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("writing the index: {e}"));
    }
    if opts.out.exists() {
        if !opts.out.join("index.json").is_file() {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(format!("{} exists and is not an index; not replaced", opts.out.display()));
        }
        std::fs::remove_dir_all(&opts.out).map_err(|e| format!("replacing {}: {e}", opts.out.display()))?;
    }
    std::fs::rename(&tmp, &opts.out).map_err(|e| format!("renaming the new index into place: {e}"))?;
    ix.out.libraries = libs.len();
    Ok(ix.out)
}

/// Put the libraries together: the files of the index, by path.
fn assemble(libs: &[LibData], out: &mut Outcome, opts: &Options) -> Vec<(String, Value)> {
    // every name the index knows
    let mut present: BTreeMap<&str, Vec<(&LibData, &Loc)>> = BTreeMap::new();
    let mut removals: BTreeMap<&str, Vec<(&LibData, &Removal)>> = BTreeMap::new();
    for l in libs {
        for loc in &l.locations {
            present.entry(loc.name.as_str()).or_default().push((l, loc));
        }
        for r in &l.removals {
            removals.entry(r.name.as_str()).or_default().push((l, r));
        }
    }
    let analysis = |n: &str| present.get(n).and_then(|v| v.first()).and_then(|(l, _)| l.analysis.get(n));
    let valid_at = |locs: &[(&LibData, &Loc)]| locs.iter().any(|(_, loc)| !loc.problems.iter().any(is_error));
    let mut valid: BTreeSet<&str> = present.iter().filter(|(_, v)| valid_at(v)).map(|(k, _)| *k).collect();

    // the cross-check hash per place (8.3)
    let mut alarms = Vec::new();
    let mut sha: BTreeMap<&str, Option<String>> = BTreeMap::new();
    for (n, locs) in &present {
        let values: BTreeSet<String> =
            locs.iter().filter_map(|(l, _)| l.analysis.get(*n).and_then(|a| a.sha256.clone())).collect();
        if values.len() > 1 {
            let places: Vec<Value> = locs
                .iter()
                .map(|(l, loc)| json!({"library": l.key, "path": loc.path, "sha256": l.analysis.get(*n).and_then(|a| a.sha256.clone())}))
                .collect();
            alarms.push(json!({"kind": "sha256-conflict", "name": n, "places": places}));
            sha.insert(n, None);
            valid.remove(n);
        } else {
            sha.insert(n, values.into_iter().next());
        }
    }

    // history, link by link (8.3)
    let link = |n: &str| -> Option<Linked> {
        if valid.contains(n) {
            let a = analysis(n)?;
            return Some(Linked {
                history: &a.history,
                base: &a.base,
                series: Some((a.series.as_deref(), a.generation)),
            });
        }
        let r = removals.get(n)?.iter().find(|(_, r)| r.valid)?.1;
        Some(Linked { history: &r.history, base: &r.base, series: None })
    };
    let mut hist: BTreeMap<&str, Value> = BTreeMap::new();
    let mut verified_of: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut history_invalid: BTreeSet<&str> = BTreeSet::new();
    for n in present.keys() {
        let Some(a) = analysis(n) else { continue };
        let listed = &a.history;
        if a.base.is_empty() {
            hist.insert(
                n,
                json!({"listed": listed, "verified": [], "declared": listed, "nearest": null, "state": "none",
                    "generation": a.generation, "generation_state": "verified"}),
            );
            continue;
        }
        // the generation, against the first item of base when it is at hand (3.4, 8.3)
        let mut generation_state = "declared";
        if let Some((parent_series, parent_generation)) = a.base.first().and_then(|p| link(p)).and_then(|l| l.series) {
            let same = history::same_series(a.series.as_deref(), parent_series);
            let want = if same { parent_generation + 1 } else { history::FIRST };
            generation_state = if want == a.generation { "verified" } else { "invalid" };
        }
        let mut k = 0;
        let mut based: Vec<String> = a.base.clone();
        let mut state = "verified";
        while let Some(parent) = based.first().cloned() {
            if listed.get(k) != Some(&parent) {
                state = "invalid";
                break;
            }
            let Some(l) = link(&parent) else {
                state = if k == 0 { "gap" } else { "partial" };
                break;
            };
            if l.history != &listed[k + 1..] {
                state = "invalid";
                break;
            }
            k += 1;
            based = l.base.to_vec();
        }
        if state == "verified" && k < listed.len() {
            state = "partial";
        }
        let nearest =
            (state == "gap").then(|| listed.iter().find(|h| present.contains_key(h.as_str())).cloned()).flatten();
        if state == "invalid" || generation_state == "invalid" {
            history_invalid.insert(n);
        }
        let verified: Vec<String> = if state == "invalid" { Vec::new() } else { listed[..k].to_vec() };
        let declared: Vec<String> = listed[verified.len()..].to_vec();
        verified_of.insert(n, verified.clone());
        hist.insert(
            n,
            json!({"listed": listed, "verified": verified, "declared": declared, "nearest": nearest, "state": state,
                "generation": a.generation, "generation_state": generation_state}),
        );
    }
    for n in &history_invalid {
        valid.remove(n);
    }

    // reverse lookups, from valid objects only
    let mut referenced_by: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut base_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut newer: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let ifc_of = |n: &str| {
        analysis(n)
            .and_then(|a| a.manifest.as_ref())
            .and_then(|m| m.get("ifc"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let libs_with = |n: &str| -> BTreeSet<String> {
        present.get(n).map(|v| v.iter().map(|(l, _)| l.key.clone()).collect()).unwrap_or_default()
    };
    // the libraries that mark an object latest (5.2)
    let marked_by = |n: &str| -> Vec<String> {
        present
            .get(n)
            .map(|v| v.iter().filter(|(_, loc)| loc.latest).map(|(l, _)| l.key.clone()).collect())
            .unwrap_or_default()
    };
    for n in &valid {
        let Some(a) = analysis(n) else { continue };
        for r in &a.references {
            if !r.at.starts_with("toa.yaml:base/") {
                referenced_by.entry(r.name.clone()).or_default().push(json!({"name": n, "at": r.at}));
            }
        }
        let verified = verified_of.get(n).cloned().unwrap_or_default();
        if let Some(first) = verified.first() {
            base_by.entry(first.clone()).or_default().push(n.to_string());
        }
        // a newer version (7.5): a library marks N latest, the verified part of its history holds
        // P, and N and P are in one series
        let marks = marked_by(n);
        if marks.is_empty() {
            continue;
        }
        for p in &verified {
            let Some(pa) = analysis(p) else { continue };
            let same = match (&a.series, &pa.series) {
                (None, None) => true, // a type without name: the lineage is the series
                (x, y) => x == y,
            };
            if !same || name::prefix_of(p) != name::prefix_of(n) {
                continue;
            }
            let both: Vec<String> = libs_with(n).intersection(&libs_with(p)).cloned().collect();
            let ifc_changed = ifc_of(n) != ifc_of(p);
            newer
                .entry(p.clone())
                .or_default()
                .push(json!({"name": n, "marked_by": marks, "together": both, "ifc_changed": ifc_changed}));
        }
    }

    // the same files without a declared lineage (8.3), over every library of the index
    let mut same_found = crate::same::Found::default();
    for l in libs {
        same_found.library(&l.key);
    }
    for (n, locs) in &present {
        let Some(a) = analysis(n) else { continue };
        let mut named: BTreeSet<String> = a.references.iter().map(|r| r.name.clone()).collect();
        let lineage: BTreeSet<String> = a.base.iter().chain(&a.history).cloned().collect();
        named.extend(lineage.iter().cloned());
        let places = locs
            .iter()
            .map(|(l, loc)| crate::same::Place {
                library: libs.iter().position(|x| x.key == l.key).unwrap_or(0),
                path: loc.path.clone(),
                order: loc.first_commit.as_ref().and_then(|c| l.order.get(c)).copied(),
                date: loc.first_date.as_deref().and_then(epoch),
            })
            .collect();
        same_found.add(crate::same::Candidate {
            name: n.to_string(),
            author: a.manifest.as_ref().and_then(|m| m.get("author")).and_then(Value::as_str).map(str::to_string),
            named,
            lineage,
            files: a.files.iter().map(|f| (f.path.clone(), f.blob.clone(), f.size)).collect(),
            places,
        });
    }
    let same_of = crate::same::compute(&same_found);
    let same_files = |n: &str| -> Value {
        let s = same_found.index_of(n).and_then(|i| same_of.get(&i));
        crate::same::to_json(&same_found, &s.cloned().unwrap_or_default())
    };

    let mut names: BTreeSet<String> = present.keys().map(|s| s.to_string()).collect();
    names.extend(removals.keys().map(|s| s.to_string()));
    names.extend(referenced_by.keys().cloned());
    names.extend(base_by.keys().cloned());
    names.retain(|n| name::is_object_name(n));

    let mut files: Vec<(String, Value)> = Vec::new();
    let mut problems_lines = String::new();
    let mut push_problem = |v: &Value, lib: &str, object: Option<&str>| {
        let mut p = json!({"kind": "diag"});
        if let (Value::Object(m), Value::Object(src)) = (&mut p, v) {
            for (k, x) in src {
                m.insert(k.clone(), x.clone());
            }
            m.insert("library".into(), lib.into());
            if let Some(o) = object {
                m.insert("object".into(), o.into());
            }
        }
        problems_lines.push_str(&p.to_string());
        problems_lines.push('\n');
        out.problems.push(p);
    };
    for l in libs {
        for p in &l.problems {
            push_problem(p, &l.key, None);
        }
    }

    for n in &names {
        let (prefix, h) = name::split(n).unwrap_or(("", ""));
        let locs = present.get(n.as_str()).cloned().unwrap_or_default();
        let a = analysis(n);
        let mut problems: Vec<Value> = Vec::new();
        for (l, loc) in &locs {
            for p in &loc.problems {
                let mut p = p.clone();
                p["library"] = l.key.clone().into();
                push_problem(&p, &l.key, Some(n));
                problems.push(p);
            }
        }
        if history_invalid.contains(n.as_str()) {
            let p = json!({"level": "error", "code": "E_INDEX_HISTORY", "section": "8.3", "path": "history.json",
                "message": "history.json disagrees with the chain of base that can be found"});
            push_problem(&p, "", Some(n));
            problems.push(p);
        }
        let mut locations: Vec<Value> = Vec::new();
        let mut first: Option<(i64, Value)> = None;
        for (l, loc) in &locs {
            if let (Some(c), Some(d)) = (&loc.first_commit, &loc.first_date) {
                let e = epoch(d).unwrap_or(i64::MAX);
                if first.as_ref().is_none_or(|(x, _)| e < *x) {
                    first =
                        Some((e, json!({"library": l.key, "commit": c, "date": d, "signature": loc.first_signature})));
                }
            }
            locations.push(json!({"library": l.key, "path": loc.path, "first_commit": loc.first_commit, "first_date": loc.first_date,
                "first_signature": loc.first_signature, "notes": loc.notes, "stamps": loc.stamps, "source_list": loc.source_list, "label": loc.label, "latest": loc.latest}));
        }
        let rems: Vec<Value> = removals
            .get(n.as_str())
            .map(|v| v.iter().map(|(l, r)| json!({"library": l.key, "path": r.path, "commit": r.commit, "date": r.date, "valid": r.valid, "record": r.record})).collect())
            .unwrap_or_default();
        let is_present = !locs.is_empty();
        let mut o = serde_json::Map::new();
        o.insert("toa-index".into(), FORMAT.into());
        o.insert("name".into(), n.clone().into());
        o.insert("prefix".into(), prefix.into());
        o.insert("present".into(), is_present.into());
        if is_present {
            o.insert("valid".into(), valid.contains(n.as_str()).into());
            o.insert("problems".into(), problems.into());
            o.insert("sha256".into(), sha.get(n.as_str()).cloned().flatten().into());
            o.insert("manifest".into(), a.and_then(|a| a.manifest.clone()).unwrap_or(Value::Null));
            if let Some(t) = a.and_then(|a| a.title.clone()) {
                o.insert("title".into(), t.into());
            }
            o.insert(
                "references".into(),
                serde_json::to_value(a.map(|a| a.references.clone()).unwrap_or_default()).unwrap_or_default(),
            );
            o.insert("base".into(), a.map(|a| a.base.clone()).unwrap_or_default().into());
            o.insert("history".into(), hist.get(n.as_str()).cloned().unwrap_or(Value::Null));
            o.insert(
                "files".into(),
                serde_json::to_value(a.map(|a| a.files.clone()).unwrap_or_default()).unwrap_or_default(),
            );
            o.insert("locations".into(), locations.into());
            o.insert("first".into(), first.map(|f| f.1).unwrap_or(Value::Null));
            o.insert("same_files".into(), same_files(n));
        } else {
            // only removal records, or only references: the history the records give
            let h =
                removals.get(n.as_str()).and_then(|v| v.iter().find(|(_, r)| r.valid)).map(|(_, r)| r.history.clone());
            o.insert("history".into(), h.map(|h| json!({"listed": h})).unwrap_or(Value::Null));
        }
        o.insert("removals".into(), rems.into());
        o.insert("referenced_by".into(), referenced_by.get(n).cloned().unwrap_or_default().into());
        o.insert("base_by".into(), base_by.get(n).cloned().unwrap_or_default().into());
        o.insert("newer".into(), newer.get(n).cloned().unwrap_or_default().into());
        files.push((format!("objects/{}/{n}.json", &h[..2]), Value::Object(o)));
    }

    for l in libs {
        let objects: BTreeSet<&str> = l.locations.iter().map(|x| x.name.as_str()).collect();
        let removed: BTreeSet<&str> =
            l.removals.iter().map(|r| r.name.as_str()).filter(|r| !objects.contains(r)).collect();
        let v = json!({
            "key": l.key, "groups": l.groups, "address": l.address, "branch": l.branch, "commit": l.commit, "date": l.date,
            "previous": l.previous, "readable": l.readable, "library_yaml": l.library_yaml, "includes": l.includes,
            "objects": objects, "removed": removed, "problems": l.problems,
        });
        files.push((format!("libraries/{}.json", l.key), v));
    }

    out.objects = present.len();
    out.valid = valid.len();
    let built = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let root = json!({
        "toa-index": FORMAT,
        "tool": opts.tool,
        "standard": "0.1",
        "built": utc(built as i64),
        "libraries": libs.iter().map(|l| json!({"key": l.key, "address": l.address, "branch": l.branch, "commit": l.commit,
            "date": l.date, "objects": l.locations.iter().map(|x| x.name.as_str()).collect::<BTreeSet<_>>().len(), "readable": l.readable})).collect::<Vec<_>>(),
        "counts": {"objects": present.len(), "valid": valid.len(), "names": names.len()},
        "alarms": alarms,
        "same_files": {"min_size": crate::same::MIN_SIZE, "common_groups": crate::same::COMMON_GROUPS,
            "common_authors": crate::same::COMMON_AUTHORS, "common_cap": crate::same::COMMON_CAP, "listed": crate::same::LISTED},
    });
    files.push(("index.json".into(), root));
    files.push(("problems.jsonl".into(), Value::String(problems_lines)));
    files
}

/// A Unix time as `YYYY-MM-DDTHH:MM:SSZ`.
fn utc(t: i64) -> String {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2026-11-16T10:00:00+08:00"), epoch("2026-11-16T02:00:00Z"));
        assert_eq!(utc(epoch("2026-11-23T12:34:56Z").unwrap()), "2026-11-23T12:34:56Z");
    }

    #[test]
    fn keys() {
        assert_eq!(key_of_address("https://github.com/A/b.git"), "github-com-a-b-git");
        assert_eq!(key_of_address("ssh://git@host:22/x/"), "host-22-x");
    }
}
