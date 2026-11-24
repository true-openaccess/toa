//! Helpers of the tests that build git repositories: a temporary directory per test, and the
//! system git with the user's own settings kept out.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use toa_repo::builtin::BuiltinGit;
use toa_repo::git::{Git, SystemGit};

/// A fresh, empty directory under the system's temporary directory.
pub fn temp(label: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join("toa-tests").join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The environment that keeps the user's and the system's git settings out of a test.
pub fn env(home: &Path) -> Vec<(String, String)> {
    let cfg = home.join("gitconfig");
    if !cfg.exists() {
        std::fs::write(&cfg, b"").unwrap();
    }
    let h = home.to_string_lossy().to_string();
    vec![
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_CONFIG_GLOBAL".into(), cfg.to_string_lossy().to_string()),
        ("HOME".into(), h.clone()),
        ("XDG_CONFIG_HOME".into(), h),
        ("GIT_AUTHOR_NAME".into(), "Test".into()),
        ("GIT_AUTHOR_EMAIL".into(), "test@example.org".into()),
        ("GIT_COMMITTER_NAME".into(), "Test".into()),
        ("GIT_COMMITTER_EMAIL".into(), "test@example.org".into()),
        ("GIT_AUTHOR_DATE".into(), "2026-11-23T12:00:00Z".into()),
        ("GIT_COMMITTER_DATE".into(), "2026-11-23T12:00:00Z".into()),
    ]
}

/// The system git as the tool runs it, with the test's environment.
pub fn system_git(home: &Path) -> SystemGit {
    let mut g = SystemGit::new();
    g.env = env(home).into_iter().map(|(k, v)| (k.into(), v.into())).collect();
    g
}

/// The built-in git as the tests run it: only each repository's own settings, and local
/// addresses allowed for fetching.
pub fn builtin_git() -> BuiltinGit {
    let mut g = BuiltinGit::new();
    g.isolated = true;
    g.allow_protocol = "https:ssh:file".into();
    g
}

/// The git that reads libraries and fetches in this run: the built-in one when
/// `TOA_TEST_GIT=builtin`, else the system git. CI runs the tests both ways (tools.md 9.5).
/// Sealing's second computation and committing always use the system git.
pub fn reader(home: &Path) -> Box<dyn Git> {
    match std::env::var("TOA_TEST_GIT").as_deref() {
        Ok("builtin") => Box::new(builtin_git()),
        _ => {
            let mut g = system_git(home);
            g.allow_protocol = "https:ssh:file".into();
            Box::new(g)
        }
    }
}

/// Run git in `dir` for the test itself; the trimmed standard output.
pub fn git(home: &Path, dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> String {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    for (k, v) in env(home) {
        c.env(k, v);
    }
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().expect("git runs");
    if let Some(b) = stdin {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Write a file, making its directories; bytes as they are.
pub fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A new repository with a `.gitattributes` of a library (6.1) and a branch `main`.
pub fn library(home: &Path, dir: &Path) {
    git(home, dir, &["init", "--quiet", "--object-format=sha1"], None);
    git(home, dir, &["symbolic-ref", "HEAD", "refs/heads/main"], None);
    write(&dir.join(".gitattributes"), b"* -text\n");
}

pub fn commit_all(home: &Path, dir: &Path, msg: &str) -> String {
    git(home, dir, &["add", "--all"], None);
    git(home, dir, &["commit", "--quiet", "-m", msg], None);
    git(home, dir, &["rev-parse", "HEAD"], None)
}
