//! Publishing (7.2): what a tool does before sealed objects go onto the publication branch. It
//! makes sure there are no drafts, verifies from their files the objects published this time,
//! tidies their source lists by 5.3, and writes the library list again (5.2). It changes files,
//! and with `commit` makes a commit; it never pushes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toa_core::manifest::{self, Node};
use toa_core::name::{self, Form};
use toa_core::source::{self, IncludeLine, Kind, Line, ObjectLine, Source};
use toa_core::{history, tree, Problem};
use toa_types::readable;

use crate::git::Git;
use crate::library;
use crate::seal::{self, refs_of, shown, Journal, Stop};

/// How to publish.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The address of this library; the remote `origin` when not given (5.3).
    pub to: Option<String>,
    pub branch: String,
    pub commit: bool,
    pub dry_run: bool,
    /// The workspace list, to tell which ancestors others can still find (5.3 item 5).
    pub workspace: Option<PathBuf>,
    /// Write the library list in volumes (5.2) even when it was not; one in volumes stays so.
    pub volumes: bool,
    /// The commit must be signed (`git commit -S`); without it, the user's git settings decide.
    pub sign: bool,
    /// Leave the `latest` marks of the library list as they are (5.2): neither move nor add any.
    pub keep_latest: bool,
}

/// What publishing did, and what the user must be told (5.3, chapter 11).
#[derive(Debug, Default)]
pub struct Published {
    /// The objects published this time.
    pub objects: Vec<String>,
    /// The address of this library written into the lists, if any.
    pub address: Option<String>,
    /// Files written or removed, relative to the root.
    pub changed: Vec<String>,
    /// The other addresses the lists keep: they become public with them.
    pub addresses: Vec<String>,
    /// Objects of the lists not in this library with no git source left: only their names stay
    /// (5.3 item 4); others cannot find them by name.
    pub name_only: Vec<String>,
    /// Ancestors in the history of the new objects that are neither in this library nor in a
    /// library of the workspace list with a git address (5.3 item 5): (object, ancestor).
    pub lost_ancestors: Vec<(String, String)>,
    /// Objects on the publication branch whose list beside them still held local paths, tidied
    /// this time (5.3).
    pub repaired: Vec<String>,
    /// The paths the commit was asked to hold (relative to the root).
    pub committed: Vec<String>,
    pub notices: Vec<String>,
    /// The commit made with `commit`, and what the library check found on it.
    pub commit: Option<String>,
    /// The kind of signature of that commit, read from it (not verified), or `None` (6.3).
    pub signature: Option<crate::git::SignatureFormat>,
    /// The `latest` marks given this time (5.2): (the object the mark was taken from, if any, the
    /// object marked).
    pub latest: Vec<(Option<String>, String)>,
    pub check: Vec<Problem>,
}

/// The series of an object (3.4): its type and its `name`. Types without a name have none here.
fn series_of(n: &str, h: &Here) -> Option<(String, String)> {
    let prefix = name::prefix_of(n)?;
    let own = h.manifest.as_ref()?.get("name")?.as_str()?;
    Some((prefix.to_string(), own.to_string()))
}

/// The tool's default for the `latest` marks (object structure 14, 5.2), for objects the library
/// list has not had yet, oldest generation first. A series is linked by `base` (3.4), so an
/// object continues the series of its first `base` item when that has its type and `name`:
/// - that item is here and marked: the object takes the mark from it;
/// - that item is here and not marked: the maintainer did not recommend the series here, and the
///   object gets none;
/// - otherwise (no `base`, another series, or an earlier generation kept elsewhere) the object is
///   the first of its series here, and gets a mark.
///
/// Objects of types without a name (`evd`, `doc`) get none. Returns what changed: (the object the
/// mark was taken from, the object marked).
fn default_marks(
    here: &BTreeMap<String, Here>,
    fresh: &[&String],
    marks: &mut BTreeSet<String>,
) -> Vec<(Option<String>, String)> {
    let generation = |n: &str| here[n].history.as_ref().map_or(1, |h| h.generation);
    let mut order: Vec<&String> = fresh.to_vec();
    order.sort_by_key(|n| (generation(n), (*n).clone()));
    let mut out = Vec::new();
    for n in order {
        let Some(series) = series_of(n, &here[n.as_str()]) else { continue };
        let parent = here[n.as_str()]
            .manifest
            .as_ref()
            .and_then(|m| m.get("base"))
            .and_then(Node::as_seq)
            .and_then(|s| s.first())
            .and_then(Node::as_str)
            .map(str::to_string);
        let continues = parent.filter(|p| here.get(p).and_then(|h| series_of(p, h)).as_ref() == Some(&series));
        match continues {
            Some(p) if marks.contains(&p) => {
                marks.remove(&p);
                marks.insert(n.clone());
                out.push((Some(p), n.clone()));
            }
            Some(_) => {}
            None => {
                if marks.insert(n.clone()) {
                    out.push((None, n.clone()));
                }
            }
        }
    }
    out
}

fn problem(code: &'static str, path: impl Into<String>, msg: impl Into<String>) -> Problem {
    Problem::error(code, "7.2", path, msg)
}

/// The address of this library as 5.3 writes it: an `https` address loses its user name and
/// password, the host is in lower case; the form `user@host:path` is not rewritten.
pub fn library_address(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let fixed = if let Some(rest) = raw.strip_prefix("https://") {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        format!("https://{}/{path}", host.to_ascii_lowercase())
    } else if let Some(rest) = raw.strip_prefix("ssh://") {
        let (user, rest) = match rest.split_once('@') {
            Some((u, r)) if !u.contains('/') => (format!("{u}@"), r),
            _ => (String::new(), rest),
        };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        format!("ssh://{user}{}/{path}", host.to_ascii_lowercase())
    } else if raw.contains(':') && !raw.contains("://") && raw.split(':').next().is_some_and(|h| !h.contains('/')) {
        return Err(format!(
            "'{raw}' has the form user@host:path, which is not rewritten (5.3); give the address as https://… or \
             ssh://… with --to"
        ));
    } else {
        raw.to_string()
    };
    match source::address_problem(&fixed) {
        None => Ok(fixed),
        Some(w) => Err(format!("'{raw}' is not an address others can use ({w}); it is not added (5.3)")),
    }
}

/// An object in the working directory: its copies and what its files say.
struct Here {
    paths: Vec<String>,
    manifest: Option<Node>,
    lock: Option<serde_json::Value>,
    members: Option<Vec<u8>>,
    history: Option<toa_core::history::History>,
}

/// Files under the root whose names end with `suffix`, outside objects and `.git`.
fn files_ending(root: &Path, suffix: &str) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, suffix: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let Some(n) = e.file_name().to_str().map(str::to_string) else { continue };
            let Ok(ft) = e.file_type() else { continue };
            let path = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
            if ft.is_dir()
                && n != ".git"
                && name::form(&n) == Form::Other
                && !n.ends_with(toa_core::library::NOTES_SUFFIX)
                && !n.ends_with(toa_core::library::STAMPS_SUFFIX)
            {
                walk(&e.path(), &path, suffix, out);
            } else if ft.is_file() && n.ends_with(suffix) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", suffix, &mut out);
    out.sort();
    out
}

/// Prepare the library at `dir` for publication (7.2, 5.3).
pub fn publish(git: &dyn Git, dir: &Path, opts: &Options) -> Result<Published, Stop> {
    let Some(root) = git.toplevel(dir).map_err(|e| Stop::Git(e.to_string()))? else {
        return Err(Stop::Problems(vec![problem(
            "E_PUBLISH_REPO",
            shown(dir),
            "not in a git repository: a library is one (6.1)",
        )]));
    };
    let io = |e: std::io::Error| Stop::Io { message: e.to_string(), restored: true };
    let found = seal::scan(&root).map_err(io)?;
    let mut out = Published::default();

    // 1. no drafts; the objects published this time verify from their files
    let mut problems: Vec<Problem> = found
        .drafts
        .iter()
        .map(|d| {
            problem("E_PUBLISH_DRAFT", shown(d.strip_prefix(&root).unwrap_or(d)), "a draft: seal it first (toa seal)")
        })
        .collect();
    let mut here: BTreeMap<String, Here> = BTreeMap::new();
    for o in &found.objects {
        let rel = shown(o.strip_prefix(&root).unwrap_or(o));
        let n = o.file_name().and_then(|x| x.to_str()).unwrap_or("").to_string();
        if let Some(h) = here.get_mut(&n) {
            h.paths.push(rel);
            continue;
        }
        let manifest = std::fs::read(o.join(tree::MANIFEST)).ok().and_then(|b| manifest::parse(&b).ok());
        let lock = std::fs::read(o.join(tree::LOCK)).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let members = std::fs::read(o.join(tree::MEMBERS)).ok();
        let history = std::fs::read(o.join(tree::HISTORY)).ok().and_then(|b| history::read(&b).ok());
        here.insert(n, Here { paths: vec![rel], manifest, lock, members, history });
    }
    let (head, published) = match library::publication_head(git, &root, &opts.branch) {
        Ok(Some(h)) => {
            let names = library::objects_in(git, &root, &h.commit, library::Limits::default().max_entries)
                .map_err(|e| Stop::Git(e.to_string()))?;
            (Some(h.commit), names)
        }
        Ok(None) => (None, BTreeMap::new()),
        Err(e) => return Err(Stop::Git(e.to_string())),
    };
    let new: Vec<String> = here.keys().filter(|n| !published.contains_key(*n)).cloned().collect();
    for n in &new {
        for p in &here[n].paths {
            match crate::fs::verify_dir(&root.join(p)) {
                Ok(v) if !toa_core::problem::has_errors(&v.problems) => {}
                Ok(v) => problems.extend(v.problems.into_iter().map(|mut x| {
                    x.path = if x.path.is_empty() { p.clone() } else { format!("{p}/{}", x.path) };
                    x
                })),
                Err(e) => problems.push(problem("E_PUBLISH_READ", p, e.to_string())),
            }
        }
    }
    if !problems.is_empty() {
        return Err(Stop::Problems(problems));
    }
    out.objects = new.clone();

    // the address of this library
    let raw = match &opts.to {
        Some(a) => Some(a.clone()),
        None => git.remote_url(&root, "origin").map_err(|e| Stop::Git(e.to_string()))?,
    };
    out.address = match raw {
        Some(r) => match library_address(&r) {
            Ok(a) => Some(a),
            Err(m) => {
                out.notices.push(m);
                None
            }
        },
        None => {
            out.notices.push("no address of this library: no --to, and no remote origin; none is added (5.3)".into());
            None
        }
    };

    // the commit of objects already on the publication branch: the earliest along first parents
    let mut first_commit: BTreeMap<String, String> = BTreeMap::new();
    if let Some(h) = &head {
        if let Ok(adds) = git.additions(&root, h, None, &["*/toa.yaml"]) {
            for a in adds {
                for p in a.paths {
                    let dir = p.trim_end_matches("/toa.yaml");
                    let leaf = dir.rsplit('/').next().unwrap_or("");
                    if published.contains_key(leaf) {
                        first_commit.entry(leaf.to_string()).or_insert_with(|| a.commit.clone());
                    }
                }
            }
        }
    }

    // the objects in the libraries of the workspace list that others can reach (with a `git`
    // address): for 5.3 item 5, and the names of their interfaces for labels
    let mut elsewhere: BTreeSet<String> = BTreeSet::new();
    let mut elsewhere_ip: BTreeMap<String, String> = BTreeMap::new();
    let ws = opts.workspace.clone().or_else(|| crate::fetch::find_workspace(dir));
    if let Some(w) = ws.and_then(|w| crate::fetch::read_workspace(&w).ok()) {
        for (d, _, branch) in crate::fetch::reachable_libraries(&w) {
            // what its publication branch holds (one checkout may hold several libraries, one
            // per branch); the working directory when the branch cannot be read
            let mut found: Vec<(String, Option<Vec<u8>>)> = Vec::new();
            let head = library::publication_head(git, &d, &branch).ok().flatten();
            let listing =
                head.as_ref().and_then(|h| git.list_tree(&d, &h.commit, library::Limits::default().max_entries).ok());
            if let (Some(h), Some(listing)) = (&head, listing) {
                for f in toa_core::library::objects(&listing) {
                    let m = (name::prefix_of(&f.name) == Some(toa_types::IFC))
                        .then(|| git.object_at(&d, &h.commit, &format!("{}/{}", f.path, tree::MANIFEST)).ok().flatten())
                        .flatten()
                        .and_then(|id| git.read_blob(&d, &id, manifest::MAX_BYTES as u64).ok());
                    found.push((f.name, m));
                }
            } else if let Ok(sc) = seal::scan(&d) {
                for o in &sc.objects {
                    let Some(n) = o.file_name().and_then(|x| x.to_str()) else { continue };
                    let m = (name::prefix_of(n) == Some(toa_types::IFC))
                        .then(|| std::fs::read(o.join(tree::MANIFEST)).ok())
                        .flatten();
                    found.push((n.to_string(), m));
                }
            }
            for (n, m) in found {
                if !elsewhere_ip.contains_key(&n) {
                    let m = m.and_then(|b| manifest::parse(&b).ok());
                    if let Some(v) = m.as_ref().and_then(|m| m.get("name")).and_then(Node::as_str) {
                        elsewhere_ip.insert(n.clone(), v.to_string());
                    }
                }
                elsewhere.insert(n);
            }
        }
    }
    let ip_name = |n: &str| {
        here.get(n)
            .and_then(|h| h.manifest.as_ref())
            .and_then(|m| m.get("name"))
            .and_then(Node::as_str)
            .map(str::to_string)
            .or_else(|| elsewhere_ip.get(n).cloned())
    };
    let cached = |n: &str| -> (Option<String>, Option<Vec<String>>, Option<String>) {
        let Some(h) = here.get(n) else { return (None, None, None) };
        let refs: Vec<String> =
            refs_of(h.manifest.as_ref(), h.lock.as_ref(), h.members.as_deref()).into_iter().map(|(r, _)| r).collect();
        let license = h.manifest.as_ref().and_then(|m| m.get("license")).and_then(Node::as_str).map(str::to_string);
        let generation = readable::generation(h.history.as_ref());
        (
            readable::label(n, h.manifest.as_ref(), generation, &h.paths[0], &ip_name),
            (!refs.is_empty()).then_some(refs),
            license,
        )
    };
    let mut journal = Journal::new(opts.dry_run);
    let mut addresses: BTreeSet<String> = BTreeSet::new();
    let mut name_only: BTreeSet<String> = BTreeSet::new();

    // objects on the publication branch whose list beside them still holds local paths: such a
    // list breaks 5.3 there, so it was never validly published, and it is tidied as if the object
    // were published now (as when sealed objects were committed before `toa publish`)
    let has_local = |b: &[u8]| {
        source::read(b, false, Kind::Other, source::MAX_SIDECAR_LIST, "").lines.iter().any(|l| match l {
            Line::Object(o) => o.sources.iter().any(|s| matches!(s, Source::Local(_))),
            Line::Include(i) => i.sources.iter().any(|s| matches!(s, Source::Local(_))),
        })
    };
    out.repaired = here
        .iter()
        .filter(|(n, _)| published.contains_key(*n))
        .filter(|(_, h)| {
            h.paths
                .iter()
                .any(|p| std::fs::read(root.join(format!("{p}{}", source::SUFFIX))).is_ok_and(|b| has_local(&b)))
        })
        .map(|(n, _)| n.clone())
        .collect();
    if !out.repaired.is_empty() {
        out.notices.push(format!(
            "{} object(s) on the publication branch {} still have local paths in the list beside them, which 5.3 \
             does not allow there; their lists are tidied as if they were published now",
            out.repaired.len(),
            opts.branch
        ));
    }
    let tidied: Vec<String> = new.iter().chain(out.repaired.iter()).cloned().collect();

    // 2. the source lists beside the objects published this time (5.3)
    for n in &tidied {
        for p in &here[n].paths {
            let list_rel = format!("{p}{}", source::SUFFIX);
            let list_path = root.join(&list_rel);
            let old = std::fs::read(&list_path).ok();
            let mut lines: Vec<Line> = match &old {
                Some(b) => source::read(b, false, Kind::Other, source::MAX_SIDECAR_LIST, "").lines,
                None => Vec::new(),
            };
            // an object without a list is given one: itself, and what it needs (5.2)
            if !lines.iter().any(|l| matches!(l, Line::Object(o) if o.object == *n)) {
                let mut needed: Vec<Line> = vec![Line::Object(ObjectLine { object: n.clone(), ..Default::default() })];
                for (r, _) in refs_of(here[n].manifest.as_ref(), here[n].lock.as_ref(), here[n].members.as_deref()) {
                    needed.push(Line::Object(ObjectLine { object: r, ..Default::default() }));
                }
                needed.append(&mut lines);
                lines = needed;
            }
            let mut tidy: Vec<Line> = Vec::new();
            for l in lines {
                match l {
                    Line::Include(i) => {
                        let had = !i.sources.is_empty();
                        let sources: Vec<Source> =
                            i.sources.into_iter().filter(|s| !matches!(s, Source::Local(_))).collect();
                        if had && sources.is_empty() {
                            continue; // all its sources were local
                        }
                        for s in &sources {
                            if let Source::Git { git, .. } = s {
                                addresses.insert(git.clone());
                            }
                        }
                        tidy.push(Line::Include(IncludeLine { sources, ..i }));
                    }
                    Line::Object(mut o) => {
                        let mut sources: Vec<Source> = Vec::new();
                        if let (Some(h), Some(a)) = (here.get(&o.object), &out.address) {
                            sources.push(Source::Git {
                                git: a.clone(),
                                commit: first_commit.get(&o.object).cloned(),
                                path: Some(h.paths[0].clone()),
                            });
                        }
                        for s in o.sources {
                            if let Source::Git { git, commit, path } = s {
                                let same_place = out.address.as_deref().is_some_and(|a| source::same_address(a, &git))
                                    && here.contains_key(&o.object);
                                if !same_place {
                                    addresses.insert(git.clone());
                                    sources.push(Source::Git { git, commit, path });
                                }
                            }
                        }
                        sources.truncate(source::MAX_SOURCES);
                        o.sources = sources;
                        if here.contains_key(&o.object) {
                            let (label, refs, license) = cached(&o.object);
                            o.label = label.or(o.label);
                            o.refs = refs;
                            o.license = license.or(o.license);
                        } else if o.sources.is_empty() {
                            o.label = None;
                            o.refs = None;
                            o.license = None;
                            o.group = None;
                            name_only.insert(o.object.clone());
                        }
                        o.removed = false;
                        o.latest = false; // it counts only in the library list (5.2)
                                          // the object itself first (5.2)
                        if o.object == *n {
                            tidy.insert(0, Line::Object(o));
                        } else {
                            tidy.push(Line::Object(o));
                        }
                    }
                }
            }
            let text = source::write(&tidy);
            if old.as_deref() != Some(text.as_bytes()) {
                if let Err(e) = journal.write(&list_path, text.as_bytes()) {
                    return Err(journal.fail(e.to_string()));
                }
                out.changed.push(list_rel);
            }
        }
    }

    for n in &new {
        for a in here[n].history.iter().flat_map(|h| &h.history) {
            if !here.contains_key(a) && !elsewhere.contains(a) {
                out.lost_ancestors.push((n.clone(), a.clone()));
            }
        }
    }

    // 3. the library list again (5.2), keeping the include lines the maintainer added
    let removed: BTreeMap<String, String> = files_ending(&root, toa_core::library::REMOVED_SUFFIX)
        .into_iter()
        .filter_map(|p| {
            let leaf = p.rsplit('/').next()?.strip_suffix(toa_core::library::REMOVED_SUFFIX)?.to_string();
            (name::is_object_name(&leaf) && !here.contains_key(&leaf))
                .then(|| (leaf, p.trim_end_matches(toa_core::library::REMOVED_SUFFIX).to_string()))
        })
        .collect();
    let list_path = root.join(source::LIBRARY_LIST);
    let old_list = std::fs::read(&list_path).ok();
    let old_lines = old_list
        .as_deref()
        .map(|b| source::read(b, false, Kind::Library, source::MAX_VOLUME, "").lines)
        .unwrap_or_default();
    let volumes_dir = root.join(source::LIBRARY_VOLUMES);
    let mut old_volume_lines: Vec<Line> = Vec::new();
    let old_volumes: Vec<String> = files_ending(&root, source::SUFFIX)
        .into_iter()
        .filter(|p| p.starts_with(&format!("{}/", source::LIBRARY_VOLUMES)))
        .collect();
    for v in &old_volumes {
        if let Ok(b) = std::fs::read(root.join(v)) {
            old_volume_lines.extend(source::read(&b, false, Kind::Library, source::MAX_VOLUME, "").lines);
        }
    }
    let old_objects =
        source::merged_objects(&old_lines.iter().chain(old_volume_lines.iter()).cloned().collect::<Vec<_>>());
    let old_groups: BTreeMap<String, String> =
        old_objects.iter().filter_map(|o| o.group.clone().map(|g| (o.object.clone(), g))).collect();
    // the `latest` marks are what the library says: kept as the maintainer left them, for the
    // objects still here (5.2); objects the list has not had yet move or get one by default
    let mut old_latest: BTreeSet<String> = old_objects.iter().filter(|o| o.latest).map(|o| o.object.clone()).collect();
    if !opts.keep_latest {
        let listed: BTreeSet<&str> = old_objects.iter().map(|o| o.object.as_str()).collect();
        let fresh: Vec<&String> = here.keys().filter(|n| !listed.contains(n.as_str())).collect();
        out.latest = default_marks(&here, &fresh, &mut old_latest);
    }
    // the group: the one the list had, else the interface name of the recommended layout (5.2),
    // the second level of the path, as in `ckt/<interface name>/<name>.g<generation>/<object>`
    let group = |n: &String, path: &str| {
        old_groups.get(n).cloned().or_else(|| {
            // a frozen library has no interface level (6.2)
            if name::prefix_of(n) == Some(toa_types::LIB) {
                return None;
            }
            let mut parts = path.split('/');
            let (_, g, _) = (parts.next()?, parts.next()?, parts.next()?);
            source::group_problem(g).is_none().then(|| g.to_string())
        })
    };
    let mut object_lines: Vec<Line> = Vec::new();
    let names: BTreeSet<&String> = here.keys().chain(removed.keys()).collect();
    for n in names {
        let line = if let Some(h) = here.get(n) {
            let (label, refs, license) = cached(n);
            ObjectLine {
                object: n.clone(),
                label,
                refs,
                license,
                group: group(n, &h.paths[0]),
                removed: false,
                latest: old_latest.contains(n),
                sources: h.paths.iter().take(source::MAX_SOURCES).map(|p| Source::Local(p.clone())).collect(),
            }
        } else {
            ObjectLine {
                object: n.clone(),
                group: group(n, &removed[n]),
                removed: true,
                sources: vec![Source::Local(removed[n].clone())],
                ..Default::default()
            }
        };
        object_lines.push(Line::Object(line));
    }
    let kept_includes: Vec<Line> = old_lines
        .iter()
        .filter(|l| matches!(l, Line::Include(i) if !i.include.starts_with(&format!("{}/", source::LIBRARY_VOLUMES))))
        .cloned()
        .collect();
    let mut writes: Vec<(String, String)> = Vec::new();
    if old_volumes.is_empty() && !opts.volumes {
        let mut lines = kept_includes;
        lines.extend(object_lines);
        writes.push((source::LIBRARY_LIST.into(), source::write(&lines)));
    } else {
        // in volumes, by the first two digits of the hash (5.2)
        let mut by: BTreeMap<String, Vec<Line>> = BTreeMap::new();
        for l in object_lines {
            if let Line::Object(o) = &l {
                let h = name::split(&o.object).map(|(_, h)| h[..2].to_string()).unwrap_or_default();
                by.entry(h).or_default().push(l);
            }
        }
        let mut top: Vec<Line> = by
            .keys()
            .map(|h| {
                Line::Include(IncludeLine {
                    include: format!("{}/{h}{}", source::LIBRARY_VOLUMES, source::SUFFIX),
                    ..Default::default()
                })
            })
            .collect();
        top.extend(kept_includes);
        writes.push((source::LIBRARY_LIST.into(), source::write(&top)));
        for (h, lines) in by {
            writes.push((format!("{}/{h}{}", source::LIBRARY_VOLUMES, source::SUFFIX), source::write(&lines)));
        }
        for v in &old_volumes {
            if !writes.iter().any(|(p, _)| p == v) {
                if let Err(e) = journal.remove(&root.join(v)) {
                    return Err(journal.fail(e.to_string()));
                }
                out.changed.push(v.clone());
            }
        }
        let _ = volumes_dir;
    }
    for (rel, text) in writes {
        let p = root.join(&rel);
        if std::fs::read(&p).ok().as_deref() == Some(text.as_bytes()) {
            continue;
        }
        if let Some(d) = p.parent() {
            if !journal.dry {
                if let Err(e) = std::fs::create_dir_all(d) {
                    return Err(journal.fail(e.to_string()));
                }
            }
        }
        if let Err(e) = journal.write(&p, text.as_bytes()) {
            return Err(journal.fail(e.to_string()));
        }
        out.changed.push(rel);
    }
    out.addresses = addresses.into_iter().collect();
    out.name_only = name_only.into_iter().collect();

    // the commit, and the check once more before anything is pushed (7.2)
    if opts.commit && !opts.dry_run && (!out.changed.is_empty() || !new.is_empty()) {
        let msg =
            format!("Publish {} object(s): tidy source lists and write the library list (toa publish)", new.len());
        // what the commit holds: the lists changed, the objects published this time with their
        // sidecar files, the removals, and the files of the library (6.1, 6.2), each as it is now
        let mut paths: BTreeSet<String> = out.changed.iter().cloned().collect();
        for n in &new {
            for p in &here[n].paths {
                paths.insert(p.clone());
                for s in [toa_core::library::NOTES_SUFFIX, toa_core::library::STAMPS_SUFFIX, source::SUFFIX] {
                    paths.insert(format!("{p}{s}"));
                }
            }
        }
        for p in removed.values() {
            paths.insert(p.clone());
            for s in [
                toa_core::library::REMOVED_SUFFIX,
                toa_core::library::NOTES_SUFFIX,
                toa_core::library::STAMPS_SUFFIX,
                source::SUFFIX,
            ] {
                paths.insert(format!("{p}{s}"));
            }
        }
        for f in [".gitattributes", toa_core::library::LIBRARY_YAML, source::LIBRARY_LIST, source::LIBRARY_VOLUMES] {
            paths.insert(f.to_string());
        }
        // only what is here, or was in the last commit (a removal)
        let at_head = |p: &str| git.object_at(&root, "HEAD", p).ok().flatten().is_some();
        let paths: Vec<String> =
            paths.into_iter().filter(|p| std::fs::symlink_metadata(root.join(p)).is_ok() || at_head(p)).collect();
        out.committed = paths.clone();
        match commit_and_check(git, &root, &paths, &msg, opts.sign, head.as_deref(), &mut out.notices) {
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

/// A commit made by the tool, with its signature and the library check of it (7.2).
pub(crate) struct Committed {
    pub commit: String,
    pub signature: Option<crate::git::SignatureFormat>,
    pub check: Vec<Problem>,
}

pub(crate) enum CommitError {
    /// The commit was not made; what was written may be undone.
    Commit(String),
    /// The commit was made, but could not be read or checked.
    After(String),
}

/// Commit these paths, signed as the user's git settings say (or must be, with `sign`; 6.3),
/// tell whether it is signed, and run the library check on it against `base`.
pub(crate) fn commit_and_check(
    git: &dyn Git,
    root: &Path,
    paths: &[String],
    msg: &str,
    sign: bool,
    base: Option<&str>,
    notices: &mut Vec<String>,
) -> Result<Committed, CommitError> {
    use std::io::IsTerminal;
    let signing = sign || git.signs_commits(root).unwrap_or(false);
    if signing && !std::io::stdin().is_terminal() {
        notices.push(format!(
            "the commit is signed, but not from a terminal: a signing program that asks for a passphrase \
             gets no answer and is ended after {} s; keep the key in ssh-agent or gpg-agent",
            crate::git::SIGNING_TIMEOUT.as_secs()
        ));
    }
    let c = git.commit_paths(root, paths, msg, sign).map_err(|e| CommitError::Commit(e.to_string()))?;
    let signature = git.commit_signature(root, &c).map_err(|e| CommitError::After(e.to_string()))?;
    if signature.is_none() {
        notices.push(
            "the commit is not signed; the commit that publishes objects should be (6.3): set \
             commit.gpgsign, gpg.format and user.signingkey in git, or give --sign"
                .into(),
        );
    }
    let check = library::check(git, root, &c, base, &library::Limits::default())
        .map(|r| r.problems)
        .map_err(|e| CommitError::After(e.to_string()))?;
    Ok(Committed { commit: c, signature, check })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_as_published() {
        assert_eq!(library_address("https://me:pw@GitHub.com/a/b.git").unwrap(), "https://github.com/a/b.git");
        assert_eq!(library_address("ssh://git@Host.org/x").unwrap(), "ssh://git@host.org/x");
        assert!(library_address("git@github.com:a/b.git").unwrap_err().contains("user@host:path"));
        assert!(library_address("../local/dir").is_err());
    }
}
