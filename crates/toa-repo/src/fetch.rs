//! Fetching an object by its name (7.3).
//!
//! Where to look, in order: what is already on the machine (local libraries, the cache), each
//! copy verified from its files before it is used; then the sources the workspace list gives,
//! reading the lists of the libraries it includes; then the addresses of those sources. From a
//! git address the fixed steps of 7.3 are taken: the name first, invalid sources dropped, git
//! called by [`crate::git`], the listing checked before anything is written, the files written
//! into a temporary directory and verified, and only then renamed into place.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toa_core::library::{self, Found};
use toa_core::name;
use toa_core::source::{self, Kind, Line, Source};
use toa_core::{object, tree, Problem};

use crate::git::{Git, GitError};

/// The cache of the tool: `$TOA_CACHE`, or `toa` in the user's cache directory.
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("TOA_CACHE").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let home = |v: &str| std::env::var_os(v).filter(|d| !d.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        home("LOCALAPPDATA").map(|d| d.join("toa"))
    } else if cfg!(target_os = "macos") {
        home("HOME").map(|d| d.join("Library/Caches/toa"))
    } else {
        home("XDG_CACHE_HOME").or_else(|| home("HOME").map(|d| d.join(".cache"))).map(|d| d.join("toa"))
    }
}

/// Where fetched objects are kept as files.
pub fn cache_objects_dir() -> Option<PathBuf> {
    cache_dir().map(|d| d.join("objects"))
}

/// Why fetching failed.
#[derive(Debug)]
pub enum FetchError {
    /// Not a whole object name: nothing was done.
    BadName(String),
    /// No place had it. What was tried, and the hosts the user did not allow.
    NotFound { tried: Vec<String>, denied: Vec<String> },
    /// Writing failed, or git could not run.
    Local(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::BadName(n) => write!(f, "'{n}' is not an object name; nothing was done (7.3)"),
            FetchError::NotFound { tried, denied } => {
                write!(f, "not found")?;
                if !tried.is_empty() {
                    write!(f, "; tried: {}", tried.join("; "))?;
                }
                if !denied.is_empty() {
                    write!(f, "; not allowed to connect to: {} (use --allow-host)", denied.join(", "))?;
                }
                Ok(())
            }
            FetchError::Local(m) => write!(f, "{m}"),
        }
    }
}

/// A fetched object: where its verified files are, and where they came from.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub name: String,
    pub path: PathBuf,
    pub from: String,
}

// ------------------------------------------------------------------ writing an object safely

/// Most entries a listing of a valid object can have: its files, and at most one directory per
/// level of each.
const MAX_OBJECT_ENTRIES: usize = tree::MAX_FILES * (tree::MAX_DEPTH + 2);

fn temp_name(name: &str) -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!(".toa-tmp-{name}-{}-{n}", std::process::id())
}

/// Whether a relative path stays inside the directory it is joined to.
fn stays_inside(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains(['\\', ':'])
        && rel.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

/// Verify the files in `tmp` as the object `name`, then rename `tmp` to `parent/name`. On any
/// failure `tmp` is removed completely.
fn place(tmp: &Path, parent: &Path, name: &str) -> Result<PathBuf, Vec<Problem>> {
    let fail = |ps: Vec<Problem>| {
        let _ = std::fs::remove_dir_all(tmp);
        Err(ps)
    };
    let c = match crate::fs::read_dir(tmp) {
        Ok(c) => c,
        Err(e) => return fail(vec![Problem::error("E_FETCH_WRITE", "7.3", name, e.to_string())]),
    };
    let files: Vec<(&str, &[u8])> = c.files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let dirs: Vec<&str> = c.dirs.iter().map(String::as_str).collect();
    let mut v = object::verify_files(name, &files, &dirs, &[]);
    v.problems.extend(c.problems);
    if toa_core::problem::has_errors(&v.problems) {
        return fail(v.problems);
    }
    let target = parent.join(name);
    if target.exists() {
        // another copy is in place already: keep it when it verifies, else replace it
        match crate::fs::verify_dir(&target) {
            Ok(t) if !toa_core::problem::has_errors(&t.problems) => {
                let _ = std::fs::remove_dir_all(tmp);
                return Ok(target);
            }
            _ => {
                if let Err(e) = std::fs::remove_dir_all(&target) {
                    return fail(vec![Problem::error("E_FETCH_WRITE", "7.3", name, e.to_string())]);
                }
            }
        }
    }
    match std::fs::rename(tmp, &target) {
        Ok(()) => Ok(target),
        Err(e) => fail(vec![Problem::error("E_FETCH_WRITE", "7.3", name, e.to_string())]),
    }
}

/// Write the object `name` held in a git repository as the tree `tree_id` into `parent/name`,
/// by steps 5 and 6 of 7.3: the listing is read (counted, and stopped beyond what a valid
/// object can have) and every path, mode, size and number checked by 3.1 before anything is
/// written; the files are written as raw bytes into a temporary directory and verified.
pub fn write_from_git(
    git: &dyn Git,
    repo: &Path,
    tree_id: &str,
    name: &str,
    parent: &Path,
) -> Result<PathBuf, Vec<Problem>> {
    let p = |code, m: String| vec![Problem::error(code, "7.3", name, m)];
    if !name::is_object_name(name) {
        return Err(p("E_NAME", "not an object name".into()));
    }
    // step 5
    let listing = match git.list_tree(repo, tree_id, MAX_OBJECT_ENTRIES) {
        Ok(l) => l,
        Err(e) => return Err(p("E_FETCH_LISTING", e.to_string())),
    };
    let listing: Vec<library::Listed> = listing
        .into_iter()
        .map(|mut e| {
            e.path = format!("{name}/{}", e.path);
            e
        })
        .collect();
    let found = Found {
        path: name.to_string(),
        name: name.to_string(),
        tree: tree_id.to_string(),
        members: (0..listing.len()).collect(),
    };
    let problems = library::verify_listing(&found, &listing);
    let shape: Vec<Problem> = problems.into_iter().collect();
    if toa_core::problem::has_errors(&shape) {
        return Err(shape);
    }
    // step 6
    if let Err(e) = std::fs::create_dir_all(parent) {
        return Err(p("E_FETCH_WRITE", e.to_string()));
    }
    let tmp = parent.join(temp_name(name));
    let write = || -> Result<(), String> {
        std::fs::create_dir(&tmp).map_err(|e| e.to_string())?;
        for e in &listing {
            let rel = &e.path[name.len() + 1..];
            if !stays_inside(rel) {
                return Err(format!("{rel}: a path that leaves the directory"));
            }
            let target = tmp.join(rel);
            if e.is_tree() {
                std::fs::create_dir_all(&target).map_err(|x| x.to_string())?;
            } else {
                let bytes = git.read_blob(repo, &e.id, tree::MAX_FILE_BYTES).map_err(|x| x.to_string())?;
                if let Some(d) = target.parent() {
                    std::fs::create_dir_all(d).map_err(|x| x.to_string())?;
                }
                std::fs::write(&target, bytes).map_err(|x| x.to_string())?;
            }
        }
        Ok(())
    };
    if let Err(m) = write() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(p("E_FETCH_WRITE", m));
    }
    place(&tmp, parent, name)
}

/// Copy a verified object directory into `parent/name`, verifying the copy.
pub fn copy_dir(from: &Path, name: &str, parent: &Path) -> Result<PathBuf, Vec<Problem>> {
    let p = |m: String| vec![Problem::error("E_FETCH_WRITE", "7.3", name, m)];
    let c = crate::fs::read_dir(from).map_err(|e| p(e.to_string()))?;
    std::fs::create_dir_all(parent).map_err(|e| p(e.to_string()))?;
    let tmp = parent.join(temp_name(name));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir(&tmp)?;
        for d in &c.dirs {
            std::fs::create_dir_all(tmp.join(d))?;
        }
        for (rel, bytes) in &c.files {
            std::fs::write(tmp.join(rel), bytes)?;
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(p(e.to_string()));
    }
    place(&tmp, parent, name)
}

// ------------------------------------------------------------------ the workspace list

/// The workspace list (8.1), read leniently.
#[derive(Debug, Default, Clone)]
pub struct Workspace {
    pub file: PathBuf,
    pub lines: Vec<Line>,
    /// The hosts the user wrote into their own list: connected to without asking (8.1; the
    /// tool's choice, chapter 11).
    pub own_hosts: BTreeSet<String>,
}

/// The libraries of a workspace list that others can reach: include lines with a `git` address
/// and a local checkout. For each, the directory, the address and the publication branch.
pub fn reachable_libraries(w: &Workspace) -> Vec<(PathBuf, String, String)> {
    let base = w.file.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out = Vec::new();
    for l in &w.lines {
        let Line::Include(i) = l else { continue };
        let Some(address) = i.sources.iter().find_map(|s| match s {
            Source::Git { git, .. } => Some(git.clone()),
            _ => None,
        }) else {
            continue;
        };
        for s in &i.sources {
            if let Source::Local(p) = s {
                let d = if source::is_absolute_local(p) { PathBuf::from(p) } else { base.join(p) };
                let branch = i.branch.clone().unwrap_or_else(|| crate::library::DEFAULT_BRANCH.to_string());
                out.push((d, address.clone(), branch));
            }
        }
    }
    out
}

/// Where others find the objects published in the reachable libraries of a workspace list: for
/// each name, a `git` source with the library's address, the earliest commit along first parents
/// of its publication branch that holds it, and its path there (5.1). The first copy in each
/// library counts; libraries that cannot be read are skipped.
pub fn published_places(git: &dyn Git, w: &Workspace) -> BTreeMap<String, Vec<Source>> {
    let mut out: BTreeMap<String, Vec<Source>> = BTreeMap::new();
    for (dir, address, branch) in reachable_libraries(w) {
        let Ok(Some(head)) = crate::library::publication_head(git, &dir, &branch) else { continue };
        let Ok(listing) = git.list_tree(&dir, &head.commit, crate::library::Limits::default().max_entries) else {
            continue;
        };
        let mut first: BTreeMap<String, String> = BTreeMap::new();
        if let Ok(adds) = git.additions(&dir, &head.commit, None, &["*/toa.yaml"]) {
            for a in adds {
                for p in a.paths {
                    first.entry(p.trim_end_matches("/toa.yaml").to_string()).or_insert(a.commit.clone());
                }
            }
        }
        let mut seen = BTreeSet::new();
        for f in library::objects(&listing) {
            if !seen.insert(f.name.clone()) {
                continue;
            }
            let v = out.entry(f.name.clone()).or_default();
            if !v.iter().any(|s| matches!(s, Source::Git { git, .. } if source::same_address(git, &address))) {
                v.push(Source::Git { git: address.clone(), commit: first.get(&f.path).cloned(), path: Some(f.path) });
            }
        }
    }
    out
}

/// Find `workspace.source.jsonl` from `start` upwards.
pub fn find_workspace(start: &Path) -> Option<PathBuf> {
    let start = std::fs::canonicalize(start).ok()?;
    start.ancestors().map(|d| d.join(source::WORKSPACE_LIST)).find(|f| f.is_file())
}

pub fn read_workspace(file: &Path) -> std::io::Result<Workspace> {
    let meta = std::fs::metadata(file)?;
    if meta.len() > source::MAX_VOLUME as u64 {
        return Err(std::io::Error::other("the workspace list is larger than 16 MiB"));
    }
    let bytes = std::fs::read(file)?;
    let r = source::read(&bytes, false, Kind::Other, source::MAX_VOLUME, "");
    let mut own_hosts = BTreeSet::new();
    for l in &r.lines {
        let sources = match l {
            Line::Object(o) => &o.sources,
            Line::Include(i) => &i.sources,
        };
        for s in sources {
            if let Source::Git { git, .. } = s {
                if let Some(h) = source::host_of(git) {
                    own_hosts.insert(h);
                }
            }
        }
    }
    Ok(Workspace { file: file.to_path_buf(), lines: r.lines, own_hosts })
}

// ------------------------------------------------------------------ the fetcher

/// A place where an object may be: a git source with commit and path, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GitPlace {
    address: String,
    commit: Option<String>,
    path: Option<String>,
}

/// Asks the user whether to connect to a host.
pub type Ask<'a> = Box<dyn FnMut(&str) -> bool + 'a>;

/// Fetches objects by name (7.3).
pub struct Fetcher<'a> {
    git: &'a dyn Git,
    /// Where fetched objects are written as files, and the bare repository git fetches into.
    pub cache: PathBuf,
    /// Local libraries and directories to look in first: the repository at hand, the local
    /// sources of the workspace list.
    pub local: Vec<PathBuf>,
    /// The publication branches the workspace list gives for local libraries: what git stores in
    /// them is looked in too, besides the working directory and `HEAD` (one checkout may hold
    /// several libraries, one per branch).
    pub local_branches: Vec<(PathBuf, String)>,
    pub workspace: Option<Workspace>,
    /// Do not connect anywhere.
    pub offline: bool,
    /// Hosts allowed without asking.
    pub allowed: BTreeSet<String>,
    /// Asked before connecting to another host; `None` means not allowed (not interactive).
    pub ask: Option<Ask<'a>>,
    denied: BTreeSet<String>,
    /// Addresses that could not be fetched from, and (address, branch) lists that could not be
    /// fetched, as `address\nbranch`.
    failed: BTreeSet<String>,
    /// Lists of libraries read in this run: (address, branch) -> objects with their places.
    lists: BTreeMap<(String, String), BTreeMap<String, Vec<GitPlace>>>,
    tried: Vec<String>,
}

impl<'a> Fetcher<'a> {
    pub fn new(git: &'a dyn Git, cache: PathBuf) -> Self {
        Fetcher {
            git,
            cache,
            local: Vec::new(),
            local_branches: Vec::new(),
            workspace: None,
            offline: false,
            allowed: BTreeSet::new(),
            ask: None,
            denied: BTreeSet::new(),
            failed: BTreeSet::new(),
            lists: BTreeMap::new(),
            tried: Vec::new(),
        }
    }

    /// Look where a workspace list says: the local sources of its include lines are libraries on
    /// this machine (8.1), looked in first; its object lines and git sources come after.
    pub fn use_workspace(&mut self, w: Workspace) {
        let base = w.file.parent().map(Path::to_path_buf).unwrap_or_default();
        for l in &w.lines {
            if let Line::Include(i) = l {
                for s in &i.sources {
                    if let Source::Local(p) = s {
                        let d = if source::is_absolute_local(p) { PathBuf::from(p) } else { base.join(p) };
                        let b = i.branch.clone().unwrap_or_else(|| crate::library::DEFAULT_BRANCH.into());
                        if !self.local.contains(&d) {
                            self.local.push(d.clone());
                        }
                        self.local_branches.push((d, b));
                    }
                }
            }
        }
        self.workspace = Some(w);
    }

    fn store(&self) -> PathBuf {
        self.cache.join("git")
    }

    /// Fetch one object by name, into `into` (or the cache). The result is always verified from
    /// its files.
    pub fn fetch(&mut self, name: &str, into: Option<&Path>) -> Result<Fetched, FetchError> {
        if !name::is_object_name(name) {
            return Err(FetchError::BadName(name.to_string()));
        }
        self.tried.clear();
        let objects = self.cache.join("objects");
        let parent = into.map(Path::to_path_buf).unwrap_or_else(|| objects.clone());

        // 1. on the machine: local libraries and directories, then the cache
        let mut roots = self.local.clone();
        roots.push(objects.clone());
        for r in &roots {
            if let Some(f) = self.find_local(r, name, &parent) {
                return Ok(f);
            }
        }
        let store = self.store();
        if store.is_dir() {
            // kept by an earlier fetch, or in a library the index fetched: the tree is the name
            let tree_id = name::split(name).map(|(_, h)| h.to_string()).unwrap_or_default();
            if let Ok(Some(_)) = self.git.object_size(&store, &tree_id) {
                if let Ok(p) = write_from_git(self.git, &store, &tree_id, name, &parent) {
                    let _ = self.git.update_ref(&store, &format!("refs/toa/objects/{name}"), &tree_id);
                    return Ok(Fetched { name: name.into(), path: p, from: "the cache".into() });
                }
            }
        }

        // 2. and 3. the workspace list and the sources it leads to
        let places = self.places(name);
        // at most 3 sources are tried for an object (7.3 step 1)
        for place in places.into_iter().take(source::MAX_SOURCES) {
            if let Some(f) = self.fetch_from(&place, name, &parent)? {
                return Ok(f);
            }
        }
        Err(FetchError::NotFound { tried: self.tried.clone(), denied: self.denied.iter().cloned().collect() })
    }

    /// A copy on the machine, verified; copied into `parent` when that is elsewhere.
    fn find_local(&mut self, root: &Path, name: &str, parent: &Path) -> Option<Fetched> {
        let Ok(scan) = crate::seal::scan(root) else { return None };
        for p in scan.objects.iter().filter(|p| p.file_name().and_then(|n| n.to_str()) == Some(name)) {
            match crate::fs::verify_dir(p) {
                Ok(v) if !toa_core::problem::has_errors(&v.problems) => {
                    let path = if p.parent() == Some(parent) { p.clone() } else { copy_dir(p, name, parent).ok()? };
                    return Some(Fetched { name: name.into(), path, from: p.display().to_string() });
                }
                _ => self.tried.push(format!("{} does not verify from its files", p.display())),
            }
        }
        // a local library: what git stores in its head and in the publication branches the
        // workspace list gives for it, when the working directory lacks it
        if let Ok(Some(top)) = self.git.toplevel(root) {
            if crate::fs::under(&top, root).is_some() {
                let mut heads: Vec<String> = self.git.resolve_commit(root, "HEAD").ok().flatten().into_iter().collect();
                for (_, b) in self.local_branches.iter().filter(|(d, _)| d == root) {
                    if let Ok(Some(h)) = crate::library::publication_head(self.git, root, b) {
                        if !heads.contains(&h.commit) {
                            heads.push(h.commit);
                        }
                    }
                }
                for head in heads {
                    if let Ok(list) = self.git.list_tree(root, &head, 2_000_000) {
                        for f in library::objects(&list).into_iter().filter(|f| f.name == name) {
                            if let Ok(p) = write_from_git(self.git, root, &f.tree, name, parent) {
                                return Some(Fetched {
                                    name: name.into(),
                                    path: p,
                                    from: format!("{} (git)", root.display()),
                                });
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// The git places the workspace list gives for a name: its own object lines, then the lists
    /// of the libraries it includes (7.3, 8.1).
    fn places(&mut self, name: &str) -> Vec<GitPlace> {
        let mut out = Vec::new();
        let Some(ws) = self.workspace.clone() else { return out };
        let base = ws.file.parent().map(Path::to_path_buf).unwrap_or_default();
        for l in &ws.lines {
            if let Line::Object(o) = l {
                if o.object != name {
                    continue;
                }
                for s in &o.sources {
                    match s {
                        Source::Git { git, commit, path } => {
                            out.push(GitPlace { address: git.clone(), commit: commit.clone(), path: path.clone() })
                        }
                        Source::Local(p) => {
                            let dir = if source::is_absolute_local(p) { PathBuf::from(p) } else { base.join(p) };
                            if dir.file_name().and_then(|n| n.to_str()) == Some(name) {
                                self.local.push(dir.parent().map(Path::to_path_buf).unwrap_or_default());
                            }
                        }
                    }
                }
            }
        }
        for l in &ws.lines {
            let Line::Include(inc) = l else { continue };
            let branch = inc.branch.clone().unwrap_or_else(|| crate::library::DEFAULT_BRANCH.into());
            for s in inc.sources.iter().take(source::MAX_SOURCES) {
                if let Source::Git { git, .. } = s {
                    if let Some(found) = self.list_of(git, &branch, &inc.include, 0) {
                        if let Some(ps) = found.get(name) {
                            out.extend(ps.iter().cloned());
                        }
                    }
                }
            }
        }
        out.dedup();
        out
    }

    /// Whether connecting to the host of an address is allowed (chapter 11): hosts the user
    /// wrote into their own list or allowed, else asked when there is someone to ask.
    fn may_connect(&mut self, address: &str) -> bool {
        if self.offline {
            return false;
        }
        let Some(host) = source::host_of(address) else { return false };
        if self.allowed.contains(&host) || self.workspace.as_ref().is_some_and(|w| w.own_hosts.contains(&host)) {
            return true;
        }
        if self.denied.contains(&host) {
            return false;
        }
        let yes = self.ask.as_mut().is_some_and(|ask| ask(&host));
        if yes {
            self.allowed.insert(host);
        } else {
            self.denied.insert(host);
        }
        yes
    }

    fn ensure_store(&self) -> Result<PathBuf, FetchError> {
        let store = self.store();
        if !store.join("HEAD").is_file() {
            std::fs::create_dir_all(&store).map_err(|e| FetchError::Local(e.to_string()))?;
            self.git.init_bare(&store).map_err(|e| FetchError::Local(e.to_string()))?;
        }
        Ok(store)
    }

    /// The objects of a library's list fetched from an address: the latest commit of the
    /// branch is fetched, and only the list (and the lists it includes from the same library)
    /// read, its size looked at first (7.3).
    fn list_of(
        &mut self,
        address: &str,
        branch: &str,
        include: &str,
        depth: usize,
    ) -> Option<BTreeMap<String, Vec<GitPlace>>> {
        let key = (address.to_string(), format!("{branch}:{include}"));
        if let Some(l) = self.lists.get(&key) {
            return Some(l.clone());
        }
        // a list that could not be fetched is not tried again; another branch of the same
        // repository is another library, and is (5.1)
        let failed_key = format!("{address}\n{branch}");
        if depth > source::MAX_INCLUDE_DEPTH
            || source::address_problem(address).is_some()
            || self.failed.contains(address)
            || self.failed.contains(&failed_key)
        {
            return None;
        }
        if !self.may_connect(address) {
            return None;
        }
        let store = self.ensure_store().ok()?;
        let refname = format!("refs/toa/lists/{}/{branch}", short_key(address));
        let want = format!("refs/heads/{branch}:{refname}");
        if let Err(e) = self.git.fetch(&store, address, &[&want], true) {
            self.tried.push(format!("the list of {address} ({branch}): {e}"));
            self.failed.insert(failed_key);
            return None;
        }
        let commit = self.git.resolve_commit(&store, &refname).ok()??;
        let mut out: BTreeMap<String, Vec<GitPlace>> = BTreeMap::new();
        self.read_list(&store, address, &commit, include, depth, &mut out, &mut BTreeSet::new());
        self.lists.insert(key, out.clone());
        Some(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn read_list(
        &mut self,
        store: &Path,
        address: &str,
        commit: &str,
        include: &str,
        depth: usize,
        out: &mut BTreeMap<String, Vec<GitPlace>>,
        seen: &mut BTreeSet<String>,
    ) {
        if depth > source::MAX_INCLUDE_DEPTH || !seen.insert(include.to_string()) {
            return;
        }
        let Ok(Some(id)) = self.git.object_at(store, commit, include) else { return };
        let Ok(bytes) = self.git.read_blob(store, &id, source::MAX_VOLUME as u64) else { return };
        let r = source::read(&bytes, false, Kind::Library, source::MAX_VOLUME, include);
        for l in r.lines {
            match l {
                Line::Object(o) => {
                    for s in o.sources.iter().take(source::MAX_SOURCES) {
                        let place = match s {
                            // a path relative to the library is a place in that library (5.1, 5.3)
                            Source::Local(p) if source::names_path_problem(p).is_none() => GitPlace {
                                address: address.to_string(),
                                commit: Some(commit.to_string()),
                                path: Some(p.clone()),
                            },
                            Source::Git { git, commit, path } => {
                                GitPlace { address: git.clone(), commit: commit.clone(), path: path.clone() }
                            }
                            Source::Local(_) => continue,
                        };
                        out.entry(o.object.clone()).or_default().push(place);
                    }
                }
                Line::Include(i) if i.sources.is_empty() => {
                    self.read_list(store, address, commit, &i.include, depth + 1, out, seen)
                }
                Line::Include(i) => {
                    let b = i.branch.clone().unwrap_or_else(|| crate::library::DEFAULT_BRANCH.into());
                    for s in i.sources.iter().take(source::MAX_SOURCES) {
                        if let Source::Git { git, .. } = s {
                            if let Some(more) = self.list_of(git, &b, &i.include, depth + 1) {
                                for (k, v) in more {
                                    out.entry(k).or_default().extend(v);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Fetch from one git place: by name first, then by commit and path (7.3).
    fn fetch_from(&mut self, place: &GitPlace, name: &str, parent: &Path) -> Result<Option<Fetched>, FetchError> {
        let address = &place.address;
        if source::address_problem(address).is_some() {
            self.tried.push(format!("{address}: not a valid address, not handed to git"));
            return Ok(None);
        }
        if self.failed.contains(address) || !self.may_connect(address) {
            return Ok(None);
        }
        let store = self.ensure_store()?;
        let tree_id = name::split(name).map(|(_, h)| h.to_string()).unwrap_or_default();
        let keep = |me: &Self, p: PathBuf, how: &str| {
            let _ = me.git.update_ref(&store, &format!("refs/toa/objects/{name}"), &tree_id);
            Fetched { name: name.into(), path: p, from: format!("{address} ({how})") }
        };
        // by name
        match self.git.fetch(&store, address, &[&tree_id], false) {
            Ok(()) => match write_from_git(self.git, &store, &tree_id, name, parent) {
                Ok(p) => return Ok(Some(keep(self, p, "by name"))),
                Err(ps) => self.tried.push(format!("{address}: {}", ps[0])),
            },
            Err(GitError::Missing(m) | GitError::TooOld(m)) => return Err(FetchError::Local(m)),
            Err(e) => self.tried.push(format!("{address} by name: {e}")),
        }
        // by commit and path
        if let (Some(c), Some(path)) = (&place.commit, &place.path) {
            match self.git.fetch(&store, address, &[c.as_str()], true) {
                Ok(()) => match self.git.object_at(&store, c, path) {
                    Ok(Some(id)) if id == tree_id => match write_from_git(self.git, &store, &tree_id, name, parent) {
                        Ok(p) => return Ok(Some(keep(self, p, "by commit and path"))),
                        Err(ps) => self.tried.push(format!("{address}: {}", ps[0])),
                    },
                    Ok(other) => self.tried.push(format!("{address}: at {path} in {c} is {other:?}, not the object")),
                    Err(e) => self.tried.push(format!("{address}: {e}")),
                },
                Err(e) => {
                    self.tried.push(format!("{address} by commit: {e}"));
                    self.failed.insert(address.clone());
                }
            }
        }
        Ok(None)
    }
}

/// A short key for an address, for names of refs: its host and the first hex digits of its
/// SHA-256.
pub(crate) fn short_key(address: &str) -> String {
    let h = toa_core::hash::digest(toa_core::hash::Algorithm::Sha256, address.trim_end_matches('/').as_bytes())
        .unwrap_or_default();
    let host = source::host_of(address).unwrap_or_else(|| "x".into());
    format!("{host}-{}", &toa_core::hash::hex(&h)[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside() {
        assert!(stays_inside("a/b.txt"));
        for bad in ["", "/a", "a/../b", "..", "a\\b", "c:x", "a//b", "./a"] {
            assert!(!stays_inside(bad), "{bad}");
        }
    }
}
