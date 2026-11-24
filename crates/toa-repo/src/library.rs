//! A library in git: the publication branch and "published" (6.3), and the library check (6.4).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toa_core::history::History;
use toa_core::library::{self, Found, Listed, Sidecar};
use toa_core::manifest::{self, Node};
use toa_core::name::{self, Form};
use toa_core::object;
use toa_core::source::{self, Kind, Line};
use toa_core::tree;
use toa_core::{history, members, Problem};

use crate::git::{Git, GitError};

/// The default publication branch (6.3).
pub const DEFAULT_BRANCH: &str = "main";

/// Where the publication branch of a checkout is (6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The ref that decided: `refs/remotes/origin/<branch>` or `refs/heads/<branch>`.
    pub from: String,
    pub commit: String,
}

/// Why the publication branch could not be found.
#[derive(Debug)]
pub enum HeadError {
    /// The repository has commits, but not this branch: an error, never "nothing published".
    Missing(String),
    Git(GitError),
}

impl std::fmt::Display for HeadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeadError::Missing(b) => write!(
                f,
                "the repository has commits but no branch '{b}' (6.3); name the publication branch with --branch"
            ),
            HeadError::Git(e) => write!(f, "{e}"),
        }
    }
}

impl From<GitError> for HeadError {
    fn from(e: GitError) -> Self {
        HeadError::Git(e)
    }
}

/// The head of the publication branch in a checkout (6.3): the remote-tracking ref when there is
/// one, otherwise the local branch. `None` for a repository without commits.
pub fn publication_head(git: &dyn Git, repo: &Path, branch: &str) -> Result<Option<Head>, HeadError> {
    if let Some(w) = source::branch_problem(branch) {
        return Err(HeadError::Git(GitError::BadArgument(format!("branch '{branch}': {w}"))));
    }
    for r in [format!("refs/remotes/origin/{branch}"), format!("refs/heads/{branch}")] {
        if let Some(commit) = git.resolve_commit(repo, &r)? {
            return Ok(Some(Head { from: r, commit }));
        }
    }
    if git.has_commits(repo)? {
        return Err(HeadError::Missing(branch.to_string()));
    }
    Ok(None)
}

/// The object names whose directories are in a commit, with their paths (6.3: "published" means
/// exactly this, for the head of the publication branch).
pub fn objects_in(
    git: &dyn Git,
    repo: &Path,
    commit: &str,
    max_entries: usize,
) -> Result<BTreeMap<String, Vec<String>>, GitError> {
    let listing = git.list_tree(repo, commit, max_entries)?;
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in library::objects(&listing) {
        out.entry(f.name).or_default().push(f.path);
    }
    Ok(out)
}

/// Limits the deployment sets for the library check.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Entries of a listing.
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_entries: 2_000_000 }
    }
}

/// What the library check found.
#[derive(Debug, Default)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Item 9: the references to objects not in this library, each with the paths that refer.
    pub outside: BTreeMap<String, Vec<String>>,
    /// Object directories checked.
    pub objects: usize,
}

fn err(code: &'static str, section: &'static str, path: impl Into<String>, msg: impl Into<String>) -> Problem {
    Problem::error(code, section, path, msg)
}

/// What one object's files say, read once per distinct object.
#[derive(Debug, Clone, Default)]
struct Files {
    manifest: Option<Node>,
    history: Option<History>,
    lock: Option<serde_json::Value>,
}

impl Files {
    fn name(&self) -> Option<&str> {
        self.manifest.as_ref().and_then(|m| m.get("name")).and_then(Node::as_str)
    }
    /// The ancestors along the first item of `base`.
    fn ancestors(&self) -> &[String] {
        self.history.as_ref().map(|h| h.history.as_slice()).unwrap_or(&[])
    }
}

struct Checker<'a> {
    git: &'a dyn Git,
    repo: &'a Path,
    listing: Vec<Listed>,
    by_path: BTreeMap<String, usize>,
    found: Vec<Found>,
    names: BTreeSet<String>,
    report: Report,
}

impl Checker<'_> {
    fn blob(&mut self, e: &Listed, max: u64, code: &'static str, section: &'static str) -> Option<Vec<u8>> {
        match e.size {
            Some(n) if n > max => {
                self.report.problems.push(err(code, section, &e.path, format!("{n} bytes; at most {max}")));
                return None;
            }
            None => {
                self.report.problems.push(err(code, section, &e.path, "not a file"));
                return None;
            }
            _ => {}
        }
        match self.git.read_blob(self.repo, &e.id, max) {
            Ok(b) => Some(b),
            Err(x) => {
                self.report.problems.push(err("E_LIB_READ", section, &e.path, x.to_string()));
                None
            }
        }
    }

    fn get(&self, path: &str) -> Option<&Listed> {
        self.by_path.get(path).map(|&i| &self.listing[i])
    }
}

/// The library check (6.4) of commit `new` against `base`, the current head of the publication
/// branch (`None` for the first commit of a library). An error in one object or file is
/// reported for it alone; the check goes on. A [`GitError`] means the check could not run.
pub fn check(git: &dyn Git, repo: &Path, new: &str, base: Option<&str>, limits: &Limits) -> Result<Report, GitError> {
    let mut report = Report::default();

    // item 1: the repository
    let format = git.object_format(repo)?;
    if format != "sha1" {
        report.problems.push(err("E_LIB_FORMAT", "6.1", "", format!("the repository uses {format}, not SHA-1")));
    }
    let Some(new_commit) = git.resolve_commit(repo, new)? else {
        report.problems.push(err("E_LIB_LISTING", "6.4", "", format!("'{new}' is not a commit")));
        return Ok(report);
    };
    if let Some(b) = base {
        match git.resolve_commit(repo, b)? {
            Some(bc) if git.is_ancestor(repo, &bc, &new_commit)? => {}
            Some(_) => report.problems.push(err(
                "E_LIB_HISTORY",
                "6.4",
                "",
                "the new commit is not a descendant of the base: history was rewritten",
            )),
            None => report.problems.push(err("E_LIB_HISTORY", "6.4", "", format!("the base '{b}' is not a commit"))),
        }
    }
    let listing = match git.list_tree(repo, &new_commit, limits.max_entries) {
        Ok(l) => l,
        Err(e @ (GitError::TooLarge(_) | GitError::Unreadable(_) | GitError::Failed { .. })) => {
            report.problems.push(err("E_LIB_LISTING", "6.4", "", format!("the listing cannot be read: {e}")));
            return Ok(report);
        }
        Err(e) => return Err(e),
    };
    let found = library::objects(&listing);
    let names: BTreeSet<String> = found.iter().map(|f| f.name.clone()).collect();
    let by_path = listing.iter().enumerate().map(|(i, e)| (e.path.clone(), i)).collect();
    let mut c = Checker { git, repo, listing, by_path, found, names, report };

    whole_library(&mut c);
    let files = objects(&mut c);
    outside_objects(&mut c);
    let marked = source_lists(&mut c);
    let base_listing = match base {
        Some(b) => match git.resolve_commit(repo, b)? {
            Some(bc) => match git.list_tree(repo, &bc, limits.max_entries) {
                Ok(l) => Some(l),
                Err(e) => {
                    c.report.problems.push(err("E_LIB_LISTING", "6.4", "", format!("the base cannot be read: {e}")));
                    None
                }
            },
            None => None,
        },
        None => None,
    };
    notes(&mut c, base_listing.as_deref());
    removals(&mut c, base_listing.as_deref(), &new_commit);
    lineage(&mut c, &files);
    latest_marks(&mut c, &files, &marked);
    c.report.objects = c.found.len();
    Ok(c.report)
}

/// Item 1 on the listing: modes, `.gitattributes`, `.gitmodules`; item 3 on names.
fn whole_library(c: &mut Checker) {
    let mut attributes = Vec::new();
    for i in 0..c.listing.len() {
        let e = c.listing[i].clone();
        let leaf = e.leaf().to_string();
        let inside = library::inside_object(&e.path, &c.found);
        if !inside {
            let mode_problem = match e.mode.as_str() {
                "100644" | "040000" => None,
                "100755" => Some("an executable file"),
                "120000" => Some("a symbolic link"),
                "160000" => Some("a submodule"),
                _ => Some("an entry that is not a regular file or a directory"),
            };
            if let Some(m) = mode_problem {
                c.report.problems.push(err("E_LIB_MODE", "6.1", &e.path, format!("{m}: only 100644 and 40000")));
            }
        }
        let lower = leaf.to_ascii_lowercase();
        if lower == library::ATTRIBUTES {
            attributes.push(e.clone());
        }
        if lower == library::MODULES {
            c.report.problems.push(err("E_LIB_GITMODULES", "6.1", &e.path, "no .gitmodules anywhere"));
        }
        // item 3: draft names as names, and names that start with one and a dot
        let draft = name::form(&leaf) == Form::Draft
            || library::sidecar_of(&leaf).is_some_and(|(b, _)| name::form(b) == Form::Draft);
        if draft {
            c.report.problems.push(err("E_LIB_DRAFT", "2.4", &e.path, "a draft name on a publication branch"));
        }
        // item 2, last part: an object name is only the name of a directory
        if !e.is_tree() && name::is_object_name(&leaf) {
            c.report.problems.push(err(
                "E_LIB_NOT_DIR",
                "6.4",
                &e.path,
                "an object name is only the name of a directory",
            ));
        }
    }
    let root: Vec<&Listed> = attributes.iter().filter(|e| e.path == library::ATTRIBUTES).collect();
    for e in &attributes {
        if e.path != library::ATTRIBUTES {
            c.report.problems.push(err(
                "E_LIB_ATTRIBUTES",
                "6.1",
                &e.path,
                "only one .gitattributes, at the root, in lower case",
            ));
        }
    }
    match root.first() {
        Some(e) if e.is_regular_file() => {
            let e = (*e).clone();
            if let Some(bytes) = c.blob(&e, 64 * 1024, "E_LIB_ATTRIBUTES", "6.1") {
                for m in library::attributes_problems(&bytes) {
                    c.report.problems.push(err("E_LIB_ATTRIBUTES", "6.1", &e.path, m));
                }
            }
        }
        Some(e) => c.report.problems.push(err("E_LIB_ATTRIBUTES", "6.1", &e.path, "is a regular file")),
        None => c.report.problems.push(err(
            "E_LIB_ATTRIBUTES",
            "6.1",
            library::ATTRIBUTES,
            "a library has a .gitattributes at its root with the line '* -text'",
        )),
    }
}

/// Items 2 and 4 for every object directory; each distinct object's files are read once.
fn objects(c: &mut Checker) -> BTreeMap<String, Files> {
    let mut read: BTreeMap<String, Files> = BTreeMap::new();
    for k in 0..c.found.len() {
        let f = c.found[k].clone();
        if let Some(w) = library::budget_before_object(&f.path) {
            c.report.problems.push(err("E_LIB_PATH_BUDGET", "6.2", &f.path, w));
        }
        let problems = library::verify_listing(&f, &c.listing);
        let valid = problems.is_empty();
        c.report.problems.extend(problems);
        if !valid || read.contains_key(&f.name) {
            continue;
        }
        let files = object_files(c, &f);
        read.insert(f.name.clone(), files);
    }
    read
}

/// Item 4 for one object: manifest, `lock.json`, `history.json`, and the draft names of item 3.
fn object_files(c: &mut Checker, f: &Found) -> Files {
    let mut out = Files::default();
    let prefix = name::prefix_of(&f.name).unwrap_or("").to_string();
    let at = |rel: &str| format!("{}/{rel}", f.path);
    let members: Vec<(String, Listed)> =
        f.members.iter().map(|&i| (c.listing[i].path[f.path.len() + 1..].to_string(), c.listing[i].clone())).collect();
    let paths: Vec<&str> = members.iter().filter(|(_, e)| e.is_blob()).map(|(p, _)| p.as_str()).collect();
    let get = |p: &str| members.iter().find(|(r, _)| r == p).map(|(_, e)| e.clone());

    match get(tree::MANIFEST) {
        Some(e) => {
            if let Some(bytes) = c.blob(&e, manifest::MAX_BYTES as u64, "E_MANIFEST_BYTES", "3.2") {
                match manifest::parse(&bytes) {
                    Ok(root) => {
                        for mut p in object::check_common(&prefix, &root, &paths) {
                            p.path = at(&p.path);
                            c.report.problems.push(p);
                        }
                        for d in toa_core::draft::drafts_in_manifest(&root) {
                            c.report.problems.push(err(
                                "E_LIB_DRAFT",
                                "2.4",
                                at(tree::MANIFEST),
                                format!("the draft name '{d}' on a publication branch"),
                            ));
                        }
                        for r in object::references(&root) {
                            if !r.draft && !c.names.contains(&r.name) {
                                c.report.outside.entry(r.name).or_default().push(format!(
                                    "{}:{}",
                                    at(tree::MANIFEST),
                                    r.at
                                ));
                            }
                        }
                        out.manifest = Some(root);
                    }
                    Err(ps) => {
                        for mut p in ps {
                            p.path = at(&p.path);
                            c.report.problems.push(p);
                        }
                    }
                }
            }
        }
        None => {
            c.report.problems.push(err("E_TREE_NO_MANIFEST", "3.1", at(tree::MANIFEST), "an object has a toa.yaml"))
        }
    }
    if let Some(e) = get(tree::LOCK) {
        if let Some(bytes) = c.blob(&e, 1024 * 1024, "E_LOCK", "3.3") {
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(v) if v.is_object() => {
                    for d in toa_core::draft::drafts_in_json(&v) {
                        c.report.problems.push(err(
                            "E_LIB_DRAFT",
                            "2.4",
                            at(tree::LOCK),
                            format!("the draft name '{d}' on a publication branch"),
                        ));
                    }
                    for r in object::lock_references(&v) {
                        if !r.draft && !c.names.contains(&r.name) {
                            c.report.outside.entry(r.name).or_default().push(format!("{}:{}", at(tree::LOCK), r.at));
                        }
                    }
                    out.lock = Some(v);
                }
                _ => c.report.problems.push(err("E_LOCK", "3.3", at(tree::LOCK), "lock.json is a JSON object")),
            }
        }
    }
    if let Some(e) = get(tree::MEMBERS) {
        if let Some(bytes) = c.blob(&e, members::MAX_BYTES as u64, "E_MEMBERS", "3.3") {
            for d in members::drafts(&bytes) {
                c.report.problems.push(err(
                    "E_LIB_DRAFT",
                    "2.4",
                    at(tree::MEMBERS),
                    format!("the draft name '{d}' on a publication branch"),
                ));
            }
            for mut p in members::read(&bytes).1 {
                p.path = at(&p.path);
                c.report.problems.push(p);
            }
            for r in members::references(&bytes) {
                if !r.draft && !c.names.contains(&r.name) {
                    c.report.outside.entry(r.name).or_default().push(format!("{}:{}", at(tree::MEMBERS), r.at));
                }
            }
        }
    }
    let base: Vec<String> = out
        .manifest
        .as_ref()
        .and_then(|m| m.get("base"))
        .and_then(Node::as_seq)
        .map(|v| v.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    match get(tree::HISTORY) {
        Some(e) => {
            if let Some(bytes) = c.blob(&e, 1024 * 1024, "E_HISTORY", "3.4") {
                match history::read(&bytes) {
                    Ok(h) => {
                        if out.manifest.is_none() {
                            // the manifest was reported; base is unknown
                        } else if base.is_empty() {
                            c.report.problems.push(err(
                                "E_HISTORY",
                                "3.4",
                                at(tree::HISTORY),
                                "an object without base has no history.json",
                            ));
                        } else if h.history[0] != base[0] {
                            c.report.problems.push(err(
                                "E_HISTORY",
                                "3.4",
                                at(tree::HISTORY),
                                "its first item is not the first item of base",
                            ));
                        }
                        out.history = Some(h);
                    }
                    Err(m) => c.report.problems.push(err("E_HISTORY", "3.4", at(tree::HISTORY), m)),
                }
            }
        }
        None if !base.is_empty() && out.manifest.is_some() => {
            c.report.problems.push(err("E_HISTORY", "3.4", at(tree::HISTORY), "an object with base has a history.json"))
        }
        None => {}
    }
    out
}

/// Item 5: names outside objects, the path budget, sidecar names.
fn outside_objects(c: &mut Checker) {
    let mut stamps: Vec<String> = Vec::new();
    for i in 0..c.listing.len() {
        let e = c.listing[i].clone();
        if library::inside_object(&e.path, &c.found) {
            // the path budget of the whole library still counts files inside objects
            if e.path.len() > library::MAX_IN_LIBRARY {
                c.report.problems.push(err(
                    "E_LIB_PATH_BUDGET",
                    "6.2",
                    &e.path,
                    format!("{} characters; a path in a library has at most {}", e.path.len(), library::MAX_IN_LIBRARY),
                ));
            }
            continue;
        }
        let leaf = e.leaf();
        if !e.utf8 {
            c.report.problems.push(err("E_LIB_NAME", "6.2", &e.path, "a name that is not UTF-8"));
        } else if let Some(w) = tree::component_problem(leaf, true) {
            c.report.problems.push(err("E_LIB_NAME", "6.2", &e.path, w));
        }
        if e.path.len() > library::MAX_IN_LIBRARY {
            c.report.problems.push(err(
                "E_LIB_PATH_BUDGET",
                "6.2",
                &e.path,
                format!("{} characters; a path in a library has at most {}", e.path.len(), library::MAX_IN_LIBRARY),
            ));
        }
        if let Some((base, kind)) = library::sidecar_of(leaf) {
            if name::form(base) != Form::Object {
                continue; // a draft's, reported as item 3
            }
            let ok = match kind {
                Some(Sidecar::Notes | Sidecar::Stamps) => e.is_tree(),
                Some(Sidecar::SourceList | Sidecar::Removal) => e.is_regular_file(),
                None => false,
            };
            if !ok {
                c.report.problems.push(err(
                    "E_LIB_SIDECAR",
                    "4",
                    &e.path,
                    "a name that starts with an object name and a dot is a .notes directory, a .source.jsonl file, \
                     a .removed.json file or a .stamps directory",
                ));
            } else if kind == Some(Sidecar::Stamps) {
                stamps.push(e.path.clone());
            }
        }
    }
    // a stamps directory holds only regular files placed directly in it, each at most 64 KiB, named
    // by 3.1 item 3 (4.3); what a proof says is not looked at
    for dir in stamps {
        let prefix = format!("{dir}/");
        for e in c.listing.iter().filter(|e| e.path.starts_with(&prefix)) {
            let rel = &e.path[prefix.len()..];
            if rel.contains('/') {
                continue; // inside a directory of the stamps directory, which is reported itself
            }
            if !e.is_regular_file() || tree::component_problem(rel, false).is_some() {
                c.report.problems.push(err(
                    "E_LIB_STAMPS",
                    "4.3",
                    &e.path,
                    "a stamps directory holds only regular files placed directly in it, named by 3.1",
                ));
            } else if e.size.unwrap_or(u64::MAX) > library::MAX_STAMP {
                c.report.problems.push(err("E_LIB_STAMPS", "4.3", &e.path, "a timestamp proof has at most 64 KiB"));
            }
        }
    }
}

/// The kind and size limit of a source list by its path in a library (5.2).
pub fn list_kind(path: &str) -> (Kind, usize) {
    let leaf = path.rsplit('/').next().unwrap_or("");
    if path == source::LIBRARY_LIST || path.starts_with(&format!("{}/", source::LIBRARY_VOLUMES)) {
        (Kind::Library, source::MAX_VOLUME)
    } else if leaf.strip_suffix(source::SUFFIX).is_some_and(name::is_object_name) {
        (Kind::Sidecar, source::MAX_SIDECAR_LIST)
    } else {
        (Kind::Other, source::MAX_VOLUME)
    }
}

/// Item 6: every source list outside objects. Returns the objects the library list and its volumes
/// mark `latest`, with the list that marks each.
fn source_lists(c: &mut Checker) -> BTreeMap<String, String> {
    let mut marked = BTreeMap::new();
    let lists: Vec<Listed> = c
        .listing
        .iter()
        .filter(|e| e.is_blob() && e.leaf().ends_with(source::SUFFIX) && !library::inside_object(&e.path, &c.found))
        .cloned()
        .collect();
    let mut includes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &lists {
        let (kind, limit) = list_kind(&e.path);
        let Some(bytes) = c.blob(e, limit as u64, "E_SOURCE_LIST", "5.1") else { continue };
        let r = source::read(&bytes, true, kind, limit, &e.path);
        c.report.problems.extend(r.problems);
        for l in &r.lines {
            if let Line::Object(o) = l {
                if o.latest {
                    marked.entry(o.object.clone()).or_insert_with(|| e.path.clone());
                }
            }
            if let Line::Include(inc) = l {
                if inc.sources.is_empty() {
                    // relative to the root of the library (5.1); it must be in this commit
                    if c.get(&inc.include).is_none_or(|x| !x.is_blob()) {
                        c.report.problems.push(err(
                            "E_SOURCE_LIST",
                            "5.1",
                            &e.path,
                            format!("includes '{}', which is not a list in this commit", inc.include),
                        ));
                    }
                    includes.entry(e.path.clone()).or_default().push(inc.include.clone());
                }
            }
        }
    }
    // every volume is reached from the library list within 8 levels
    let mut reached = BTreeSet::new();
    let mut level = vec![source::LIBRARY_LIST.to_string()];
    for _ in 0..=source::MAX_INCLUDE_DEPTH {
        let mut next = Vec::new();
        for l in level {
            if reached.insert(l.clone()) {
                next.extend(includes.get(&l).cloned().unwrap_or_default());
            }
        }
        level = next;
    }
    for e in &lists {
        if e.path.starts_with(&format!("{}/", source::LIBRARY_VOLUMES)) && !reached.contains(&e.path) {
            c.report.problems.push(err(
                "E_SOURCE_LIST",
                "5.2",
                &e.path,
                "a volume that library.source.jsonl does not include within 8 levels",
            ));
        }
    }
    marked
}

/// Item 6, the `latest` marks (5.2): every marked object is in this library and not removed; of
/// two marked objects of one series, neither is in the `history` of the other. Two marked objects
/// that branched from one ancestor are allowed, and reported.
fn latest_marks(c: &mut Checker, files: &BTreeMap<String, Files>, marked: &BTreeMap<String, String>) {
    for (n, list) in marked {
        if !c.names.contains(n) {
            c.report.problems.push(err(
                "E_LATEST",
                "5.2",
                list,
                format!("{n} is marked latest but is not in this library (or only its removal record is)"),
            ));
        }
    }
    let here: Vec<(&String, &Files)> = marked.keys().filter_map(|n| files.get(n).map(|f| (n, f))).collect();
    for (i, (a, fa)) in here.iter().enumerate() {
        for (b, fb) in &here[i + 1..] {
            if name::prefix_of(a) != name::prefix_of(b) {
                continue;
            }
            let related = fa.ancestors().contains(b) || fb.ancestors().contains(a);
            // one series: the same name (for a type without name, none), and linked by base:
            // one in the history of the other, or both from one ancestor (3.4). Objects that
            // only share a name, such as circuits of different interfaces, are not
            let linked = related || fa.ancestors().iter().any(|x| fb.ancestors().contains(x));
            let same = fa.name() == fb.name() && linked;
            if !same {
                continue;
            }
            let list = &marked[*a];
            if related {
                let (older, newer) = if fb.ancestors().contains(a) { (a, b) } else { (b, a) };
                c.report.problems.push(err(
                    "E_LATEST",
                    "5.2",
                    list,
                    format!("{older} and {newer} are both marked latest, and {older} is in the history of {newer}"),
                ));
            } else {
                c.report.problems.push(Problem::warning(
                    "W_LATEST_BRANCHES",
                    "5.2",
                    list,
                    format!("{a} and {b} of one series are both marked latest: they branched from one ancestor"),
                ));
            }
        }
    }
}

/// Item 7: notes directories, and that no note was changed or removed.
fn notes(c: &mut Checker, base: Option<&[Listed]>) {
    // (object name, note file) -> (blob id, path)
    let mut seen: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    let dirs: Vec<Listed> = c
        .listing
        .iter()
        .filter(|e| e.is_tree() && matches!(library::sidecar_of(e.leaf()), Some((_, Some(Sidecar::Notes)))))
        .filter(|e| !library::inside_object(&e.path, &c.found))
        .cloned()
        .collect();
    for d in &dirs {
        let object = library::sidecar_of(d.leaf()).map(|(b, _)| b.to_string()).unwrap_or_default();
        let prefix = format!("{}/", d.path);
        let members: Vec<Listed> = c.listing.iter().filter(|e| e.path.starts_with(&prefix)).cloned().collect();
        for e in members {
            let rel = &e.path[prefix.len()..];
            let ok = !rel.contains('/')
                && e.is_regular_file()
                && rel.ends_with(".md")
                && tree::component_problem(rel, false).is_none();
            if !ok {
                c.report.problems.push(err(
                    "E_LIB_NOTES",
                    "4.1",
                    &e.path,
                    "a notes directory holds only regular .md files placed directly in it, named by 3.1",
                ));
                continue;
            }
            if e.size.unwrap_or(u64::MAX) > library::MAX_NOTE {
                c.report.problems.push(err("E_LIB_NOTES", "4.1", &e.path, "a note has at most 256 KiB"));
            }
            match seen.get(&(object.clone(), rel.to_string())) {
                Some((id, other)) if *id != e.id => c.report.problems.push(err(
                    "E_LIB_NOTES",
                    "4.1",
                    &e.path,
                    format!("differs from the note of the same name at {other}"),
                )),
                Some(_) => {}
                None => {
                    seen.insert((object.clone(), rel.to_string()), (e.id.clone(), e.path.clone()));
                }
            }
        }
    }
    let Some(base) = base else { return };
    for e in base {
        if !e.is_blob() {
            continue;
        }
        let Some((dir, file)) = e.path.rsplit_once('/') else { continue };
        let leaf = dir.rsplit('/').next().unwrap_or("");
        let Some((object, Some(Sidecar::Notes))) = library::sidecar_of(leaf) else { continue };
        if !c.names.contains(object) {
            continue; // the object was removed; its notes may go with it
        }
        match seen.get(&(object.to_string(), file.to_string())) {
            Some((id, _)) if *id == e.id => {}
            Some((_, path)) => c.report.problems.push(err(
                "E_LIB_NOTE_CHANGED",
                "4.1",
                path,
                format!("the note {} was changed; notes are only added", e.path),
            )),
            None => c.report.problems.push(err(
                "E_LIB_NOTE_CHANGED",
                "4.1",
                &e.path,
                "the note was removed, but its object is still here; notes are only added",
            )),
        }
    }
}

/// Item 8: every removal record added or changed since the base is true (4.2).
fn removals(c: &mut Checker, base: Option<&[Listed]>, new_commit: &str) {
    let before: BTreeMap<&str, &str> = base.unwrap_or(&[]).iter().map(|e| (e.path.as_str(), e.id.as_str())).collect();
    let records: Vec<Listed> = c
        .listing
        .iter()
        .filter(
            |e| matches!(library::sidecar_of(e.leaf()), Some((b, Some(Sidecar::Removal))) if name::is_object_name(b)),
        )
        .filter(|e| !library::inside_object(&e.path, &c.found) && e.is_regular_file())
        .filter(|e| before.get(e.path.as_str()) != Some(&e.id.as_str()))
        .cloned()
        .collect();
    for e in records {
        let Some(bytes) = c.blob(&e, library::MAX_REMOVAL, "E_REMOVAL", "4.2") else { continue };
        let object = library::sidecar_of(e.leaf()).map(|(b, _)| b.to_string()).unwrap_or_default();
        for m in removal_problems(c.git, c.repo, &bytes, &object, new_commit) {
            c.report.problems.push(err("E_REMOVAL", "4.2", &e.path, m));
        }
    }
}

/// A removal record's fields, read by 4.2.
#[derive(Debug, Clone, Default)]
pub struct Removal {
    pub object: String,
    pub commit: String,
    pub path: String,
    pub license: Option<String>,
    pub base: Option<Vec<String>>,
    pub history: Option<Vec<String>>,
}

/// Read a removal record by the form of 4.2 (not whether it is true).
pub fn read_removal(bytes: &[u8], object: &str) -> Result<Removal, Vec<String>> {
    let mut out = Vec::new();
    let v: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("not JSON: {e}")]),
    };
    let Some(m) = v.as_object() else { return Err(vec!["a removal record is a JSON object".into()]) };
    for k in m.keys() {
        if !matches!(
            k.as_str(),
            "toa" | "object" | "commit" | "path" | "license" | "label" | "base" | "history" | "reason" | "sources"
        ) {
            out.push(format!("unknown field '{k}'"));
        }
    }
    if m.get("toa").and_then(|x| x.as_str()) != Some("removed/1") {
        out.push("toa is \"removed/1\"".into());
    }
    let s = |k: &str| m.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let names = |k: &str, out: &mut Vec<String>| -> Option<Vec<String>> {
        let x = m.get(k)?;
        let list: Option<Vec<String>> = x.as_array().and_then(|a| {
            a.iter().map(|n| n.as_str().filter(|s| name::is_object_name(s)).map(str::to_string)).collect()
        });
        if list.as_ref().is_none_or(Vec::is_empty) {
            out.push(format!("{k} is a list of object names"));
        }
        list
    };
    let r = Removal {
        object: s("object").unwrap_or_default(),
        commit: s("commit").unwrap_or_default(),
        path: s("path").unwrap_or_default(),
        license: s("license"),
        base: names("base", &mut out),
        history: names("history", &mut out),
    };
    if r.object != object {
        out.push("object is the name in the file name".into());
    }
    if !source::is_commit(&r.commit) {
        out.push("commit is 40 lowercase hexadecimal digits".into());
    }
    if let Some(w) = source::names_path_problem(&r.path) {
        out.push(format!("path: {w}"));
    }
    if m.get("license").is_some_and(|x| !x.is_string()) {
        out.push("license is a string".into());
    }
    if m.get("label").is_some_and(|x| x.as_str().is_none_or(|t| t.chars().count() > source::MAX_TEXT)) {
        out.push("label is a string of at most 256 characters".into());
    }
    if m.get("reason").is_some_and(|x| x.as_str().is_none_or(|t| t.chars().count() > library::MAX_REASON)) {
        out.push("reason is a string of at most 1,000 characters".into());
    }
    if let Some(x) = m.get("sources") {
        match x.as_array() {
            Some(a) if a.len() <= source::MAX_SOURCES => {
                for src in a {
                    match source::read_source(src, false) {
                        Ok(Some(source::Source::Git { .. })) => {}
                        Ok(_) => out.push("sources are git sources only".into()),
                        Err(w) => out.push(format!("sources: {w}")),
                    }
                }
            }
            _ => out.push("sources is a list of at most 3 git sources".into()),
        }
    }
    if out.is_empty() {
        Ok(r)
    } else {
        Err(out)
    }
}

/// Whether a removal record is true (4.2): its commit is in the history of the new commit, at
/// its path is the object the name says, and what it copies is the object's.
fn removal_problems(git: &dyn Git, repo: &Path, bytes: &[u8], object: &str, new_commit: &str) -> Vec<String> {
    let r = match read_removal(bytes, object) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let mut out = Vec::new();
    match git.is_ancestor(repo, &r.commit, new_commit) {
        Ok(true) => {}
        Ok(false) => return vec![format!("commit {} is not in the history of the new commit", r.commit)],
        Err(e) => return vec![format!("commit {}: {e}", r.commit)],
    }
    let want = name::split(object).map(|(_, h)| h).unwrap_or("");
    match git.object_at(repo, &r.commit, &r.path) {
        Ok(Some(id)) if id == want => {}
        Ok(Some(id)) => return vec![format!("at {} in {} is {id}, not the object", r.path, r.commit)],
        Ok(None) => return vec![format!("nothing at {} in {}", r.path, r.commit)],
        Err(e) => return vec![e.to_string()],
    }
    let read = |file: &str, max: u64| -> Option<Vec<u8>> {
        let id = git.object_at(repo, &r.commit, &format!("{}/{file}", r.path)).ok()??;
        git.read_blob(repo, &id, max).ok()
    };
    let manifest = read(tree::MANIFEST, manifest::MAX_BYTES as u64).and_then(|b| manifest::parse(&b).ok());
    let Some(m) = manifest else { return vec!["the object's toa.yaml cannot be read".into()] };
    let license = m.get("license").and_then(Node::as_str).map(str::to_string);
    if r.license.is_some() && r.license != license {
        out.push("license is not the object's".into());
    }
    let base: Option<Vec<String>> =
        m.get("base").and_then(Node::as_seq).map(|v| v.iter().filter_map(|n| n.as_str().map(str::to_string)).collect());
    if r.base != base {
        out.push("base is not the object's".into());
    }
    let hist = read(tree::HISTORY, 1024 * 1024).and_then(|b| history::read(&b).ok()).map(|h| h.history);
    if r.history != hist {
        out.push("history is not the object's".into());
    }
    out
}

/// Item 4, the part across objects: when the first item of `base` is in the library, or only its
/// removal record is, the rest of `history` is its `history`; when it is in the library, the
/// `generation` is computed from it (3.4).
fn lineage(c: &mut Checker, files: &BTreeMap<String, Files>) {
    let records: BTreeMap<String, Listed> = c
        .listing
        .iter()
        .filter_map(|e| match library::sidecar_of(e.leaf()) {
            Some((b, Some(Sidecar::Removal))) if e.is_regular_file() && !library::inside_object(&e.path, &c.found) => {
                Some((b.to_string(), e.clone()))
            }
            _ => None,
        })
        .collect();
    for f in c.found.clone() {
        let Some(own) = files.get(&f.name) else { continue };
        let Some(lineage) = own.history.clone() else { continue };
        let h = &lineage.history;
        let parent = &h[0];
        let rest = &h[1..];
        if let Some(p) = files.get(parent) {
            let want = history::of(parent, p.history.as_ref(), history::same_series(own.name(), p.name())).generation;
            if lineage.generation != want {
                c.report.problems.push(err(
                    "E_HISTORY",
                    "3.4",
                    format!("{}/{}", f.path, tree::HISTORY),
                    format!("generation is {}; from {parent} it is {want}", lineage.generation),
                ));
            }
        }
        let theirs: Option<Vec<String>> = if let Some(p) = files.get(parent) {
            Some(p.ancestors().to_vec())
        } else if let Some(rec) = records.get(parent) {
            c.git
                .read_blob(c.repo, &rec.id, library::MAX_REMOVAL)
                .ok()
                .and_then(|b| read_removal(&b, parent).ok())
                .map(|r| r.history.unwrap_or_default())
        } else {
            None
        };
        if let Some(t) = theirs {
            if t != rest {
                c.report.problems.push(err(
                    "E_HISTORY",
                    "3.4",
                    format!("{}/{}", f.path, tree::HISTORY),
                    format!("after {parent} it does not list that object's history"),
                ));
            }
        }
    }
}
