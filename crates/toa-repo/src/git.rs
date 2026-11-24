//! Every call of git, behind one interface.
//!
//! [`Git`] is what the rest of the tool needs from git: refs, listings, blobs, the tree hash git
//! computes from a working directory, and fetching. [`SystemGit`] implements it by running the
//! system's `git` (approach A of `docs/lines/tools.md` 9.1); a compiled-in implementation can
//! stand behind the same interface later.
//!
//! The rules of the standard for calling git (7.3, chapter 11) are kept here, once:
//! - every argument is checked first, and placed after `--` or `--end-of-options`, or confirmed
//!   not to start with a hyphen;
//! - git never prompts (`GIT_TERMINAL_PROMPT=0`), and the variables that would point git at
//!   another repository are removed;
//! - every call has a timeout, and when it runs out git and the processes it started are ended;
//! - fetching allows only `https` and `ssh`, has git check what it receives, does not follow
//!   redirects and leaves no remote behind;
//! - listings are counted while they are read, and reading stops at the limit.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// The oldest git this tool works with.
pub const MIN_VERSION: (u32, u32) = (2, 30);

/// How long a commit that is signed may take: a signing program waiting for a passphrase that
/// nobody types is ended then.
pub const SIGNING_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a call of git did not give a result.
#[derive(Debug)]
pub enum GitError {
    /// No `git` could be started.
    Missing(String),
    /// The installed git is older than [`MIN_VERSION`].
    TooOld(String),
    /// An argument was rejected before git was called (chapter 11).
    BadArgument(String),
    /// Git ran and failed.
    Failed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    /// The timeout ran out; git and its processes were ended.
    TimedOut(String),
    /// A listing or a blob passed the limit; reading stopped.
    TooLarge(String),
    /// Git printed something this tool cannot read.
    Unreadable(String),
    /// The built-in git cannot do this, or cannot read this repository completely: the system
    /// git is needed (docs/lines/tools.md 9.4).
    NeedsGit(String),
    Io(std::io::Error),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Missing(m) => write!(f, "git is not installed or cannot be started: {m}"),
            GitError::TooOld(v) => {
                write!(f, "git {v} is too old; this tool needs git {}.{} or later", MIN_VERSION.0, MIN_VERSION.1)
            }
            GitError::BadArgument(m) => write!(f, "not handed to git: {m}"),
            GitError::Failed { command, code: None, stderr } if command.ends_with("(built-in git)") => {
                let first = stderr.trim().lines().last().unwrap_or("");
                write!(f, "'git {command}' failed: {first}")
            }
            GitError::Failed { command, code, stderr } => {
                let code = code.map_or_else(|| "a signal".to_string(), |c| c.to_string());
                let first = stderr.trim().lines().last().unwrap_or("");
                write!(f, "'git {command}' failed ({code}): {first}")
            }
            GitError::TimedOut(c) => write!(f, "'git {c}' took too long and was ended"),
            GitError::TooLarge(m) => write!(f, "too large: {m}"),
            GitError::Unreadable(m) => write!(f, "cannot read git's output: {m}"),
            GitError::NeedsGit(m) => write!(
                f,
                "this needs git installed ({}.{} or later); the built-in git cannot do it: {m}",
                MIN_VERSION.0, MIN_VERSION.1
            ),
            GitError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GitError {}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        GitError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, GitError>;

/// One entry of a recursive listing (`git ls-tree -r -t -l`).
pub use toa_core::library::Listed as ListEntry;

/// What the tool needs from git. Paths of repositories are directories inside them (a working
/// directory, or a bare repository).
pub trait Git {
    /// The root of the working directory that holds `dir`, or `None` outside a repository.
    fn toplevel(&self, dir: &Path) -> Result<Option<PathBuf>>;
    /// The git directory of the repository that holds `dir`.
    fn git_dir(&self, dir: &Path) -> Result<PathBuf>;
    /// `sha1` or `sha256` (6.1).
    fn object_format(&self, repo: &Path) -> Result<String>;
    /// The commit a revision names, or `None` when there is none.
    fn resolve_commit(&self, repo: &Path, rev: &str) -> Result<Option<String>>;
    /// The object a ref points to, or `None`.
    fn resolve_ref(&self, repo: &Path, refname: &str) -> Result<Option<String>>;
    /// The object at `path` in `commit` (`git rev-parse commit:path`), or `None`.
    fn object_at(&self, repo: &Path, commit: &str, path: &str) -> Result<Option<String>>;
    /// Whether the repository has any commit under `refs/`.
    fn has_commits(&self, repo: &Path) -> Result<bool>;
    /// Whether `ancestor` is an ancestor of (or the same as) `descendant`.
    fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool>;
    /// The recursive listing of a tree or commit, trees included, with blob sizes. Reading stops
    /// with [`GitError::TooLarge`] beyond `max_entries`.
    fn list_tree(&self, repo: &Path, tree_ish: &str, max_entries: usize) -> Result<Vec<ListEntry>>;
    /// The size of an object, or `None` when the repository does not have it.
    fn object_size(&self, repo: &Path, id: &str) -> Result<Option<u64>>;
    /// The content of a blob. Git's header gives the size before the content is read; a blob
    /// larger than `max` is skipped with [`GitError::TooLarge`].
    fn read_blob(&self, repo: &Path, id: &str, max: u64) -> Result<Vec<u8>>;
    /// The tree hash git gives a directory of a working directory with the actual settings of
    /// that repository: ignore rules, attributes and file modes (7.1, step 3.4). `rel` is the
    /// path from the root of the working directory, `/` between levels.
    fn tree_of_dir(&self, root: &Path, rel: &str) -> Result<String>;
    /// The address of a remote, or `None`.
    fn remote_url(&self, repo: &Path, remote: &str) -> Result<Option<String>>;
    /// The committer date of a commit, ISO 8601 with the committer's offset (`%cI`).
    fn commit_date(&self, repo: &Path, commit: &str) -> Result<String>;
    /// Along the first parents of `commit` (after `since`, when given), oldest first: the commits
    /// that added files matching `patterns` (git pathspecs, where `*` matches `/` too), with
    /// each commit's id, committer date (`%cI`) and the paths it added. Merges count by their
    /// difference to the first parent; renames count as additions.
    fn additions(&self, repo: &Path, commit: &str, since: Option<&str>, patterns: &[&str]) -> Result<Vec<Addition>>;
    /// Stage these paths of a working directory (relative, `/` between levels; removed files
    /// too) and commit only them with a message; the new commit. Hooks run as the user set them.
    ///
    /// The commit is signed as the user's git settings say (`commit.gpgsign`, `gpg.format`,
    /// `user.signingkey`; 6.3); with `sign`, it must be signed (`git commit -S`), and fails when it
    /// cannot be.
    fn commit_paths(&self, root: &Path, paths: &[String], message: &str, sign: bool) -> Result<String>;
    /// Whether commits are signed by the user's settings (`commit.gpgsign`) in this repository.
    fn signs_commits(&self, repo: &Path) -> Result<bool>;
    /// The kind of signature a commit carries, from its `gpgsig` header, or `None` when it has
    /// none. The signature is not verified.
    fn commit_signature(&self, repo: &Path, commit: &str) -> Result<Option<SignatureFormat>>;
    /// Create a bare repository with the SHA-1 object format (6.1).
    fn init_bare(&self, dir: &Path) -> Result<()>;
    /// Point a ref at an object, so that git does not clean it away (7.3, "keeping them").
    fn update_ref(&self, repo: &Path, refname: &str, id: &str) -> Result<()>;
    /// Fetch objects from an address into a repository by the fixed steps of 7.3: `wants` are
    /// object ids, or `refs/heads/<branch>:refs/toa/<name>` to fetch the latest commit of a branch
    /// into a ref of the tool's own. Nothing else is written; no remote is left behind.
    /// `shallow` fetches only the commits named, not their history.
    fn fetch(&self, repo: &Path, address: &str, wants: &[&str], shallow: bool) -> Result<()>;
}

/// The kind of a commit's signature, by the armour of its `gpgsig` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureFormat {
    Ssh,
    OpenPgp,
    X509,
    /// A signature of a kind git does not make by itself.
    Other,
}

impl SignatureFormat {
    /// As the index and `--json` write it.
    pub fn as_str(self) -> &'static str {
        match self {
            SignatureFormat::Ssh => "ssh",
            SignatureFormat::OpenPgp => "openpgp",
            SignatureFormat::X509 => "x509",
            SignatureFormat::Other => "other",
        }
    }
}

/// The signature of a raw commit object: the `gpgsig` header (or `gpgsig-sha256`) among the
/// headers before the first empty line. Not verified.
pub fn signature_of_commit(raw: &[u8]) -> Option<SignatureFormat> {
    for line in raw.split(|b| *b == b'\n') {
        if line.is_empty() {
            break; // the end of the headers
        }
        let rest = line.strip_prefix(b"gpgsig ").or_else(|| line.strip_prefix(b"gpgsig-sha256 "));
        if let Some(armour) = rest {
            let armour = String::from_utf8_lossy(armour);
            return Some(match armour.trim() {
                "-----BEGIN SSH SIGNATURE-----" => SignatureFormat::Ssh,
                "-----BEGIN PGP SIGNATURE-----" => SignatureFormat::OpenPgp,
                "-----BEGIN SIGNED MESSAGE-----" => SignatureFormat::X509,
                _ => SignatureFormat::Other,
            });
        }
    }
    None
}

/// A commit that added files (`Git::additions`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addition {
    pub commit: String,
    /// The committer date, ISO 8601 with the committer's offset.
    pub date: String,
    pub paths: Vec<String>,
}

/// Checks an argument for git that is not placed after `--`: not empty, not starting with a
/// hyphen, no whitespace or control characters (chapter 11).
pub fn safe_arg(s: &str) -> Result<&str> {
    if s.is_empty() || s.starts_with('-') || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(GitError::BadArgument(format!("{s:?}")));
    }
    Ok(s)
}

pub(crate) fn is_hex_id(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Which git `toa` uses (docs/lines/tools.md 9.1, approach B).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Choice {
    /// The system git when it is installed and new enough, else the built-in one.
    #[default]
    Auto,
    System,
    Builtin,
}

impl std::str::FromStr for Choice {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "auto" => Ok(Choice::Auto),
            "system" => Ok(Choice::System),
            "builtin" => Ok(Choice::Builtin),
            _ => Err(format!("--git takes auto, system or builtin, not {s:?}")),
        }
    }
}

/// The git to use, and a line for the user when the built-in one stands in for a missing one.
pub fn choose(choice: Choice) -> std::result::Result<(Box<dyn Git>, Option<String>), String> {
    let system = SystemGit::new();
    match choice {
        Choice::System => Ok((Box::new(system), None)),
        #[cfg(feature = "builtin-git")]
        Choice::Builtin => Ok((Box::new(crate::builtin::BuiltinGit::new()), None)),
        #[cfg(not(feature = "builtin-git"))]
        Choice::Builtin => Err("this toa was built without the built-in git".into()),
        Choice::Auto => match system.version() {
            Ok(_) => Ok((Box::new(system), None)),
            #[cfg(feature = "builtin-git")]
            Err(e) => Ok((Box::new(crate::builtin::BuiltinGit::new()), Some(format!("using the built-in git: {e}")))),
            #[cfg(not(feature = "builtin-git"))]
            Err(_) => Ok((Box::new(system), None)),
        },
    }
}

/// The system's `git`.
pub struct SystemGit {
    exe: OsString,
    /// Timeout of local calls.
    pub timeout: Duration,
    /// Timeout of calls that use the network.
    pub network_timeout: Duration,
    /// The value of `GIT_ALLOW_PROTOCOL` when fetching: `https:ssh` (7.3). Tests of local
    /// fetching widen it; the tool never does.
    pub allow_protocol: String,
    /// Extra environment for every call, after the tool's own (tests use it to keep the
    /// user's settings out).
    pub env: Vec<(OsString, OsString)>,
    version: OnceLock<std::result::Result<(u32, u32), String>>,
    cat: Mutex<Option<CatFile>>,
}

/// The variables that would make git work on another repository than the one named.
const CLEARED_ENV: [&str; 9] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
    "GIT_LITERAL_PATHSPECS",
];

impl Default for SystemGit {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemGit {
    pub fn new() -> Self {
        let exe = std::env::var_os("TOA_GIT").unwrap_or_else(|| "git".into());
        SystemGit {
            exe,
            timeout: Duration::from_secs(600),
            network_timeout: Duration::from_secs(300),
            allow_protocol: "https:ssh".into(),
            env: Vec::new(),
            version: OnceLock::new(),
            cat: Mutex::new(None),
        }
    }

    /// The version of git, checked once (`git --version`, at least [`MIN_VERSION`]).
    pub fn version(&self) -> Result<(u32, u32)> {
        let v = self.version.get_or_init(|| {
            let out = self.command(None).arg("--version").output().map_err(|e| format!("missing: {e}"))?;
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            let v = text.trim().strip_prefix("git version ").unwrap_or("").to_string();
            let mut parts = v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
            let major = parts.next().and_then(|s| s.parse().ok());
            let minor = parts.next().and_then(|s| s.parse().ok());
            match (major, minor) {
                (Some(a), Some(b)) if (a, b) >= MIN_VERSION => Ok((a, b)),
                (Some(_), Some(_)) => Err(format!("old: {v}")),
                _ => Err(format!("missing: unexpected output {:?}", text.trim())),
            }
        });
        match v {
            Ok(v) => Ok(*v),
            Err(m) if m.starts_with("old: ") => Err(GitError::TooOld(m[5..].to_string())),
            Err(m) => Err(GitError::Missing(m.trim_start_matches("missing: ").to_string())),
        }
    }

    fn command(&self, repo: Option<&Path>) -> Command {
        self.command_in(repo, false)
    }

    /// With `foreground`, git stays in the tool's process group and, when the tool runs in a
    /// terminal, reads from it: a signing program may ask for a passphrase there.
    fn command_in(&self, repo: Option<&Path>, foreground: bool) -> Command {
        let mut c = Command::new(&self.exe);
        if let Some(r) = repo {
            c.arg("-C").arg(r);
        }
        for k in CLEARED_ENV {
            c.env_remove(k);
        }
        c.env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").env("LC_ALL", "C");
        for (k, v) in &self.env {
            c.env(k, v);
        }
        use std::io::IsTerminal;
        if foreground && std::io::stdin().is_terminal() {
            c.stdin(Stdio::inherit());
            return c;
        }
        c.stdin(Stdio::null());
        #[cfg(unix)]
        if !foreground {
            use std::os::unix::process::CommandExt;
            c.process_group(0);
        }
        c
    }

    /// Run git to the end with a timeout; the output, or the failure.
    fn run(&self, repo: Option<&Path>, args: &[&OsStr], env: &[(&str, &OsStr)], timeout: Duration) -> Result<Vec<u8>> {
        self.run_in(repo, args, env, timeout, false)
    }

    fn run_in(
        &self,
        repo: Option<&Path>,
        args: &[&OsStr],
        env: &[(&str, &OsStr)],
        timeout: Duration,
        foreground: bool,
    ) -> Result<Vec<u8>> {
        self.version()?;
        let mut c = self.command_in(repo, foreground);
        c.args(args);
        for (k, v) in env {
            c.env(k, v);
        }
        let shown = shown(args);
        let child =
            c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| GitError::Missing(e.to_string()))?;
        let (status, out, err, timed_out) = wait_with_timeout(child, timeout)?;
        if timed_out {
            return Err(GitError::TimedOut(shown));
        }
        if !status.success() {
            return Err(GitError::Failed {
                command: shown,
                code: status.code(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
            });
        }
        Ok(out)
    }

    fn run_text(&self, repo: Option<&Path>, args: &[&str]) -> Result<String> {
        let a: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let out = self.run(repo, &a, &[], self.timeout)?;
        String::from_utf8(out).map_err(|_| GitError::Unreadable(shown(&a))).map(|s| s.trim_end().to_string())
    }

    /// Like `run_text`, but a failure with exit code 1 (git's "no") gives `None`.
    fn run_maybe(&self, repo: Option<&Path>, args: &[&str]) -> Result<Option<String>> {
        match self.run_text(repo, args) {
            Ok(s) => Ok(Some(s)),
            Err(GitError::Failed { code: Some(1 | 128), .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// A command for messages: without the `-c` settings the tool always passes.
fn shown(args: &[&OsStr]) -> String {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if *a == "-c" {
            skip = true;
            continue;
        }
        out.push(a.to_string_lossy());
    }
    out.join(" ")
}

/// End a process and every process it started.
fn kill_tree(child_id: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child_id.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // the child leads its own process group (`process_group(0)`); one run in the foreground
        // does not, and is ended alone
        let group = Command::new("kill")
            .args(["-KILL", "--", &format!("-{child_id}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !group.is_ok_and(|s| s.success()) {
            let _ = Command::new("kill")
                .args(["-KILL", "--", &child_id.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// Ends a process tree when a timeout runs out, unless dropped first.
struct Watchdog {
    done: Option<mpsc::Sender<()>>,
    fired: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watchdog {
    fn start(pid: u32, timeout: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        let fired = Arc::new(AtomicBool::new(false));
        let f = fired.clone();
        let thread = std::thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(timeout) {
                f.store(true, Ordering::SeqCst);
                kill_tree(pid);
            }
        });
        Watchdog { done: Some(tx), fired, thread: Some(thread) }
    }
    fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Some(tx) = self.done.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn wait_with_timeout(
    mut child: Child,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>, bool)> {
    let dog = Watchdog::start(child.id(), timeout);
    let mut stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut v = Vec::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.take(64 * 1024).read_to_end(&mut v);
            let _ = std::io::copy(e, &mut std::io::sink());
        }
        v
    });
    let mut out = Vec::new();
    if let Some(mut o) = child.stdout.take() {
        o.read_to_end(&mut out)?;
    }
    let status = child.wait()?;
    let err = err_thread.join().unwrap_or_default();
    let fired = dog.fired();
    drop(dog);
    Ok((status, out, err, fired))
}

/// A long-running `git cat-file --batch` for one repository.
struct CatFile {
    repo: PathBuf,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl CatFile {
    fn header(&mut self, id: &str) -> Result<Option<(String, u64)>> {
        writeln!(self.stdin, "{id}")?;
        self.stdin.flush()?;
        let mut line = String::new();
        self.stdout.read_line(&mut line)?;
        let line = line.trim_end();
        if line.ends_with(" missing") || line.ends_with(" ambiguous") {
            return Ok(None);
        }
        let mut parts = line.split(' ');
        let (_, kind, size) = (parts.next(), parts.next(), parts.next());
        match (kind, size.and_then(|s| s.parse::<u64>().ok())) {
            (Some(k), Some(n)) => Ok(Some((k.to_string(), n))),
            _ => Err(GitError::Unreadable(format!("cat-file header {line:?}"))),
        }
    }
}

impl Drop for CatFile {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl SystemGit {
    /// Run `f` on the `cat-file --batch` of `repo`, starting it when needed.
    fn with_cat<T>(&self, repo: &Path, f: impl FnOnce(&mut CatFile) -> Result<T>) -> Result<T> {
        self.version()?;
        let mut guard = self.cat.lock().unwrap_or_else(|e| e.into_inner());
        if guard.as_ref().is_none_or(|c| c.repo != repo) {
            *guard = None;
            let mut c = self.command(Some(repo));
            c.args(["cat-file", "--batch"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut child = c.spawn().map_err(|e| GitError::Missing(e.to_string()))?;
            let stdin = child.stdin.take().ok_or_else(|| GitError::Unreadable("cat-file stdin".into()))?;
            let stdout = BufReader::new(child.stdout.take().ok_or_else(|| GitError::Unreadable("cat-file".into()))?);
            *guard = Some(CatFile { repo: repo.to_path_buf(), child, stdin, stdout });
        }
        let cat = guard.as_mut().expect("started above");
        let r = f(cat);
        if matches!(r, Err(GitError::Io(_) | GitError::Unreadable(_))) {
            *guard = None;
        }
        r
    }
}

fn parse_list_record(rec: &[u8]) -> Result<ListEntry> {
    let bad = || GitError::Unreadable(format!("ls-tree record {:?}", String::from_utf8_lossy(rec)));
    let tab = rec.iter().position(|&b| b == b'\t').ok_or_else(bad)?;
    let head = std::str::from_utf8(&rec[..tab]).map_err(|_| bad())?;
    let mut f = head.split_ascii_whitespace();
    let (mode, kind, id, size) = (f.next(), f.next(), f.next(), f.next());
    let (Some(mode), Some(kind), Some(id), Some(size)) = (mode, kind, id, size) else { return Err(bad()) };
    let raw = &rec[tab + 1..];
    let (path, utf8) = match std::str::from_utf8(raw) {
        Ok(s) => (s.to_string(), true),
        Err(_) => (String::from_utf8_lossy(raw).into_owned(), false),
    };
    Ok(ListEntry {
        mode: mode.to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
        size: size.parse().ok(),
        path,
        utf8,
    })
}

impl Git for SystemGit {
    fn toplevel(&self, dir: &Path) -> Result<Option<PathBuf>> {
        match self.run_maybe(Some(dir), &["rev-parse", "--show-toplevel"]) {
            Ok(Some(s)) if !s.is_empty() => Ok(Some(PathBuf::from(s))),
            Ok(_) => Ok(None),
            Err(GitError::Failed { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn git_dir(&self, dir: &Path) -> Result<PathBuf> {
        let s = self.run_text(Some(dir), &["rev-parse", "--absolute-git-dir"])?;
        Ok(PathBuf::from(s))
    }

    fn object_format(&self, repo: &Path) -> Result<String> {
        // `--show-object-format` exists from git 2.25
        self.run_text(Some(repo), &["rev-parse", "--show-object-format"])
    }

    fn resolve_commit(&self, repo: &Path, rev: &str) -> Result<Option<String>> {
        let rev = safe_arg(rev)?;
        let spec = format!("{rev}^{{commit}}");
        self.run_maybe(Some(repo), &["rev-parse", "--verify", "--quiet", "--end-of-options", &spec])
    }

    fn resolve_ref(&self, repo: &Path, refname: &str) -> Result<Option<String>> {
        if !refname.starts_with("refs/") {
            return Err(GitError::BadArgument(format!("{refname:?}")));
        }
        let r = safe_arg(refname)?;
        self.run_maybe(Some(repo), &["rev-parse", "--verify", "--quiet", "--end-of-options", r])
    }

    fn object_at(&self, repo: &Path, commit: &str, path: &str) -> Result<Option<String>> {
        if !is_hex_id(commit) {
            return Err(GitError::BadArgument(format!("{commit:?} is not a commit id")));
        }
        if path.is_empty() || path.chars().any(char::is_control) {
            return Err(GitError::BadArgument(format!("{path:?}")));
        }
        let spec = format!("{commit}:{path}");
        self.run_maybe(Some(repo), &["rev-parse", "--verify", "--quiet", "--end-of-options", &spec])
    }

    fn has_commits(&self, repo: &Path) -> Result<bool> {
        let s = self.run_text(Some(repo), &["for-each-ref", "--count=1", "--format=%(objectname)", "refs/"])?;
        if !s.is_empty() {
            return Ok(true);
        }
        Ok(self.run_maybe(Some(repo), &["rev-parse", "--verify", "--quiet", "HEAD"])?.is_some())
    }

    fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
        let (a, d) = (safe_arg(ancestor)?, safe_arg(descendant)?);
        match self.run_text(Some(repo), &["merge-base", "--is-ancestor", a, d]) {
            Ok(_) => Ok(true),
            Err(GitError::Failed { code: Some(1), .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn list_tree(&self, repo: &Path, tree_ish: &str, max_entries: usize) -> Result<Vec<ListEntry>> {
        self.version()?;
        let t = safe_arg(tree_ish)?;
        let mut c = self.command(Some(repo));
        c.args(["ls-tree", "-r", "-t", "-l", "-z", t]).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| GitError::Missing(e.to_string()))?;
        let dog = Watchdog::start(child.id(), self.timeout);
        let mut stderr = child.stderr.take();
        let err_thread = std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(e) = stderr.as_mut() {
                let _ = e.take(64 * 1024).read_to_end(&mut v);
                let _ = std::io::copy(e, &mut std::io::sink());
            }
            v
        });
        let mut out = Vec::new();
        let mut too_many = false;
        let mut parse_error = None;
        if let Some(o) = child.stdout.take() {
            let mut r = BufReader::new(o);
            let mut rec = Vec::new();
            loop {
                rec.clear();
                // one record is at most a mode, a type, an id, a size and a path
                let n = r.by_ref().take(64 * 1024).read_until(0, &mut rec)?;
                if n == 0 {
                    break;
                }
                if rec.last() != Some(&0) {
                    parse_error = Some(GitError::Unreadable("an ls-tree record without its end".into()));
                    break;
                }
                rec.pop();
                if out.len() >= max_entries {
                    too_many = true;
                    break;
                }
                match parse_list_record(&rec) {
                    Ok(e) => out.push(e),
                    Err(e) => {
                        parse_error = Some(e);
                        break;
                    }
                }
            }
        }
        if too_many || parse_error.is_some() {
            kill_tree(child.id());
        }
        let status = child.wait()?;
        let err = err_thread.join().unwrap_or_default();
        let fired = dog.fired();
        drop(dog);
        if too_many {
            return Err(GitError::TooLarge(format!("the listing of {t} has more than {max_entries} entries")));
        }
        if let Some(e) = parse_error {
            return Err(e);
        }
        if fired {
            return Err(GitError::TimedOut(format!("ls-tree {t}")));
        }
        if !status.success() {
            return Err(GitError::Failed {
                command: format!("ls-tree -r -t -l -z {t}"),
                code: status.code(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
            });
        }
        Ok(out)
    }

    fn object_size(&self, repo: &Path, id: &str) -> Result<Option<u64>> {
        if !is_hex_id(id) {
            return Err(GitError::BadArgument(format!("{id:?} is not an object id")));
        }
        self.with_cat(repo, |cat| {
            // `--batch` prints the content after the header; ask for the header only through
            // `info` would need git 2.36, so read the header and skip the content
            let Some((_, size)) = cat.header(id)? else { return Ok(None) };
            std::io::copy(&mut cat.stdout.by_ref().take(size + 1), &mut std::io::sink())?;
            Ok(Some(size))
        })
    }

    fn read_blob(&self, repo: &Path, id: &str, max: u64) -> Result<Vec<u8>> {
        if !is_hex_id(id) {
            return Err(GitError::BadArgument(format!("{id:?} is not an object id")));
        }
        self.with_cat(repo, |cat| {
            let Some((kind, size)) = cat.header(id)? else {
                return Err(GitError::Failed {
                    command: format!("cat-file {id}"),
                    code: None,
                    stderr: "missing".into(),
                });
            };
            if kind != "blob" || size > max {
                std::io::copy(&mut cat.stdout.by_ref().take(size + 1), &mut std::io::sink())?;
                return Err(if kind != "blob" {
                    GitError::Unreadable(format!("{id} is a {kind}, not a blob"))
                } else {
                    GitError::TooLarge(format!("blob {id} has {size} bytes, more than {max}"))
                });
            }
            let mut buf = vec![0u8; size as usize];
            cat.stdout.read_exact(&mut buf)?;
            let mut lf = [0u8; 1];
            cat.stdout.read_exact(&mut lf)?;
            Ok(buf)
        })
    }

    fn tree_of_dir(&self, root: &Path, rel: &str) -> Result<String> {
        if rel.is_empty() || rel.split('/').any(|p| p.is_empty() || p == "." || p == "..") {
            return Err(GitError::BadArgument(format!("{rel:?}")));
        }
        let git_dir = self.git_dir(root)?;
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let index = git_dir.join(format!("toa-index-{}-{n}", std::process::id()));
        let _ = std::fs::remove_file(&index);
        let env: [(&str, &OsStr); 2] =
            [("GIT_INDEX_FILE", index.as_os_str()), ("GIT_LITERAL_PATHSPECS", OsStr::new("1"))];
        let add: [&OsStr; 4] = ["add".as_ref(), "--all".as_ref(), "--".as_ref(), rel.as_ref()];
        let r = self.run(Some(root), &add, &env, self.timeout).and_then(|_| {
            let prefix = format!("--prefix={rel}/");
            let wt: [&OsStr; 2] = ["write-tree".as_ref(), prefix.as_ref()];
            self.run(Some(root), &wt, &env, self.timeout)
        });
        let _ = std::fs::remove_file(&index);
        let out = r?;
        let id = String::from_utf8_lossy(&out).trim().to_string();
        if !is_hex_id(&id) {
            return Err(GitError::Unreadable(format!("write-tree printed {id:?}")));
        }
        Ok(id)
    }

    fn remote_url(&self, repo: &Path, remote: &str) -> Result<Option<String>> {
        let r = safe_arg(remote)?;
        if !r.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(GitError::BadArgument(format!("{remote:?}")));
        }
        self.run_maybe(Some(repo), &["config", "--get", &format!("remote.{r}.url")])
    }

    fn commit_date(&self, repo: &Path, commit: &str) -> Result<String> {
        let c = safe_arg(commit)?;
        self.run_text(Some(repo), &["show", "-s", "--format=%cI", "--end-of-options", c])
    }

    fn additions(&self, repo: &Path, commit: &str, since: Option<&str>, patterns: &[&str]) -> Result<Vec<Addition>> {
        let range = match since {
            Some(s) => format!("{}..{}", safe_arg(s)?, safe_arg(commit)?),
            None => safe_arg(commit)?.to_string(),
        };
        let mut args = vec![
            "-c",
            "core.quotePath=false",
            "log",
            "--first-parent",
            "-m",
            "--reverse",
            "--no-renames",
            "--diff-filter=A",
            "--name-only",
            "--format=@@toa %H %cI",
            &range,
            "--",
        ];
        args.extend(patterns);
        let s = self.run_text(Some(repo), &args)?;
        let mut out: Vec<Addition> = Vec::new();
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("@@toa ") {
                let (id, date) = rest.split_once(' ').unwrap_or((rest, ""));
                out.push(Addition { commit: id.to_string(), date: date.to_string(), paths: Vec::new() });
            } else if !line.is_empty() {
                if let Some(last) = out.last_mut() {
                    last.paths.push(line.to_string());
                }
            }
        }
        out.retain(|a| !a.paths.is_empty());
        Ok(out)
    }

    fn commit_paths(&self, root: &Path, paths: &[String], message: &str, sign: bool) -> Result<String> {
        if paths.is_empty() || paths.iter().any(|p| p.is_empty() || p.split('/').any(|x| x.is_empty() || x == "..")) {
            return Err(GitError::BadArgument(format!("{paths:?}")));
        }
        let env: [(&str, &OsStr); 1] = [("GIT_LITERAL_PATHSPECS", OsStr::new("1"))];
        let mut add: Vec<&OsStr> = vec!["add".as_ref(), "--all".as_ref(), "--".as_ref()];
        add.extend(paths.iter().map(|p| OsStr::new(p.as_str())));
        self.run(Some(root), &add, &env, self.timeout)?;
        let mut commit: Vec<&OsStr> = vec!["commit".as_ref(), "--quiet".as_ref()];
        if sign {
            commit.push("-S".as_ref());
        }
        commit.extend([OsStr::new("-m"), OsStr::new(message), OsStr::new("--")]);
        commit.extend(paths.iter().map(|p| OsStr::new(p.as_str())));
        // a signing program may ask for a passphrase in the terminal; it is not waited for long
        let signing = sign || self.signs_commits(root)?;
        let timeout = if signing { SIGNING_TIMEOUT.min(self.timeout) } else { self.timeout };
        self.run_in(Some(root), &commit, &env, timeout, signing)?;
        self.run_text(Some(root), &["rev-parse", "HEAD"])
    }

    fn signs_commits(&self, repo: &Path) -> Result<bool> {
        Ok(self.run_maybe(Some(repo), &["config", "--type=bool", "--get", "commit.gpgsign"])?.as_deref()
            == Some("true"))
    }

    fn commit_signature(&self, repo: &Path, commit: &str) -> Result<Option<SignatureFormat>> {
        if !is_hex_id(commit) {
            return Err(GitError::BadArgument(commit.to_string()));
        }
        let raw =
            self.run(Some(repo), &["cat-file".as_ref(), "commit".as_ref(), commit.as_ref()], &[], self.timeout)?;
        Ok(signature_of_commit(&raw))
    }

    fn init_bare(&self, dir: &Path) -> Result<()> {
        let a: [&OsStr; 5] =
            ["init".as_ref(), "--bare".as_ref(), "--quiet".as_ref(), "--object-format=sha1".as_ref(), dir.as_os_str()];
        self.run(None, &a, &[], self.timeout).map(|_| ())
    }

    fn update_ref(&self, repo: &Path, refname: &str, id: &str) -> Result<()> {
        if !refname.starts_with("refs/toa/") || !is_hex_id(id) {
            return Err(GitError::BadArgument(format!("{refname} {id}")));
        }
        self.run_text(Some(repo), &["update-ref", "--no-deref", refname, id]).map(|_| ())
    }

    fn fetch(&self, repo: &Path, address: &str, wants: &[&str], shallow: bool) -> Result<()> {
        // the address keeps 5.1 (checked by the caller) or is a local test address
        if address.is_empty() || address.starts_with('-') {
            return Err(GitError::BadArgument(format!("{address:?}")));
        }
        let mut specs = Vec::new();
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
            } else {
                return Err(GitError::BadArgument(format!("{w:?}")));
            }
        }
        let mut args: Vec<&OsStr> = Vec::new();
        for c in [
            "fetch.fsckObjects=true",
            "transfer.fsckObjects=true",
            "http.followRedirects=false",
            "protocol.version=2",
            "fetch.writeCommitGraph=false",
            "maintenance.auto=false",
            "gc.auto=0",
        ] {
            args.push("-c".as_ref());
            args.push(c.as_ref());
        }
        for a in ["fetch", "--quiet", "--no-tags", "--no-write-fetch-head", "--no-recurse-submodules", "--no-auto-gc"] {
            args.push(a.as_ref());
        }
        if shallow {
            args.push("--depth=1".as_ref());
        }
        args.push("--".as_ref());
        args.push(address.as_ref());
        for s in &specs {
            args.push(s.as_ref());
        }
        let env: [(&str, &OsStr); 2] =
            [("GIT_ALLOW_PROTOCOL", self.allow_protocol.as_ref()), ("GCM_INTERACTIVE", "never".as_ref())];
        self.run(Some(repo), &args, &env, self.network_timeout).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_that_look_like_options_are_refused() {
        assert!(safe_arg("--upload-pack=x").is_err());
        assert!(safe_arg("").is_err());
        assert!(safe_arg("a b").is_err());
        assert!(safe_arg("main").is_ok());
        let g = SystemGit::new();
        assert!(matches!(g.object_at(Path::new("."), "-x", "a"), Err(GitError::BadArgument(_))));
        assert!(matches!(g.update_ref(Path::new("."), "refs/heads/x", "0"), Err(GitError::BadArgument(_))));
    }

    #[test]
    fn listing_records_are_read() {
        let e = parse_list_record(b"100644 blob 0123456789012345678901234567890123456789      12\ta/b c").unwrap();
        assert_eq!((e.mode.as_str(), e.size, e.path.as_str()), ("100644", Some(12), "a/b c"));
        let t = parse_list_record(b"040000 tree 0123456789012345678901234567890123456789       -\ta").unwrap();
        assert!(t.is_tree() && t.size.is_none());
        assert!(parse_list_record(b"garbage").is_err());
    }
}
