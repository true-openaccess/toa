//! Trees git stores that are not canonical or not well-formed (docs/lines/tools.md 9.4): each is
//! made an object directory in a library commit, and must be rejected both by the library check
//! (6.4, verifying from the listing) and by writing an object out of git (7.3, steps 5 and 6),
//! through the system git. The well-formed tree beside them must pass both.
//!
//! The trees are written with `git hash-object -t tree --literally`, which stores any bytes.

#[path = "support.rs"]
mod support;

use std::path::Path;

use support::{git, temp};
use toa_core::hash::{self, Algorithm};
use toa_repo::{fetch, library};

const MANIFEST: &[u8] = b"type: doc\nlicense: MIT\n";

fn object(home: &Path, repo: &Path, kind: &str, bytes: &[u8]) -> String {
    let mut args = vec!["hash-object", "-w", "--stdin"];
    if kind == "tree" {
        args.extend(["-t", "tree", "--literally"]);
    }
    git(home, repo, &args, Some(bytes))
}

/// A tree body from entries `(mode, name, id)`, in the order given.
fn body(entries: &[(&str, &[u8], &str)]) -> Vec<u8> {
    let mut b = Vec::new();
    for (mode, name, id) in entries {
        b.extend_from_slice(mode.as_bytes());
        b.push(b' ');
        b.extend_from_slice(name);
        b.push(0);
        b.extend_from_slice(&hash::from_hex(id).unwrap());
    }
    b
}

/// The cases: label, and the entries of the object tree beside `toa.yaml`.
/// One entry of a tree body: mode, name, id.
type Row<'a> = (&'static str, &'static [u8], &'a str);

fn cases<'a>(x: &'a str, sub: &'a str, m: &'a str) -> Vec<(&'static str, Vec<Row<'a>>)> {
    vec![
        ("good", vec![("100644", b"a", x), ("100644", b"toa.yaml", m)]),
        ("zero-padded-mode", vec![("0100644", b"a", x), ("100644", b"toa.yaml", m)]),
        ("slash-in-name", vec![("100644", b"a/b", x), ("100644", b"toa.yaml", m)]),
        ("dotdot", vec![("40000", b"..", sub), ("100644", b"toa.yaml", m)]),
        ("dotgit", vec![("40000", b".git", sub), ("100644", b"toa.yaml", m)]),
        ("dotgit-upper", vec![("40000", b".GIT", sub), ("100644", b"toa.yaml", m)]),
        ("dotgit-ntfs-short", vec![("40000", b"GIT~1", sub), ("100644", b"toa.yaml", m)]),
        ("unsorted", vec![("100644", b"toa.yaml", m), ("100644", b"a", x)]),
        ("duplicate", vec![("100644", b"a", x), ("100644", b"a", x), ("100644", b"toa.yaml", m)]),
        ("empty-name", vec![("100644", b"", x), ("100644", b"toa.yaml", m)]),
        ("executable", vec![("100755", b"run.sh", x), ("100644", b"toa.yaml", m)]),
        ("symlink", vec![("120000", b"link", x), ("100644", b"toa.yaml", m)]),
    ]
}

#[test]
fn bad_trees_are_rejected_by_the_check_and_by_fetching() {
    let home = temp("trees-home");
    let repo = temp("trees-repo");
    git(&home, &repo, &["init", "--quiet", "--bare", "--object-format=sha1"], None);
    let reader = support::reader(&home);
    let g = reader.as_ref();

    let x = object(&home, &repo, "blob", b"x\n");
    let m = object(&home, &repo, "blob", MANIFEST);
    let attrs = object(&home, &repo, "blob", b"* -text\n");
    let sub = object(&home, &repo, "tree", &body(&[("100644", b"a", &x)]));

    let mut failures = Vec::new();
    for (label, entries) in cases(&x, &sub, &m) {
        let t = object(&home, &repo, "tree", &body(&entries));
        let name = format!("doc_{t}");
        let doc = object(&home, &repo, "tree", &body(&[("40000", name.as_bytes(), &t)]));
        let root =
            object(&home, &repo, "tree", &body(&[("100644", b".gitattributes", &attrs), ("40000", b"doc", &doc)]));
        let commit = git(&home, &repo, &["commit-tree", &root, "-m", label], None);

        // the library check, from the listing (6.4 item 2, 7.4)
        let report = library::check(g, &repo, &commit, None, &library::Limits::default()).unwrap();
        let codes: Vec<String> = report.problems.iter().map(|p| format!("{} {}", p.code, p.path)).collect();
        let rejected = report.problems.iter().any(|p| p.is_error());
        // fetching: the listing checked before anything is written (7.3 steps 5 and 6)
        let out = temp(&format!("trees-out-{label}"));
        let written = fetch::write_from_git(g, &repo, &t, &name, &out);
        let left: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name()).collect();

        if label == "good" {
            // the tree written by hand is the canonical one, and its hash is the files' hash
            let files: Vec<(&str, &[u8])> = vec![("a", b"x\n"), ("toa.yaml", MANIFEST)];
            assert_eq!(hash::hash_files(Algorithm::Sha1, &files).unwrap(), t);
            if rejected {
                failures.push(format!("{label}: the check rejected it: {codes:?}"));
            }
            match &written {
                Ok(p) if p.ends_with(&name) => {}
                other => failures.push(format!("{label}: not written: {other:?}")),
            }
        } else {
            if !rejected {
                failures.push(format!("{label}: the check passed it"));
            }
            if written.is_ok() {
                failures.push(format!("{label}: written out of git"));
            }
            if !left.is_empty() {
                failures.push(format!("{label}: left behind {left:?}"));
            }
        }
        eprintln!("{label}: check {codes:?}; fetch {:?}", written.as_ref().map_err(|ps| ps[0].code));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
