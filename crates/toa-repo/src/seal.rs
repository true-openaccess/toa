//! Sealing and unsealing (7.1): drafts become objects, and back.
//!
//! Both complete, or change nothing: every file written and every name changed is recorded in a
//! journal, and undone in reverse order when a later step fails.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toa_core::history::History;
use toa_core::manifest::{self, Node};
use toa_core::name::{self, Form};
use toa_core::object;
use toa_core::source::{self, IncludeLine, Kind, Line, ObjectLine, Source};
use toa_core::{draft, history, members, tree, Problem};

use crate::fs::{read_dir, DirContents};
use crate::git::Git;
use crate::library;

/// Why sealing or unsealing stopped. Nothing was changed, or what was changed was restored.
#[derive(Debug)]
pub enum Stop {
    /// Problems with the drafts or objects: nothing was changed.
    Problems(Vec<Problem>),
    /// Reading, writing or renaming failed; what was changed was restored. `restored` is false
    /// when restoring failed too, and the message says what is left.
    Io { message: String, restored: bool },
    /// Git could not run.
    Git(String),
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Problems(p) => write!(f, "{} problem(s); nothing was changed", p.len()),
            Stop::Io { message, restored: true } => write!(f, "{message}; everything was restored"),
            Stop::Io { message, restored: false } => write!(f, "{message}; RESTORING FAILED, see above"),
            Stop::Git(m) => write!(f, "{m}"),
        }
    }
}

fn problem(code: &'static str, path: impl Into<String>, msg: impl Into<String>) -> Problem {
    Problem::error(code, "7.1", path, msg)
}

/// Shown path: `/` between levels.
pub(crate) fn shown(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

// ------------------------------------------------------------------ journal

enum Done {
    Wrote { path: PathBuf, old: Option<Vec<u8>> },
    Renamed { from: PathBuf, to: PathBuf },
}

/// Every change made so far, to be undone when a later step fails.
#[derive(Default)]
pub(crate) struct Journal {
    done: Vec<Done>,
    /// Only compute; change nothing (`--dry-run`).
    pub(crate) dry: bool,
}

impl Journal {
    pub(crate) fn new(dry: bool) -> Self {
        Journal { done: Vec::new(), dry }
    }

    pub(crate) fn write(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if self.dry {
            return Ok(());
        }
        let old = match std::fs::read(path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        std::fs::write(path, bytes)?;
        self.done.push(Done::Wrote { path: path.to_path_buf(), old });
        Ok(())
    }

    pub(crate) fn remove(&mut self, path: &Path) -> std::io::Result<()> {
        if self.dry {
            return Ok(());
        }
        let old = std::fs::read(path)?;
        std::fs::remove_file(path)?;
        self.done.push(Done::Wrote { path: path.to_path_buf(), old: Some(old) });
        Ok(())
    }

    pub(crate) fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        if self.dry {
            return Ok(());
        }
        if std::fs::symlink_metadata(to).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} exists already", shown(to)),
            ));
        }
        std::fs::rename(from, to)
            .map_err(|e| std::io::Error::new(e.kind(), format!("renaming {}: {e}", shown(from))))?;
        self.done.push(Done::Renamed { from: from.to_path_buf(), to: to.to_path_buf() });
        Ok(())
    }

    /// Undo everything, newest first. The messages of what could not be undone.
    pub(crate) fn undo(&mut self) -> Vec<String> {
        let mut failed = Vec::new();
        while let Some(d) = self.done.pop() {
            let r = match &d {
                Done::Wrote { path, old: Some(b) } => std::fs::write(path, b),
                Done::Wrote { path, old: None } => std::fs::remove_file(path),
                Done::Renamed { from, to } => std::fs::rename(to, from),
            };
            if let Err(e) = r {
                let what = match d {
                    Done::Wrote { path, .. } => format!("restore {}", shown(&path)),
                    Done::Renamed { from, to } => format!("rename {} back to {}", shown(&to), shown(&from)),
                };
                failed.push(format!("could not {what}: {e}"));
            }
        }
        failed
    }

    pub(crate) fn fail(&mut self, message: String) -> Stop {
        let failed = self.undo();
        for f in &failed {
            eprintln!("toa: {f}");
        }
        Stop::Io { message, restored: failed.is_empty() }
    }
}

// ------------------------------------------------------------------ finding directories

/// The drafts and object directories under a directory, at any depth (6.2). The insides of
/// drafts, objects and notes directories, and `.git`, are not walked.
#[derive(Debug, Default)]
pub struct Scan {
    pub drafts: Vec<PathBuf>,
    pub objects: Vec<PathBuf>,
}

pub fn scan(root: &Path) -> std::io::Result<Scan> {
    let mut out = Scan::default();
    walk(root, &mut out, 0)?;
    out.drafts.sort();
    out.objects.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Scan, depth: usize) -> std::io::Result<()> {
    if depth > 32 {
        return Ok(());
    }
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        if !ft.is_dir() || ft.is_symlink() {
            continue;
        }
        let Some(n) = e.file_name().to_str().map(str::to_string) else { continue };
        if n == ".git" || n.ends_with(library_notes_suffix()) {
            continue;
        }
        match name::form(&n) {
            Form::Draft => out.drafts.push(e.path()),
            Form::Object => out.objects.push(e.path()),
            Form::Reserved => {}
            Form::Other => walk(&e.path(), out, depth + 1)?,
        }
    }
    Ok(())
}

fn library_notes_suffix() -> &'static str {
    toa_core::library::NOTES_SUFFIX
}

fn leaf(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
}

/// The sidecar files beside a directory that move with it (7.1, step 3.5).
fn sidecars(dir: &Path, name: &str) -> Vec<(PathBuf, String)> {
    let parent = dir.parent().unwrap_or(Path::new("."));
    [toa_core::library::NOTES_SUFFIX, source::SUFFIX]
        .iter()
        .map(|s| (parent.join(format!("{name}{s}")), s.to_string()))
        .filter(|(p, _)| std::fs::symlink_metadata(p).is_ok())
        .collect()
}

// ------------------------------------------------------------------ objects on the machine

/// The files of an object, as read and verified.
#[derive(Debug, Clone)]
struct Known {
    path: Option<PathBuf>,
    manifest: Option<Node>,
    lock: Option<serde_json::Value>,
    /// `members.source.jsonl`, whose objects are references too (3.3).
    members: Option<Vec<u8>>,
    /// `history.json`; `None` for an object without `base`, of generation 1.
    history: Option<History>,
}

/// Where objects on the machine are found by name: object directories under the roots, each
/// verified from its files before it is used (7.3, 7.4).
struct Machine {
    paths: BTreeMap<String, Vec<PathBuf>>,
    known: BTreeMap<String, Option<Known>>,
    /// Where others find objects published in the reachable libraries of the workspace list.
    places: BTreeMap<String, Vec<Source>>,
}

impl Machine {
    fn new(roots: &[PathBuf]) -> Self {
        let mut paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for r in roots {
            if let Ok(s) = scan(r) {
                for o in s.objects {
                    paths.entry(leaf(&o)).or_default().push(o);
                }
            }
        }
        Machine { paths, known: BTreeMap::new(), places: BTreeMap::new() }
    }

    fn add(&mut self, name: &str, k: Known) {
        self.known.insert(name.to_string(), Some(k));
    }

    /// The object, verified from its files; `None` when no copy on the machine verifies.
    fn get(&mut self, name: &str) -> Option<Known> {
        if let Some(k) = self.known.get(name) {
            return k.clone();
        }
        let mut found = None;
        for p in self.paths.get(name).cloned().unwrap_or_default() {
            let Ok(v) = crate::fs::verify_dir(&p) else { continue };
            if toa_core::problem::has_errors(&v.problems) {
                continue;
            }
            let lock = std::fs::read(p.join(tree::LOCK)).ok().and_then(|b| serde_json::from_slice(&b).ok());
            let members = std::fs::read(p.join(tree::MEMBERS)).ok();
            let hist = std::fs::read(p.join(tree::HISTORY)).ok().and_then(|b| history::read(&b).ok());
            found = Some(Known { path: Some(p), manifest: v.manifest, lock, members, history: hist });
            break;
        }
        self.known.insert(name.to_string(), found.clone());
        found
    }
}

/// The objects on the machine, for the checks of the type specifications (`toa-types`).
struct MachineLookup<'a>(std::cell::RefCell<&'a mut Machine>);

impl toa_types::Lookup for MachineLookup<'_> {
    fn manifest(&self, name: &str) -> Option<Node> {
        self.0.borrow_mut().get(name)?.manifest
    }
    fn file_sha256(&self, name: &str, path: &str) -> Option<String> {
        let dir = self.0.borrow_mut().get(name)?.path?;
        let bytes = std::fs::read(dir.join(path)).ok()?;
        toa_core::hash::digest(toa_core::hash::Algorithm::Sha256, &bytes).ok().map(|d| toa_core::hash::hex(&d))
    }
}

/// The references of an object (3.3): manifest, `lock.json` and `members.source.jsonl`,
/// `(name, via base only)`.
pub(crate) fn refs_of(
    manifest: Option<&Node>,
    lock: Option<&serde_json::Value>,
    member_list: Option<&[u8]>,
) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut add = |n: String, based: bool| match out.iter_mut().find(|(x, _)| *x == n) {
        Some(e) => e.1 &= based,
        None => out.push((n, based)),
    };
    if let Some(m) = manifest {
        for r in object::references(m) {
            add(r.name, r.at.starts_with("base/"));
        }
    }
    if let Some(l) = lock {
        for r in object::lock_references(l) {
            add(r.name, false);
        }
    }
    if let Some(b) = member_list {
        for r in members::references(b) {
            add(r.name, false);
        }
    }
    out
}

/// Whether source lists follow an object of this name further (5.2), by its type.
fn followed(n: &str) -> bool {
    name::prefix_of(n).is_some_and(toa_types::followed)
}

/// The objects needed to use an object (5.2): all its references; then, for every object of a
/// type that is followed (interfaces, circuits, testbenches, frozen libraries) reached other than
/// only through `base`, its references except `base`, until none is new.
fn needed(own: &[(String, bool)], machine: &mut Machine) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut follow: Vec<String> = Vec::new();
    for (n, based) in own {
        if !order.contains(n) {
            order.push(n.clone());
        }
        if !based && followed(n) && !follow.contains(n) {
            follow.push(n.clone());
        }
    }
    let mut i = 0;
    while i < follow.len() {
        let n = follow[i].clone();
        i += 1;
        let Some(k) = machine.get(&n) else { continue };
        for (r, based) in refs_of(k.manifest.as_ref(), k.lock.as_ref(), k.members.as_deref()) {
            if based {
                continue;
            }
            if !order.contains(&r) {
                order.push(r.clone());
            }
            if followed(&r) && !follow.contains(&r) {
                follow.push(r);
            }
        }
    }
    order
}

/// `dir` spelled as git spells the root of its working directory, so that the paths found under
/// it have `root` as a prefix (an 8.3 short name, other letter case or a symbolic link would not).
fn spelled_as_git(root: Option<&Path>, dir: &Path) -> PathBuf {
    root.and_then(|r| crate::fs::under(r, dir)).unwrap_or_else(|| dir.to_path_buf())
}

/// A path for a local source: relative to `base` when inside it, `/` between levels.
fn local_path(base: &Path, p: &Path) -> String {
    match p.strip_prefix(base) {
        Ok(r) => shown(r),
        Err(_) => shown(p),
    }
}

/// The source list beside a sealed object (5.2): the object first, without a source, then what
/// it needs with cached fields and the sources known for them. Include lines and the `git`
/// sources of an earlier list are kept; local paths are found again.
fn source_list(
    object: &str,
    manifest: Option<&Node>,
    lock: Option<&serde_json::Value>,
    member_list: Option<&[u8]>,
    old: Option<&[u8]>,
    machine: &mut Machine,
    base: &Path,
) -> String {
    let old_lines =
        old.map(|b| source::read(b, false, Kind::Other, source::MAX_SIDECAR_LIST, "").lines).unwrap_or_default();
    let old_objects = source::merged_objects(&old_lines);
    let cached = |m: Option<&Node>, l: Option<&serde_json::Value>, b: Option<&[u8]>| {
        let refs: Vec<String> = refs_of(m, l, b).into_iter().map(|(n, _)| n).collect();
        let license = m.and_then(|m| m.get("license")).and_then(Node::as_str).map(str::to_string);
        (if refs.is_empty() { None } else { Some(refs) }, license)
    };
    let mut lines = Vec::new();
    let (refs, license) = cached(manifest, lock, member_list);
    let label = old_objects.iter().find(|o| o.object == object).and_then(|o| o.label.clone());
    lines.push(Line::Object(ObjectLine { object: object.to_string(), label, refs, license, ..Default::default() }));
    for n in needed(&refs_of(manifest, lock, member_list), machine) {
        let prev = old_objects.iter().find(|o| o.object == n);
        let mut sources: Vec<Source> = prev
            .map(|o| o.sources.iter().filter(|s| matches!(s, Source::Git { .. })).cloned().collect())
            .unwrap_or_default();
        // where others find it: the libraries of the workspace list with an address
        for s in machine.places.get(&n).cloned().unwrap_or_default() {
            let Source::Git { git: a, .. } = &s else { continue };
            if !sources.iter().any(|x| matches!(x, Source::Git { git, .. } if source::same_address(git, a))) {
                sources.push(s);
            }
        }
        let k = machine.get(&n);
        if let Some(p) = k.as_ref().and_then(|k| k.path.as_ref()) {
            sources.push(Source::Local(local_path(base, p)));
        }
        sources.truncate(source::MAX_SOURCES);
        let (refs, license) = match &k {
            Some(k) => cached(k.manifest.as_ref(), k.lock.as_ref(), k.members.as_deref()),
            None => (prev.and_then(|o| o.refs.clone()), prev.and_then(|o| o.license.clone())),
        };
        lines.push(Line::Object(ObjectLine {
            object: n,
            label: prev.and_then(|o| o.label.clone()),
            refs,
            license,
            sources,
            ..Default::default()
        }));
    }
    for l in &old_lines {
        if let Line::Include(i) = l {
            lines.push(Line::Include(IncludeLine { ..i.clone() }));
        }
    }
    source::write(&lines)
}

// ------------------------------------------------------------------ sealing

/// What sealing did, or would do with `dry_run`.
#[derive(Debug, Default)]
pub struct Sealed {
    /// Draft name, object name, the new path.
    pub objects: Vec<(String, String, PathBuf)>,
    /// Things the user must be told, such as the second computation not being made.
    pub notices: Vec<String>,
}

pub struct SealOptions {
    pub dry_run: bool,
    /// Other directories where objects named by `base` may be found (the cache of fetched
    /// objects, for example).
    pub lookup: Vec<PathBuf>,
    /// The workspace list; found from the directory upwards when not given.
    pub workspace: Option<PathBuf>,
}

struct Draft {
    path: PathBuf,
    name: String,
    contents: DirContents,
    manifest_bytes: Vec<u8>,
    manifest: Node,
    lock_bytes: Option<Vec<u8>>,
    members_bytes: Option<Vec<u8>>,
    deps: Vec<String>,
}

/// Seal every draft under `dir` (7.1).
pub fn seal(git: &dyn Git, dir: &Path, opts: &SealOptions) -> Result<Sealed, Stop> {
    let root = git.toplevel(dir).map_err(|e| Stop::Git(e.to_string()))?;
    let dir = &spelled_as_git(root.as_deref(), dir);
    let base = root.clone().unwrap_or_else(|| dir.to_path_buf());
    let found = scan(dir).map_err(|e| Stop::Io { message: format!("reading {}: {e}", shown(dir)), restored: true })?;
    let mut problems = Vec::new();

    // step 1: objects whose content does not match their names stop everything
    for o in &found.objects {
        match crate::fs::verify_dir(o) {
            Ok(v) if !toa_core::problem::has_errors(&v.problems) => {}
            Ok(v) => problems.push(problem(
                "E_SEAL_CHANGED",
                shown(o),
                format!(
                    "an object directory whose content does not match its name, or not well-formed ({}); it may be \
                     someone else's object that was changed: make it a draft or restore it",
                    v.problems.first().map(|p| p.to_string()).unwrap_or_default()
                ),
            )),
            Err(e) => problems.push(problem("E_SEAL_READ", shown(o), e.to_string())),
        }
    }

    // read the drafts
    let mut drafts: BTreeMap<String, Draft> = BTreeMap::new();
    for d in &found.drafts {
        let n = leaf(d);
        if let Some(other) = drafts.get(&n) {
            problems.push(problem(
                "E_SEAL_DUPLICATE",
                shown(d),
                format!("the draft name is also used by {}", shown(&other.path)),
            ));
            continue;
        }
        let contents = match read_dir(d) {
            Ok(c) => c,
            Err(e) => {
                problems.push(problem("E_SEAL_READ", shown(d), e.to_string()));
                continue;
            }
        };
        let Some((_, mb)) = contents.files.iter().find(|(p, _)| p == tree::MANIFEST) else {
            problems.push(problem("E_TREE_NO_MANIFEST", shown(d), "a draft has a toa.yaml"));
            continue;
        };
        let mb = mb.clone();
        let manifest = match manifest::parse(&mb) {
            Ok(m) => m,
            Err(ps) => {
                for mut p in ps {
                    p.path = format!("{}/{}", shown(d), p.path);
                    problems.push(p);
                }
                continue;
            }
        };
        let lock_bytes = contents.files.iter().find(|(p, _)| p == tree::LOCK).map(|(_, b)| b.clone());
        let mut deps = draft::drafts_in_manifest(&manifest);
        if let Some(lb) = &lock_bytes {
            match serde_json::from_slice::<serde_json::Value>(lb) {
                Ok(v) => deps.extend(draft::drafts_in_json(&v)),
                Err(e) => problems.push(problem("E_LOCK", format!("{}/lock.json", shown(d)), e.to_string())),
            }
        }
        let members_bytes = contents.files.iter().find(|(p, _)| p == tree::MEMBERS).map(|(_, b)| b.clone());
        if let Some(b) = &members_bytes {
            deps.extend(members::drafts(b));
        }
        deps.sort();
        deps.dedup();
        deps.retain(|x| *x != n);
        drafts.insert(
            n.clone(),
            Draft { path: d.clone(), name: n, contents, manifest_bytes: mb, manifest, lock_bytes, members_bytes, deps },
        );
    }
    for d in drafts.values() {
        for x in &d.deps {
            if !drafts.contains_key(x) {
                problems.push(problem(
                    "E_SEAL_UNKNOWN_DRAFT",
                    shown(&d.path),
                    format!("names the draft '{x}', which is not among the drafts under {}", shown(dir)),
                ));
            }
        }
    }

    // step 2: order by what they depend on
    let order = match topological(&drafts) {
        Ok(o) => o,
        Err(cycle) => {
            problems.push(problem(
                "E_SEAL_CYCLE",
                "",
                format!("the drafts name each other in a cycle: {}", cycle.join(" -> ")),
            ));
            Vec::new()
        }
    };
    if !problems.is_empty() {
        return Err(Stop::Problems(problems));
    }

    let mut roots = vec![base.clone()];
    roots.extend(opts.lookup.iter().cloned());
    // the workspace list: its libraries are looked in too, and those with an address give the
    // `git` sources of the objects published in them
    let ws = opts.workspace.clone().or_else(|| crate::fetch::find_workspace(dir));
    let ws = ws.and_then(|w| crate::fetch::read_workspace(&w).ok());
    if let Some(w) = &ws {
        roots.extend(crate::fetch::reachable_libraries(w).into_iter().map(|(d, _, _)| d));
    }
    let mut machine = Machine::new(&roots);
    if let Some(w) = &ws {
        machine.places = crate::fetch::published_places(git, w);
    }
    let mut journal = Journal { dry: opts.dry_run, ..Default::default() };
    let mut out = Sealed::default();
    let mut map: BTreeMap<String, String> = BTreeMap::new();

    for n in order {
        let d = &drafts[&n];
        match seal_one(git, root.as_deref(), &base, d, &map, &mut machine, &mut journal, &mut out) {
            Ok(object) => {
                map.insert(n.clone(), object);
            }
            Err(Stop::Problems(ps)) => {
                let failed = journal.undo();
                if !failed.is_empty() {
                    for f in &failed {
                        eprintln!("toa: {f}");
                    }
                    return Err(Stop::Io { message: "sealing stopped".into(), restored: false });
                }
                return Err(Stop::Problems(ps));
            }
            Err(Stop::Io { message, .. }) => return Err(journal.fail(message)),
            Err(Stop::Git(m)) => return Err(journal.fail(m)),
        }
    }
    if root.is_none() && !out.objects.is_empty() {
        out.notices.push(
            "not in a git repository: the hash was computed from the files only, not a second time by git (7.1, \
             step 3.4); verifying before publication and the library check will find what that would have found"
                .into(),
        );
    }
    Ok(out)
}

fn topological(drafts: &BTreeMap<String, Draft>) -> Result<Vec<String>, Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Open,
        Done,
    }
    fn visit(
        n: &str,
        drafts: &BTreeMap<String, Draft>,
        marks: &mut BTreeMap<String, Mark>,
        stack: &mut Vec<String>,
        out: &mut Vec<String>,
    ) -> Result<(), Vec<String>> {
        match marks.get(n) {
            Some(Mark::Done) => return Ok(()),
            Some(Mark::Open) => {
                let at = stack.iter().position(|x| x == n).unwrap_or(0);
                let mut c = stack[at..].to_vec();
                c.push(n.to_string());
                return Err(c);
            }
            None => {}
        }
        marks.insert(n.to_string(), Mark::Open);
        stack.push(n.to_string());
        for dep in &drafts[n].deps {
            if drafts.contains_key(dep) {
                visit(dep, drafts, marks, stack, out)?;
            }
        }
        stack.pop();
        marks.insert(n.to_string(), Mark::Done);
        out.push(n.to_string());
        Ok(())
    }
    let mut marks = BTreeMap::new();
    let mut out = Vec::new();
    for n in drafts.keys() {
        visit(n, drafts, &mut marks, &mut Vec::new(), &mut out)?;
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn seal_one(
    git: &dyn Git,
    root: Option<&Path>,
    base: &Path,
    d: &Draft,
    map: &BTreeMap<String, String>,
    machine: &mut Machine,
    journal: &mut Journal,
    out: &mut Sealed,
) -> Result<String, Stop> {
    let at = shown(&d.path);
    let io = |e: std::io::Error| Stop::Io { message: e.to_string(), restored: true };
    let probs = |ms: Vec<String>| Stop::Problems(ms.into_iter().map(|m| problem("E_SEAL_NAME", &at, m)).collect());

    // 3.1: names in place of draft names
    let mut manifest_bytes = d.manifest_bytes.clone();
    if let Some(b) = draft::replace_in_manifest(&manifest_bytes, &d.manifest, map, true).map_err(probs)? {
        manifest_bytes = b;
    }
    let mut lock_bytes = d.lock_bytes.clone();
    if let Some(lb) = &d.lock_bytes {
        if let Some(b) = draft::replace_in_json(lb, map, true).map_err(probs)? {
            lock_bytes = Some(b);
        }
    }
    let mut members_bytes = d.members_bytes.clone();
    if let Some(mb) = &d.members_bytes {
        if let Some(b) = draft::replace_in_members(mb, map, true).map_err(probs)? {
            members_bytes = Some(b);
        }
    }
    // 3.2: history.json with the generation, and an empty base removed
    let parsed = manifest::parse(&manifest_bytes).map_err(Stop::Problems)?;
    if let Some(b) = draft::remove_empty_base(&manifest_bytes, &parsed).map_err(|m| probs(vec![m]))? {
        manifest_bytes = b;
    }
    let parsed = manifest::parse(&manifest_bytes).map_err(Stop::Problems)?;
    let base_names: Vec<String> = parsed
        .get("base")
        .and_then(Node::as_seq)
        .map(|v| v.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let mut lineage: Option<History> = None;
    if let Some(first) = base_names.first() {
        // an item of another type is reported by the check of the common fields (3.3)
        if name::prefix_of(first) == name::prefix_of(&d.name) {
            let Some(parent) = machine.get(first) else {
                return Err(Stop::Problems(vec![problem(
                    "E_SEAL_PARENT",
                    &at,
                    format!(
                        "the first item of base, {first}, is not on this machine or does not verify from its \
                         files; fetch it first (toa fetch {first})"
                    ),
                )]));
            };
            let own_name = parsed.get("name").and_then(Node::as_str);
            let parent_name = parent.manifest.as_ref().and_then(|m| m.get("name")).and_then(Node::as_str);
            lineage = Some(history::of(first, parent.history.as_ref(), history::same_series(own_name, parent_name)));
        }
    }
    let history_bytes = lineage.as_ref().map(history::write);

    // the files as they will be
    let mut files: Vec<(String, Vec<u8>)> = d
        .contents
        .files
        .iter()
        .filter(|(p, _)| p != tree::HISTORY)
        .map(|(p, b)| match p.as_str() {
            tree::MANIFEST => (p.clone(), manifest_bytes.clone()),
            tree::LOCK => (p.clone(), lock_bytes.clone().unwrap_or_default()),
            tree::MEMBERS => (p.clone(), members_bytes.clone().unwrap_or_default()),
            _ => (p.clone(), b.clone()),
        })
        .collect();
    if let Some(h) = &history_bytes {
        files.push((tree::HISTORY.to_string(), h.clone()));
        files.sort();
    }

    // 3.3: check
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let dirs: Vec<&str> = d.contents.dirs.iter().map(String::as_str).collect();
    let exe: Vec<&str> = d.contents.executable.iter().map(String::as_str).collect();
    let mut v = object::verify_files(&d.name, &refs, &dirs, &exe);
    let mut ps = d.contents.problems.clone();
    ps.append(&mut v.problems);
    // the list of members keeps 3.3 once its draft names are replaced
    if let Some(b) = &members_bytes {
        ps.extend(members::read(b).1);
    }
    // what the type specification asks (types it does not know are sealed all the same)
    if !toa_core::problem::has_errors(&ps) {
        let prefix = name::prefix_of(&d.name).unwrap_or("");
        let lock_value = lock_bytes.as_deref().and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok());
        let o = toa_types::Object { manifest: &parsed, files: &files, lock: lock_value.as_ref() };
        ps.extend(toa_types::check(prefix, &o, &MachineLookup(std::cell::RefCell::new(machine))));
    }
    if toa_core::problem::has_errors(&ps) {
        for p in &mut ps {
            p.path = if p.path.is_empty() { at.clone() } else { format!("{at}/{}", p.path) };
        }
        return Err(Stop::Problems(ps));
    }
    // warnings do not stop sealing; the user is told
    for p in &ps {
        out.notices.push(format!("{}: {p}", d.name));
    }
    let Some(hash) = v.hash else {
        return Err(Stop::Problems(vec![problem("E_HASH", &at, "the hash could not be computed")]));
    };
    let prefix = name::prefix_of(&d.name).unwrap_or("");
    let object_name = name::object_name(prefix, &hash);

    // write the files that change
    if manifest_bytes != d.manifest_bytes {
        journal.write(&d.path.join(tree::MANIFEST), &manifest_bytes).map_err(io)?;
    }
    if let (Some(new), Some(old)) = (&lock_bytes, &d.lock_bytes) {
        if new != old {
            journal.write(&d.path.join(tree::LOCK), new).map_err(io)?;
        }
    }
    if let (Some(new), Some(old)) = (&members_bytes, &d.members_bytes) {
        if new != old {
            journal.write(&d.path.join(tree::MEMBERS), new).map_err(io)?;
        }
    }
    let old_history = d.contents.files.iter().find(|(p, _)| p == tree::HISTORY).map(|(_, b)| b.clone());
    match (&history_bytes, &old_history) {
        (Some(h), o) if o.as_ref() != Some(h) => journal.write(&d.path.join(tree::HISTORY), h).map_err(io)?,
        (None, Some(_)) => journal.remove(&d.path.join(tree::HISTORY)).map_err(io)?,
        _ => {}
    }

    // 3.4: the second computation, by git with the settings of this repository
    if let (Some(root), false) = (root, journal.dry) {
        let Ok(rel) = d.path.strip_prefix(root).map(shown) else {
            return Err(Stop::Problems(vec![problem(
                "E_SEAL_GIT_HASH",
                &at,
                format!("not inside the working directory {} that git names", shown(root)),
            )]));
        };
        match git.tree_of_dir(root, &rel) {
            Ok(t) if t == hash => {}
            Ok(t) => {
                let why = differences(git, root, &t, &files);
                return Err(Stop::Problems(vec![problem(
                    "E_SEAL_GIT_HASH",
                    &at,
                    format!("git gives {t}, the files give {hash}: {why}"),
                )]));
            }
            Err(crate::git::GitError::NeedsGit(_)) => out.notices.push(format!(
                "{}: no system git, so the second computation by git was not made (7.1 step 3.4); only the files were hashed",
                d.name
            )),
            Err(e) => {
                return Err(Stop::Problems(vec![problem(
                    "E_SEAL_GIT_HASH",
                    &at,
                    format!("git could not compute the hash ({e}); are all its files ignored by .gitignore?"),
                )]))
            }
        }
    } else if journal.dry && root.is_some() {
        out.notices.push(format!("{}: --dry-run does not make the second computation by git", d.name));
    }

    // 3.5: rename, with the sidecar files
    let new_path = d.path.with_file_name(&object_name);
    if std::fs::symlink_metadata(&new_path).is_ok() {
        return Err(Stop::Problems(vec![problem(
            "E_SEAL_EXISTS",
            &at,
            format!("{} exists already (two drafts of the same content in one directory?)", shown(&new_path)),
        )]));
    }
    let sides = sidecars(&d.path, &d.name);
    let mut old_list = None;
    for (p, suffix) in &sides {
        let to = p.with_file_name(format!("{object_name}{suffix}"));
        if suffix == source::SUFFIX {
            old_list = std::fs::read(p).ok();
        }
        journal.rename(p, &to).map_err(io)?;
    }
    journal.rename(&d.path, &new_path).map_err(io)?;

    // 3.6: its source list
    let lock_value = lock_bytes.as_deref().and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok());
    machine.add(
        &object_name,
        Known {
            path: Some(new_path.clone()),
            manifest: Some(parsed.clone()),
            lock: lock_value.clone(),
            members: members_bytes.clone(),
            history: lineage,
        },
    );
    let list = source_list(
        &object_name,
        Some(&parsed),
        lock_value.as_ref(),
        members_bytes.as_deref(),
        old_list.as_deref(),
        machine,
        base,
    );
    let list_path = new_path.with_file_name(format!("{object_name}{}", source::SUFFIX));
    journal.write(&list_path, list.as_bytes()).map_err(io)?;

    out.objects.push((d.name.clone(), object_name.clone(), new_path));
    Ok(object_name)
}

/// Why git's tree differs from the files: the paths that differ.
fn differences(git: &dyn Git, root: &Path, tree_id: &str, files: &[(String, Vec<u8>)]) -> String {
    let Ok(listing) = git.list_tree(root, tree_id, tree::MAX_FILES * 2) else {
        return "often a file ignored by .gitignore, an executable file, or converted line ends".into();
    };
    let mut why = Vec::new();
    let mut in_git = BTreeSet::new();
    for e in listing.iter().filter(|e| e.is_blob()) {
        in_git.insert(e.path.clone());
        match files.iter().find(|(p, _)| *p == e.path) {
            None => why.push(format!("{} is only in git's tree", e.path)),
            Some((_, b)) => {
                let id = toa_core::hash::blob_id(toa_core::hash::Algorithm::Sha1, b).map(|i| toa_core::hash::hex(&i));
                if id.ok().as_deref() != Some(e.id.as_str()) {
                    why.push(format!("{} differs (converted line ends? a filter?)", e.path));
                }
            }
        }
        if e.mode == "100755" {
            why.push(format!("{} is executable", e.path));
        }
    }
    for (p, _) in files {
        if !in_git.contains(p) {
            why.push(format!("{p} is not in git's tree (ignored by .gitignore?)"));
        }
    }
    if why.is_empty() {
        "the trees differ".into()
    } else {
        why.join("; ")
    }
}

// ------------------------------------------------------------------ unsealing

/// What unsealing did: object name, draft name, the new path.
#[derive(Debug, Default)]
pub struct Unsealed {
    pub drafts: Vec<(String, String, PathBuf)>,
}

/// The draft name unsealing gives an object: `<prefix>_draft-<first 12 hex digits>`.
pub fn draft_name_for(object: &str) -> String {
    let (prefix, hash) = name::split(object).unwrap_or(("x", "0"));
    format!("{prefix}_draft-{}", &hash[..hash.len().min(12)])
}

/// The manifest bytes, the manifest, and the `lock.json` and `members.source.jsonl` bytes of an
/// object.
type Texts = (Vec<u8>, Node, Option<Vec<u8>>, Option<Vec<u8>>);

/// Turn an object back into a draft, with the objects under `dir` that refer to it and are not
/// published, level by level up (7.1). `branch` is the publication branch.
pub fn unseal(git: &dyn Git, dir: &Path, object: &str, branch: &str) -> Result<Unsealed, Stop> {
    if !name::is_object_name(object) {
        return Err(Stop::Problems(vec![problem("E_NAME", object, "not an object name")]));
    }
    let root = git.toplevel(dir).map_err(|e| Stop::Git(e.to_string()))?;
    let dir = &spelled_as_git(root.as_deref(), dir);
    let found = scan(dir).map_err(|e| Stop::Io { message: e.to_string(), restored: true })?;
    let published: BTreeSet<String> = match &root {
        Some(r) => match library::publication_head(git, r, branch) {
            Ok(Some(h)) => library::objects_in(git, r, &h.commit, 2_000_000)
                .map_err(|e| Stop::Git(e.to_string()))?
                .into_keys()
                .collect(),
            Ok(None) => BTreeSet::new(),
            Err(e) => return Err(Stop::Git(e.to_string())),
        },
        None => BTreeSet::new(),
    };
    let mut copies: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for o in &found.objects {
        copies.entry(leaf(o)).or_default().push(o.clone());
    }
    let mut problems = Vec::new();
    // which objects refer to which
    let mut refers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut texts: BTreeMap<String, Texts> = BTreeMap::new();
    for (n, paths) in &copies {
        let p = &paths[0];
        let Ok(mb) = std::fs::read(p.join(tree::MANIFEST)) else { continue };
        let Ok(m) = manifest::parse(&mb) else { continue };
        let lb = std::fs::read(p.join(tree::LOCK)).ok();
        let lv = lb.as_deref().and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok());
        let sb = std::fs::read(p.join(tree::MEMBERS)).ok();
        for (r, _) in refs_of(Some(&m), lv.as_ref(), sb.as_deref()) {
            refers.entry(r).or_default().push(n.clone());
        }
        texts.insert(n.clone(), (mb, m, lb, sb));
    }
    if !copies.contains_key(object) {
        return Err(Stop::Problems(vec![problem(
            "E_UNSEAL_MISSING",
            object,
            format!("not found under {}", shown(dir)),
        )]));
    }
    if published.contains(object) {
        return Err(Stop::Problems(vec![problem(
            "E_UNSEAL_PUBLISHED",
            object,
            format!("it is on the publication branch '{branch}' (6.3); a published object is not unsealed"),
        )]));
    }
    // the set: the object, and the unpublished objects that refer to it, level by level up
    let mut set = vec![object.to_string()];
    let mut i = 0;
    while i < set.len() {
        let n = set[i].clone();
        i += 1;
        for r in refers.get(&n).cloned().unwrap_or_default() {
            if !set.contains(&r) && !published.contains(&r) {
                set.push(r);
            }
        }
    }
    let existing: BTreeSet<String> = found.drafts.iter().map(|d| leaf(d)).collect();
    let mut map = BTreeMap::new();
    for n in &set {
        let paths = &copies[n];
        if paths.len() > 1 {
            problems.push(problem(
                "E_UNSEAL_COPIES",
                n,
                format!(
                    "kept {} times ({}); remove the extra copies first",
                    paths.len(),
                    paths.iter().map(|p| shown(p)).collect::<Vec<_>>().join(", ")
                ),
            ));
            continue;
        }
        let dn = draft_name_for(n);
        let target = paths[0].with_file_name(&dn);
        if existing.contains(&dn) || std::fs::symlink_metadata(&target).is_ok() {
            problems.push(problem("E_UNSEAL_EXISTS", n, format!("the draft name {dn} is taken")));
        }
        map.insert(n.clone(), dn);
    }
    // the replacements, computed before anything changes
    let mut edits: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for n in &set {
        let Some((mb, m, lb, sb)) = texts.get(n) else {
            problems.push(problem("E_UNSEAL_READ", n, "its toa.yaml cannot be read"));
            continue;
        };
        let p = &copies[n][0];
        match draft::replace_in_manifest(mb, m, &map, false) {
            Ok(Some(b)) => edits.push((p.join(tree::MANIFEST), b)),
            Ok(None) => {}
            Err(ms) => problems.extend(ms.into_iter().map(|x| problem("E_UNSEAL_NAME", n, x))),
        }
        if let Some(lb) = lb {
            match draft::replace_in_json(lb, &map, false) {
                Ok(Some(b)) => edits.push((p.join(tree::LOCK), b)),
                Ok(None) => {}
                Err(ms) => problems.extend(ms.into_iter().map(|x| problem("E_UNSEAL_NAME", n, x))),
            }
        }
        if let Some(sb) = sb {
            match draft::replace_in_members(sb, &map, false) {
                Ok(Some(b)) => edits.push((p.join(tree::MEMBERS), b)),
                Ok(None) => {}
                Err(ms) => problems.extend(ms.into_iter().map(|x| problem("E_UNSEAL_NAME", n, x))),
            }
        }
    }
    if !problems.is_empty() {
        return Err(Stop::Problems(problems));
    }
    let mut journal = Journal::default();
    for (p, b) in &edits {
        if let Err(e) = journal.write(p, b) {
            return Err(journal.fail(e.to_string()));
        }
    }
    let mut out = Unsealed::default();
    for n in &set {
        let p = &copies[n][0];
        let dn = &map[n];
        for (side, suffix) in sidecars(p, n) {
            if let Err(e) = journal.rename(&side, &side.with_file_name(format!("{dn}{suffix}"))) {
                return Err(journal.fail(e.to_string()));
            }
        }
        let to = p.with_file_name(dn);
        if let Err(e) = journal.rename(p, &to) {
            return Err(journal.fail(e.to_string()));
        }
        out.drafts.push((n.clone(), dn.clone(), to));
    }
    Ok(out)
}
