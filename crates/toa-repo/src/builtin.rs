//! The built-in git: gitoxide, compiled into `toa` (docs/lines/tools.md 9). It reads
//! repositories and fetches objects. It does not commit, and does not compute the tree git would
//! store for a working directory (the second computation of sealing, 7.1 step 3.4): those answer
//! what the user's own git does, and need it.
//!
//! What it adds to gitoxide, so that it answers as the system git does:
//! - **The check of what is received** (7.3 step 4), equal to git's `fetch.fsckObjects`
//!   ([`toa_core::fsck`]). Objects are received into a quarantine directory inside the git
//!   directory, checked there, and moved into the repository only when every one passes.
//! - **The user's network settings:** `http.proxy`, `http.sslCAInfo` and `http.sslVerify` of the
//!   git configuration are read and given to the HTTP client, which gitoxide's `reqwest` back end
//!   does not do by itself.
//! - **Repositories it cannot read completely** (refs in the `reftable` format, partial clones)
//!   are refused with [`GitError::NeedsGit`], never taken for "not found".

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gix::bstr::ByteSlice;
use gix::ObjectId;
use toa_core::fsck;

use crate::git::{is_hex_id, safe_arg, Addition, Git, GitError, ListEntry, Result};

/// The built-in git.
pub struct BuiltinGit {
    /// Timeout of calls that use the network.
    pub network_timeout: Duration,
    /// The schemes fetching may use, `:` between them: `https:ssh` (7.3). Tests of local fetching
    /// widen it; the tool never does.
    pub allow_protocol: String,
    /// Read only each repository's own configuration, not the user's or the system's (tests).
    pub isolated: bool,
}

impl Default for BuiltinGit {
    fn default() -> Self {
        Self::new()
    }
}

/// An error with its causes, as one line.
fn chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        let t = c.to_string();
        if !out.contains(&t) {
            out.push_str(": ");
            out.push_str(&t);
        }
        cur = c.source();
    }
    out
}

fn failed(what: &str, e: impl std::fmt::Display) -> GitError {
    GitError::Failed { command: format!("{what} (built-in git)"), code: None, stderr: e.to_string() }
}

fn oid(s: &str) -> Result<ObjectId> {
    if !is_hex_id(s) {
        return Err(GitError::BadArgument(format!("{s:?} is not an object id")));
    }
    ObjectId::from_hex(s.as_bytes()).map_err(|e| GitError::BadArgument(e.to_string()))
}

/// The settings of a git configuration file that matter here, read without gitoxide: whether
/// refs are kept as `reftable`, and whether the repository is a partial clone.
fn unreadable_kind(git_dir: &Path) -> Option<&'static str> {
    if git_dir.join("reftable").is_dir() {
        return Some("its refs are kept in the reftable format");
    }
    let mut dirs = vec![git_dir.to_path_buf()];
    if let Ok(common) = std::fs::read_to_string(git_dir.join("commondir")) {
        let c = PathBuf::from(common.trim());
        dirs.push(if c.is_absolute() { c } else { git_dir.join(c) });
    }
    for d in dirs {
        if d.join("reftable").is_dir() {
            return Some("its refs are kept in the reftable format");
        }
        let Ok(text) = std::fs::read_to_string(d.join("config")) else { continue };
        let mut section = String::new();
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                section = l
                    .trim_matches(|c| c == '[' || c == ']')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().trim_matches('"').to_ascii_lowercase());
            match (section.as_str(), k.as_str()) {
                ("extensions", "refstorage") if v == "reftable" => {
                    return Some("its refs are kept in the reftable format")
                }
                ("extensions", "partialclone") if !v.is_empty() => return Some("it is a partial clone"),
                ("remote", "promisor") if v == "true" => return Some("it is a partial clone"),
                _ => {}
            }
        }
    }
    None
}

/// A glob of git pathspecs as `Git::additions` uses them: `*` matches anything, `/` too.
fn glob(p: &[u8], s: &[u8]) -> bool {
    match (p.first(), s.first()) {
        (None, None) => true,
        (Some(b'*'), _) => glob(&p[1..], s) || (!s.is_empty() && glob(p, &s[1..])),
        (Some(b'?'), Some(_)) => glob(&p[1..], &s[1..]),
        (Some(a), Some(b)) if a == b => glob(&p[1..], &s[1..]),
        _ => false,
    }
}

static QUARANTINE: AtomicU64 = AtomicU64::new(0);

impl BuiltinGit {
    pub fn new() -> Self {
        BuiltinGit { network_timeout: Duration::from_secs(300), allow_protocol: "https:ssh".into(), isolated: false }
    }

    fn open_options(&self) -> gix::open::Options {
        if self.isolated {
            gix::open::Options::isolated()
        } else {
            gix::open::Options::default()
        }
    }

    /// The repository that holds `dir`, refused when gitoxide cannot read it completely.
    fn open(&self, dir: &Path) -> Result<gix::Repository> {
        let (path, _) = gix::discover::upwards(dir).map_err(|e| failed("open", e))?;
        let (git_dir, _) = path.into_repository_and_work_tree_directories();
        let git_dir = if git_dir.is_absolute() { git_dir } else { std::path::absolute(&git_dir)? };
        if let Some(why) = unreadable_kind(&git_dir) {
            return Err(GitError::NeedsGit(format!("{}: {why}", git_dir.display())));
        }
        let isolated = self.isolated;
        gix::ThreadSafeRepository::discover_opts(dir, Default::default(), move |o, _| {
            if isolated {
                gix::open::Options::isolated()
            } else {
                o
            }
        })
        .map(|r| r.to_thread_local())
        .map_err(|e| failed("open", e))
    }

    fn raw(&self, repo: &gix::Repository, id: ObjectId) -> Result<Option<(gix::object::Kind, Vec<u8>)>> {
        match repo.try_find_object(id) {
            Ok(Some(o)) => Ok(Some((o.kind, o.data.clone()))),
            Ok(None) => Ok(None),
            Err(e) => Err(failed("read", e)),
        }
    }

    fn commit(&self, repo: &gix::Repository, id: ObjectId) -> Result<Option<fsck::Commit>> {
        match self.raw(repo, id)? {
            Some((gix::object::Kind::Commit, data)) => fsck::check_commit(&data, 2 * id.as_slice().len())
                .map(Some)
                .map_err(|p| GitError::Unreadable(format!("commit {id}: {}", p.join("; ")))),
            _ => Ok(None),
        }
    }

    /// An object peeled through tags to one of `kind`.
    fn peel(&self, repo: &gix::Repository, mut id: ObjectId, kind: gix::object::Kind) -> Result<Option<ObjectId>> {
        for _ in 0..16 {
            match self.raw(repo, id)? {
                Some((k, _)) if k == kind => return Ok(Some(id)),
                Some((gix::object::Kind::Tag, data)) => {
                    let Ok((target, _)) = fsck::check_tag(&data, 2 * id.as_slice().len()) else { return Ok(None) };
                    id = oid(&target)?;
                }
                Some((gix::object::Kind::Commit, data)) if kind == gix::object::Kind::Tree => {
                    let Ok(c) = fsck::check_commit(&data, 2 * id.as_slice().len()) else { return Ok(None) };
                    id = oid(&c.tree)?;
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    /// A ref by its full name, followed through symbolic refs, unpeeled.
    fn reference(&self, repo: &gix::Repository, name: &str) -> Result<Option<ObjectId>> {
        match repo.try_find_reference(name) {
            Ok(Some(mut r)) => Ok(r.follow_to_object().ok().map(|i| i.detach())),
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    /// A revision as `git rev-parse` reads the forms `toa` uses: an object id, `HEAD`, a ref by
    /// its full or short name, followed by `~N`, `^`, `^N` and `^{commit}`.
    fn revision(&self, repo: &gix::Repository, rev: &str) -> Result<Option<ObjectId>> {
        let base_end = rev.find(['~', '^']).unwrap_or(rev.len());
        let (base, mut ops) = (&rev[..base_end], &rev[base_end..]);
        let mut id = if is_hex_id(base) {
            let i = oid(base)?;
            if repo.has_object(i) {
                i
            } else {
                return Ok(None);
            }
        } else {
            let mut found = None;
            let candidates = if base == "HEAD" {
                vec!["HEAD".to_string()]
            } else {
                vec![
                    base.to_string(),
                    format!("refs/{base}"),
                    format!("refs/tags/{base}"),
                    format!("refs/heads/{base}"),
                    format!("refs/remotes/{base}"),
                    format!("refs/remotes/{base}/HEAD"),
                ]
            };
            for c in candidates {
                if c != "HEAD" && !c.starts_with("refs/") {
                    continue;
                }
                if let Some(i) = self.reference(repo, &c)? {
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => i,
                None => return Ok(None),
            }
        };
        while !ops.is_empty() {
            if let Some(rest) = ops.strip_prefix("^{commit}") {
                let Some(c) = self.peel(repo, id, gix::object::Kind::Commit)? else { return Ok(None) };
                id = c;
                ops = rest;
                continue;
            }
            let op = ops.as_bytes()[0];
            let digits = ops[1..].bytes().take_while(u8::is_ascii_digit).count();
            let n: usize =
                if digits == 0 { 1 } else { ops[1..=digits].parse().map_err(|_| GitError::BadArgument(rev.into()))? };
            ops = &ops[1 + digits..];
            let Some(c) = self.peel(repo, id, gix::object::Kind::Commit)? else { return Ok(None) };
            id = c;
            if op == b'~' {
                for _ in 0..n {
                    let Some(c) = self.commit(repo, id)? else { return Ok(None) };
                    let Some(p) = c.parents.first() else { return Ok(None) };
                    id = oid(p)?;
                }
            } else if n > 0 {
                let Some(c) = self.commit(repo, id)? else { return Ok(None) };
                let Some(p) = c.parents.get(n - 1) else { return Ok(None) };
                id = oid(p)?;
            }
        }
        Ok(Some(id))
    }

    fn tree_entries(&self, repo: &gix::Repository, id: ObjectId) -> Result<Vec<(u32, Vec<u8>, ObjectId)>> {
        let Some((gix::object::Kind::Tree, data)) = self.raw(repo, id)? else {
            return Err(failed("ls-tree", format!("not a tree: {id}")));
        };
        let len = id.as_slice().len();
        let entries = fsck::parse_tree(&data, len).ok_or_else(|| failed("ls-tree", format!("corrupt tree {id}")))?;
        let mut out = Vec::new();
        for e in entries {
            if e.name.is_empty() {
                return Err(failed("ls-tree", format!("empty filename in tree entry of {id}")));
            }
            out.push((e.canonical_mode(), e.name.to_vec(), ObjectId::from_bytes_or_panic(e.id)));
        }
        Ok(out)
    }

    fn walk(
        &self,
        repo: &gix::Repository,
        tree: ObjectId,
        prefix: &[u8],
        max: usize,
        out: &mut Vec<ListEntry>,
    ) -> Result<()> {
        for (mode, name, id) in self.tree_entries(repo, tree)? {
            if out.len() >= max {
                return Err(GitError::TooLarge(format!("the listing has more than {max} entries")));
            }
            let mut path = prefix.to_vec();
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(&name);
            let kind = match mode {
                0o040000 => "tree",
                0o160000 => "commit",
                _ => "blob",
            };
            let size = if kind == "blob" {
                let h = repo.try_find_header(id).map_err(|e| failed("ls-tree", e))?;
                Some(h.ok_or_else(|| failed("ls-tree", format!("could not get object info about {id}")))?.size())
            } else {
                None
            };
            let (p, utf8) = match std::str::from_utf8(&path) {
                Ok(s) => (s.to_string(), true),
                Err(_) => (String::from_utf8_lossy(&path).into_owned(), false),
            };
            out.push(ListEntry {
                mode: format!("{mode:06o}"),
                kind: kind.into(),
                id: id.to_string(),
                size,
                path: p,
                utf8,
            });
            if kind == "tree" {
                self.walk(repo, id, &path, max, out)?;
            }
        }
        Ok(())
    }

    /// The paths under a tree matching the patterns, with the trees they are in, for comparing.
    fn added_paths(
        &self,
        repo: &gix::Repository,
        new: ObjectId,
        old: Option<ObjectId>,
        prefix: &[u8],
        patterns: &[&str],
        out: &mut Vec<String>,
    ) -> Result<()> {
        if Some(new) == old {
            return Ok(());
        }
        let old_entries: BTreeMap<Vec<u8>, (u32, ObjectId)> = match old {
            Some(o) => self.tree_entries(repo, o)?.into_iter().map(|(m, n, i)| (n, (m, i))).collect(),
            None => BTreeMap::new(),
        };
        for (mode, name, id) in self.tree_entries(repo, new)? {
            let mut path = prefix.to_vec();
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(&name);
            let before = old_entries.get(&name);
            if mode == 0o040000 {
                let old_tree = before.filter(|(m, _)| *m == 0o040000).map(|(_, i)| *i);
                self.added_paths(repo, id, old_tree, &path, patterns, out)?;
            } else if mode != 0o160000
                && before.is_none_or(|(m, _)| *m == 0o040000 || *m == 0o160000)
                && patterns.iter().any(|p| glob(p.as_bytes(), &path))
            {
                out.push(String::from_utf8_lossy(&path).into_owned());
            }
        }
        Ok(())
    }

    fn scheme_allowed(&self, address: &str) -> bool {
        let scheme = match address.split_once("://") {
            Some((s, _)) => s.to_ascii_lowercase(),
            None if address.contains(':') && !Path::new(address).is_absolute() => "ssh".into(),
            None => "file".into(),
        };
        self.allow_protocol.split(':').any(|a| a == scheme)
    }
}

/// A setting of the user's that the HTTP client could not take.
#[derive(Debug)]
struct Setting(String);

impl std::fmt::Display for Setting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Setting {}

/// The user's git settings for HTTP, read from their configuration (9.4: gitoxide's `reqwest`
/// back end does not read them itself).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpSettings {
    /// `http.proxy`: `Some("")` turns proxies off.
    pub proxy: Option<String>,
    /// `http.sslCAInfo`: the only certificates trusted, as with git built on OpenSSL.
    pub ca_info: Option<PathBuf>,
    /// `http.sslVerify`.
    pub verify: bool,
}

impl HttpSettings {
    pub fn read(repo: &gix::Repository) -> Self {
        let c = repo.config_snapshot();
        HttpSettings {
            proxy: c.string("http.proxy").map(|s| s.to_str_lossy().into_owned()),
            ca_info: c.string("http.sslCAInfo").map(|s| PathBuf::from(s.to_str_lossy().into_owned())),
            verify: c.boolean("http.sslVerify").unwrap_or(true),
        }
    }

    /// The options of gitoxide's HTTP transport: no redirects (7.3), and the client configured
    /// with these settings.
    fn transport_options(&self, timeout: Duration) -> gix::protocol::transport::client::blocking_io::http::Options {
        use gix::protocol::transport::client::blocking_io::http;
        let settings = self.clone();
        let configure =
            move |mut b: reqwest::blocking::ClientBuilder| -> gix::error::Result<reqwest::blocking::ClientBuilder> {
                use gix::error::ErrorExt;
                let err = |m: String| Setting(m).raise();
                match settings.proxy.as_deref() {
                    Some("") => b = b.no_proxy(),
                    Some(p) => b = b.proxy(reqwest::Proxy::all(p).map_err(|e| err(format!("http.proxy {p}: {e}")))?),
                    None => {}
                }
                if let Some(ca) = &settings.ca_info {
                    let pem = std::fs::read(ca).map_err(|e| err(format!("http.sslCAInfo {}: {e}", ca.display())))?;
                    let certs = reqwest::Certificate::from_pem_bundle(&pem)
                        .map_err(|e| err(format!("http.sslCAInfo {}: {e}", ca.display())))?;
                    b = b.tls_certs_only(certs);
                }
                if !settings.verify {
                    b = b.danger_accept_invalid_certs(true);
                }
                Ok(b.timeout(timeout))
            };
        let backend = http::reqwest::Options { configure_client: Some(Box::new(configure)), configure_request: None };
        http::Options {
            follow_redirects: http::options::FollowRedirects::None,
            connect_timeout: Some(Duration::from_secs(60)),
            backend: Some(Arc::new(std::sync::Mutex::new(backend))),
            ..Default::default()
        }
    }
}

impl Git for BuiltinGit {
    fn toplevel(&self, dir: &Path) -> Result<Option<PathBuf>> {
        let Ok((path, _)) = gix::discover::upwards(dir) else { return Ok(None) };
        let (git_dir, work) = path.into_repository_and_work_tree_directories();
        if let Some(why) = unreadable_kind(&std::path::absolute(&git_dir)?) {
            return Err(GitError::NeedsGit(why.into()));
        }
        Ok(match work {
            Some(w) => Some(std::path::absolute(w)?),
            None => None,
        })
    }

    fn git_dir(&self, dir: &Path) -> Result<PathBuf> {
        let (path, _) = gix::discover::upwards(dir).map_err(|e| failed("rev-parse --absolute-git-dir", e))?;
        let (git_dir, _) = path.into_repository_and_work_tree_directories();
        Ok(std::path::absolute(git_dir)?)
    }

    fn object_format(&self, repo: &Path) -> Result<String> {
        let r = self.open(repo)?;
        Ok(match r.object_hash() {
            gix::hash::Kind::Sha1 => "sha1".into(),
            #[allow(unreachable_patterns)]
            other => other.to_string(),
        })
    }

    fn resolve_commit(&self, repo: &Path, rev: &str) -> Result<Option<String>> {
        let rev = safe_arg(rev)?;
        let r = self.open(repo)?;
        let Some(id) = self.revision(&r, rev)? else { return Ok(None) };
        Ok(self.peel(&r, id, gix::object::Kind::Commit)?.map(|i| i.to_string()))
    }

    fn resolve_ref(&self, repo: &Path, refname: &str) -> Result<Option<String>> {
        if !refname.starts_with("refs/") {
            return Err(GitError::BadArgument(format!("{refname:?}")));
        }
        let name = safe_arg(refname)?;
        let r = self.open(repo)?;
        Ok(self.reference(&r, name)?.map(|i| i.to_string()))
    }

    fn object_at(&self, repo: &Path, commit: &str, path: &str) -> Result<Option<String>> {
        if !is_hex_id(commit) {
            return Err(GitError::BadArgument(format!("{commit:?} is not a commit id")));
        }
        if path.is_empty() || path.chars().any(char::is_control) {
            return Err(GitError::BadArgument(format!("{path:?}")));
        }
        let r = self.open(repo)?;
        let Some(mut id) = self.peel(&r, oid(commit)?, gix::object::Kind::Tree)? else { return Ok(None) };
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let Ok(entries) = self.tree_entries(&r, id) else { return Ok(None) };
            match entries.into_iter().find(|(_, n, _)| n == part.as_bytes()) {
                Some((_, _, i)) => id = i,
                None => return Ok(None),
            }
        }
        Ok(Some(id.to_string()))
    }

    fn has_commits(&self, repo: &Path) -> Result<bool> {
        let r = self.open(repo)?;
        let refs = r.references().map_err(|e| failed("for-each-ref", e))?;
        let all = refs.all().map_err(|e| failed("for-each-ref", e))?;
        for mut x in all.flatten() {
            if x.follow_to_object().is_ok() {
                return Ok(true);
            }
        }
        Ok(self.reference(&r, "HEAD")?.is_some())
    }

    fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
        let (a, d) = (safe_arg(ancestor)?, safe_arg(descendant)?);
        let r = self.open(repo)?;
        let (Some(a), Some(d)) = (self.revision(&r, a)?, self.revision(&r, d)?) else {
            return Err(failed("merge-base --is-ancestor", "not a valid commit name"));
        };
        let (Some(a), Some(d)) =
            (self.peel(&r, a, gix::object::Kind::Commit)?, self.peel(&r, d, gix::object::Kind::Commit)?)
        else {
            return Err(failed("merge-base --is-ancestor", "not a commit"));
        };
        let mut seen = BTreeSet::new();
        let mut todo = vec![d];
        while let Some(c) = todo.pop() {
            if c == a {
                return Ok(true);
            }
            if !seen.insert(c) {
                continue;
            }
            if let Some(commit) = self.commit(&r, c)? {
                for p in commit.parents {
                    todo.push(oid(&p)?);
                }
            }
        }
        Ok(false)
    }

    fn list_tree(&self, repo: &Path, tree_ish: &str, max_entries: usize) -> Result<Vec<ListEntry>> {
        let t = safe_arg(tree_ish)?;
        let r = self.open(repo)?;
        let Some(id) = self.revision(&r, t)? else { return Err(failed("ls-tree", format!("not a tree object: {t}"))) };
        let Some(tree) = self.peel(&r, id, gix::object::Kind::Tree)? else {
            return Err(failed("ls-tree", format!("not a tree object: {t}")));
        };
        let mut out = Vec::new();
        self.walk(&r, tree, b"", max_entries, &mut out)?;
        Ok(out)
    }

    fn object_size(&self, repo: &Path, id: &str) -> Result<Option<u64>> {
        let i = oid(id)?;
        let r = self.open(repo)?;
        Ok(r.try_find_header(i).map_err(|e| failed("cat-file", e))?.map(|h| h.size()))
    }

    fn read_blob(&self, repo: &Path, id: &str, max: u64) -> Result<Vec<u8>> {
        let i = oid(id)?;
        let r = self.open(repo)?;
        let Some(h) = r.try_find_header(i).map_err(|e| failed("cat-file", e))? else {
            return Err(GitError::Failed { command: format!("cat-file {id}"), code: None, stderr: "missing".into() });
        };
        if h.kind() != gix::object::Kind::Blob {
            return Err(GitError::Unreadable(format!("{id} is a {}, not a blob", h.kind())));
        }
        if h.size() > max {
            return Err(GitError::TooLarge(format!("blob {id} has {} bytes, more than {max}", h.size())));
        }
        Ok(self.raw(&r, i)?.map(|(_, d)| d).unwrap_or_default())
    }

    fn tree_of_dir(&self, _root: &Path, _rel: &str) -> Result<String> {
        Err(GitError::NeedsGit("the tree the user's git would store for a directory (7.1, step 3.4)".into()))
    }

    fn remote_url(&self, repo: &Path, remote: &str) -> Result<Option<String>> {
        let rm = safe_arg(remote)?;
        if !rm.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(GitError::BadArgument(format!("{remote:?}")));
        }
        let r = self.open(repo)?;
        let c = r.config_snapshot();
        Ok(c.string(format!("remote.{rm}.url").as_str()).map(|s| s.to_str_lossy().into_owned()))
    }

    fn commit_date(&self, repo: &Path, commit: &str) -> Result<String> {
        let c = safe_arg(commit)?;
        let r = self.open(repo)?;
        let id = self.revision(&r, c)?.ok_or_else(|| failed("show", format!("unknown revision {c}")))?;
        let id = self
            .peel(&r, id, gix::object::Kind::Commit)?
            .ok_or_else(|| failed("show", format!("not a commit: {c}")))?;
        let commit = self.commit(&r, id)?.ok_or_else(|| failed("show", format!("not a commit: {c}")))?;
        fsck::iso_date(&commit.committer).ok_or_else(|| GitError::Unreadable(format!("the date of {c}")))
    }

    fn additions(&self, repo: &Path, commit: &str, since: Option<&str>, patterns: &[&str]) -> Result<Vec<Addition>> {
        let r = self.open(repo)?;
        let resolve = |rev: &str| -> Result<ObjectId> {
            let rev = safe_arg(rev)?;
            let id = self.revision(&r, rev)?.ok_or_else(|| failed("log", format!("unknown revision {rev}")))?;
            self.peel(&r, id, gix::object::Kind::Commit)?.ok_or_else(|| failed("log", format!("not a commit: {rev}")))
        };
        let head = resolve(commit)?;
        // `since..commit`: leave out what `since` reaches
        let mut excluded: BTreeSet<ObjectId> = BTreeSet::new();
        if let Some(s) = since {
            let mut todo = vec![resolve(s)?];
            while let Some(c) = todo.pop() {
                if excluded.insert(c) {
                    if let Some(cm) = self.commit(&r, c)? {
                        for p in cm.parents {
                            todo.push(oid(&p)?);
                        }
                    }
                }
            }
        }
        let mut chain = Vec::new();
        let mut cur = Some(head);
        while let Some(c) = cur {
            if excluded.contains(&c) {
                break;
            }
            let Some(cm) = self.commit(&r, c)? else { break };
            cur = match cm.parents.first() {
                Some(p) => Some(oid(p)?),
                None => None,
            };
            chain.push((c, cm));
        }
        let mut out = Vec::new();
        for (c, cm) in chain.into_iter().rev() {
            let parent_tree = match cm.parents.first() {
                Some(p) => self.commit(&r, oid(p)?)?.map(|pc| oid(&pc.tree)).transpose()?,
                None => None,
            };
            let mut paths = Vec::new();
            self.added_paths(&r, oid(&cm.tree)?, parent_tree, b"", patterns, &mut paths)?;
            if !paths.is_empty() {
                let date = fsck::iso_date(&cm.committer).unwrap_or_default();
                out.push(Addition { commit: c.to_string(), date, paths });
            }
        }
        Ok(out)
    }

    fn commit_paths(&self, _root: &Path, _paths: &[String], _message: &str, _sign: bool) -> Result<String> {
        Err(GitError::NeedsGit("committing (toa publish --commit)".into()))
    }

    fn signs_commits(&self, repo: &Path) -> Result<bool> {
        let r = self.open(repo)?;
        Ok(r.config_snapshot().boolean("commit.gpgsign").unwrap_or(false))
    }

    fn commit_signature(&self, repo: &Path, commit: &str) -> Result<Option<crate::git::SignatureFormat>> {
        let c = safe_arg(commit)?;
        let r = self.open(repo)?;
        let id = self.revision(&r, c)?.ok_or_else(|| failed("cat-file", format!("unknown revision {c}")))?;
        match self.raw(&r, id)? {
            Some((gix::object::Kind::Commit, data)) => Ok(crate::git::signature_of_commit(&data)),
            _ => Err(failed("cat-file", format!("not a commit: {c}"))),
        }
    }

    fn init_bare(&self, dir: &Path) -> Result<()> {
        gix::init_bare(dir).map(|_| ()).map_err(|e| failed("init --bare", e))
    }

    fn update_ref(&self, repo: &Path, refname: &str, id: &str) -> Result<()> {
        if !refname.starts_with("refs/toa/") || !is_hex_id(id) {
            return Err(GitError::BadArgument(format!("{refname} {id}")));
        }
        let r = self.open(repo)?;
        r.reference(refname, oid(id)?, gix::refs::transaction::PreviousValue::Any, "toa")
            .map(|_| ())
            .map_err(|e| failed("update-ref", e))
    }

    fn fetch(&self, repo: &Path, address: &str, wants: &[&str], shallow: bool) -> Result<()> {
        if address.is_empty() || address.starts_with('-') {
            return Err(GitError::BadArgument(format!("{address:?}")));
        }
        if !self.scheme_allowed(address) {
            return Err(failed("fetch", format!("the protocol of {address} is not allowed ({})", self.allow_protocol)));
        }
        let mut specs = Vec::new();
        let mut branch_refs = Vec::new();
        for w in wants {
            if is_hex_id(w) {
                specs.push(w.to_string());
            } else if let Some((b, dst)) = w.strip_prefix("refs/heads/").and_then(|x| x.split_once(':')) {
                let dst_ok = dst.strip_prefix("refs/toa/").is_some_and(|d| {
                    !d.is_empty() && d.split('/').all(|p| toa_core::tree::component_problem(p, false).is_none())
                });
                if toa_core::source::branch_problem(b).is_some() || !dst_ok {
                    return Err(GitError::BadArgument(format!("{w:?}")));
                }
                specs.push(format!("+refs/heads/{b}:{dst}"));
                branch_refs.push(dst.to_string());
            } else {
                return Err(GitError::BadArgument(format!("{w:?}")));
            }
        }
        let target = self.open(repo)?;
        let git_dir = std::path::absolute(target.git_dir())?;
        let objects = std::path::absolute(target.objects.store_ref().path())?;
        let n = QUARANTINE.fetch_add(1, Ordering::SeqCst);
        let q = git_dir.join(format!("toa-incoming-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&q);
        let result = self.fetch_into(&target, &objects, &q, address, &specs, &branch_refs, wants, shallow);
        let _ = std::fs::remove_dir_all(&q);
        result
    }
}

impl BuiltinGit {
    #[allow(clippy::too_many_arguments)]
    fn fetch_into(
        &self,
        target: &gix::Repository,
        objects: &Path,
        q: &Path,
        address: &str,
        specs: &[String],
        branch_refs: &[String],
        wants: &[&str],
        shallow: bool,
    ) -> Result<()> {
        // 1. receive into the quarantine; it sees the objects the repository has
        gix::init_bare(q).map_err(|e| failed("fetch", e))?;
        std::fs::create_dir_all(q.join("objects/info"))?;
        std::fs::write(
            q.join("objects/info/alternates"),
            format!("{}\n", objects.to_string_lossy().replace('\\', "/")),
        )?;
        let qrepo = gix::open_opts(q, self.open_options()).map_err(|e| failed("fetch", e))?;
        let settings = HttpSettings::read(if self.isolated { &qrepo } else { target });
        let remote = qrepo
            .remote_at(address)
            .map_err(|e| failed("fetch", e))?
            .with_fetch_tags(gix::remote::fetch::Tags::None)
            .with_refspecs(specs.iter().map(String::as_str), gix::remote::Direction::Fetch)
            .map_err(|e| failed("fetch", e))?;
        let interrupt = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let flag = interrupt.clone();
        let timeout = self.network_timeout;
        let timer = std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(timeout) {
                flag.store(true, Ordering::SeqCst);
            }
        });
        let received = (|| -> std::result::Result<(), String> {
            let conn = remote.connect(gix::remote::Direction::Fetch).map_err(|e| chain(&e))?;
            let conn = if address.starts_with("http://") || address.starts_with("https://") {
                conn.with_transport_options(Box::new(settings.transport_options(timeout)))
            } else {
                conn
            };
            let mut prepare = conn.prepare_fetch(gix::progress::Discard, Default::default()).map_err(|e| chain(&e))?;
            if shallow {
                prepare = prepare.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(1.try_into().expect("one")));
            }
            prepare.receive(gix::progress::Discard, &interrupt).map(|_| ()).map_err(|e| chain(&e))
        })();
        let _ = done_tx.send(());
        let _ = timer.join();
        if interrupt.load(Ordering::SeqCst) {
            return Err(GitError::TimedOut(format!("fetch {address}")));
        }
        received.map_err(|e| failed(&format!("fetch {address}"), e))?;

        // 2. the check of what was received (7.3 step 4): everything the wants reach, down to
        // what the repository had already
        let qrepo = gix::open_opts(q, self.open_options()).map_err(|e| failed("fetch", e))?;
        let mut todo: Vec<ObjectId> = Vec::new();
        for w in wants {
            if is_hex_id(w) {
                todo.push(oid(w)?);
            }
        }
        for b in branch_refs {
            match self.reference(&qrepo, b)? {
                Some(i) => todo.push(i),
                None => return Err(failed(&format!("fetch {address}"), format!("{b} was not received"))),
            }
        }
        let len = target.object_hash().len_in_bytes();
        let mut seen = BTreeSet::new();
        let mut problems = Vec::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id) || target.has_object(id) {
                continue;
            }
            let Some((kind, data)) = self.raw(&qrepo, id)? else {
                problems.push(format!("missing object {id}"));
                continue;
            };
            match kind {
                gix::object::Kind::Tree => {
                    for p in fsck::check_tree(&data, len) {
                        problems.push(format!("{id}: {p}"));
                    }
                    if let Some(entries) = fsck::parse_tree(&data, len) {
                        for e in entries.iter().filter(|e| !e.is_gitlink()) {
                            todo.push(ObjectId::from_bytes_or_panic(e.id));
                        }
                    }
                }
                gix::object::Kind::Commit => match fsck::check_commit(&data, 2 * len) {
                    Ok(c) => {
                        todo.push(oid(&c.tree)?);
                        if !shallow {
                            for p in c.parents {
                                todo.push(oid(&p)?);
                            }
                        }
                    }
                    Err(ps) => problems.extend(ps.into_iter().map(|p| format!("{id}: {p}"))),
                },
                gix::object::Kind::Tag => match fsck::check_tag(&data, 2 * len) {
                    Ok((o, _)) => todo.push(oid(&o)?),
                    Err(ps) => problems.extend(ps.into_iter().map(|p| format!("{id}: {p}"))),
                },
                gix::object::Kind::Blob => {}
            }
        }
        if !problems.is_empty() {
            problems.truncate(20);
            return Err(GitError::Failed {
                command: format!("fetch {address} (built-in git)"),
                code: None,
                stderr: format!("fatal: the objects received fail the check of 7.3 step 4: {}", problems.join("; ")),
            });
        }

        // 3. into the repository: packs and loose objects, the shallow boundary, the refs
        let qobjects = q.join("objects");
        for entry in std::fs::read_dir(&qobjects)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "info" || !entry.file_type()?.is_dir() {
                continue;
            }
            let dst_dir = objects.join(&name);
            std::fs::create_dir_all(&dst_dir)?;
            let mut files: Vec<_> = std::fs::read_dir(entry.path())?.flatten().collect();
            // a pack before its index, so that no index points to a missing pack
            files.sort_by_key(|f| f.file_name().to_string_lossy().ends_with(".idx"));
            for f in files {
                let dst = dst_dir.join(f.file_name());
                if !dst.exists() {
                    std::fs::rename(f.path(), &dst).or_else(|_| std::fs::copy(f.path(), &dst).map(|_| ()))?;
                }
            }
        }
        if let Ok(lines) = std::fs::read_to_string(q.join("shallow")) {
            let file = target.git_dir().join("shallow");
            let mut all: BTreeSet<String> =
                std::fs::read_to_string(&file).unwrap_or_default().lines().map(str::to_string).collect();
            all.extend(lines.lines().filter(|l| !l.is_empty()).map(str::to_string));
            let text: String = all.into_iter().map(|l| l + "\n").collect();
            std::fs::write(&file, text)?;
        }
        for b in branch_refs {
            if let Some(i) = self.reference(&qrepo, b)? {
                let t = gix::open_opts(target.git_dir(), self.open_options()).map_err(|e| failed("fetch", e))?;
                t.reference(b.as_str(), i, gix::refs::transaction::PreviousValue::Any, "toa fetch")
                    .map_err(|e| failed("update-ref", e))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pathspec_globs() {
        assert!(glob(b"*/toa.yaml", b"imp/a/b/imp_x/toa.yaml"));
        assert!(glob(b"*.notes/*", b"ip/a/ip_x.notes/1.md"));
        assert!(!glob(b"*/toa.yaml", b"toa.yaml"));
        assert!(glob(b"*.removed.json", b"a/b.removed.json"));
    }

    #[test]
    fn repositories_it_cannot_read() {
        let d = std::env::temp_dir().join(format!("toa-builtin-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("config"), "[core]\n\tbare = true\n[extensions]\n\trefStorage = reftable\n").unwrap();
        assert!(unreadable_kind(&d).unwrap().contains("reftable"));
        std::fs::write(d.join("config"), "[remote \"origin\"]\n\tpromisor = true\n").unwrap();
        assert!(unreadable_kind(&d).unwrap().contains("partial"));
        std::fs::write(d.join("config"), "[core]\n\tbare = true\n").unwrap();
        assert!(unreadable_kind(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
