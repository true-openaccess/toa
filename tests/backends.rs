//! The system git and the built-in git side by side (docs/lines/tools.md 9.4, 9.5): what they
//! read from the same repositories must be the same; fetching, both must refuse the same bad
//! trees before anything enters the repository; the built-in git must refuse repositories it
//! cannot read completely, saying that git is needed.

#[path = "support.rs"]
mod support;

use std::path::Path;

use support::{commit_all, git, library, temp, write};
use toa_core::hash;
use toa_repo::git::{Git, GitError};
use toa_repo::{index, library, seal};

/// Both gits as the tests run them, with local addresses allowed for fetching.
fn both(home: &Path) -> Vec<(&'static str, Box<dyn Git>)> {
    let mut s = support::system_git(home);
    s.allow_protocol = "https:ssh:file".into();
    vec![("system", Box::new(s)), ("builtin", Box::new(support::builtin_git()))]
}

/// A library with some history: two commits, a merge, an annotated tag, a branch.
fn a_library(home: &Path) -> (std::path::PathBuf, Vec<String>) {
    let lib = temp("backends-lib");
    library(home, &lib);
    let g = support::system_git(home);
    let opts = seal::SealOptions { dry_run: false, lookup: Vec::new(), workspace: None };
    write(&lib.join("ifc/x/ifc_draft-a/toa.yaml"), b"type: ifc\nlicense: MIT\nname: x\ntitle: X\npins:\n- {name: a}\n");
    write(&lib.join("ifc/x/ifc_draft-a/README.md"), "caf\u{e9}\n".as_bytes());
    seal::seal(&g, &lib, &opts).unwrap();
    let c1 = commit_all(home, &lib, "first");
    git(home, &lib, &["checkout", "--quiet", "-b", "side"], None);
    write(&lib.join("doc/d/doc_draft-d/toa.yaml"), b"type: doc\nlicense: MIT\n");
    write(&lib.join("doc/d/doc_draft-d/README.md"), b"hello\n");

    seal::seal(&g, &lib, &opts).unwrap();
    let side = commit_all(home, &lib, "side");
    git(home, &lib, &["checkout", "--quiet", "main"], None);
    write(&lib.join("notes.txt"), b"n\n");
    let c2 = commit_all(home, &lib, "second");
    git(home, &lib, &["merge", "--quiet", "--no-ff", "-m", "merge", "side"], None);
    let merge = git(home, &lib, &["rev-parse", "HEAD"], None);
    git(home, &lib, &["tag", "-a", "-m", "v1", "v1", &c2], None);
    git(home, &lib, &["remote", "add", "origin", "https://example.org/lib.git"], None);
    (lib, vec![c1, side, c2, merge])
}

#[test]
fn both_gits_read_the_same() {
    let home = temp("backends-home");
    let (lib, commits) = a_library(&home);
    let [c1, side, c2, merge] = [&commits[0], &commits[1], &commits[2], &commits[3]];
    let mut seen: Vec<(String, String)> = Vec::new();
    for (label, g) in both(&home) {
        let g = g.as_ref();
        let mut r = Vec::new();
        r.push(format!("{:?}", g.toplevel(&lib).unwrap().map(|p| p.canonicalize().unwrap())));
        r.push(format!("{:?}", g.git_dir(&lib).unwrap().canonicalize().unwrap()));
        r.push(g.object_format(&lib).unwrap());
        for rev in ["HEAD", "main", "side", "v1", "refs/heads/main", "HEAD~1", "HEAD^2", c1.as_str(), "nothing"] {
            r.push(format!("{rev} {:?}", g.resolve_commit(&lib, rev).unwrap()));
        }
        for name in ["refs/heads/side", "refs/tags/v1", "refs/heads/none"] {
            r.push(format!("{name} {:?}", g.resolve_ref(&lib, name).unwrap()));
        }
        r.push(format!("{:?}", g.object_at(&lib, merge, "ifc/x").unwrap()));
        r.push(format!("{:?}", g.object_at(&lib, merge, "ifc/none").unwrap()));
        r.push(format!("{}", g.has_commits(&lib).unwrap()));
        for (a, d) in [(c1, merge), (side, merge), (merge, c1), (c2, side)] {
            r.push(format!("{}", g.is_ancestor(&lib, a, d).unwrap()));
        }
        let listing = g.list_tree(&lib, merge, 1000).unwrap();
        r.push(format!("{listing:?}"));
        assert!(matches!(g.list_tree(&lib, merge, 2), Err(GitError::TooLarge(_))), "{label}");
        let blob = listing.iter().find(|e| e.path.ends_with("README.md")).unwrap();
        r.push(format!("{:?} {:?}", g.object_size(&lib, &blob.id).unwrap(), g.read_blob(&lib, &blob.id, 100).unwrap()));
        assert!(matches!(g.read_blob(&lib, &blob.id, 2), Err(GitError::TooLarge(_))), "{label}");
        r.push(format!("{:?}", g.object_size(&lib, &"0".repeat(40)).unwrap()));
        r.push(g.commit_date(&lib, merge).unwrap());
        r.push(format!("{:?}", g.remote_url(&lib, "origin").unwrap()));
        r.push(format!("{:?}", g.additions(&lib, merge, None, &["*/toa.yaml", "*.txt"]).unwrap()));
        r.push(format!("{:?}", g.additions(&lib, merge, Some(c1), &["*/toa.yaml"]).unwrap()));
        let check = library::check(g, &lib, merge, Some(c1), &Default::default()).unwrap();
        r.push(format!("{:?} {:?}", check.problems, check.objects));
        let head = library::publication_head(g, &lib, "main").unwrap();
        r.push(format!("{head:?}"));
        seen.push((label.to_string(), r.join("\n")));
    }
    assert_eq!(seen[0].1, seen[1].1, "system and built-in read differently");
}

#[test]
fn both_gits_index_the_same() {
    let home = temp("backends-index-home");
    let (lib, _) = a_library(&home);
    let ws = temp("backends-index-ws");
    let list = format!(
        "{{\"toa\": \"source/1\"}}\n{{\"include\": \"library.source.jsonl\", \"group\": \"a\", \"sources\": [{{\"local\": \"{}\"}}]}}\n",
        lib.to_string_lossy().replace('\\', "/")
    );
    write(&ws.join("workspace.source.jsonl"), list.as_bytes());
    let mut outputs = Vec::new();
    for (label, g) in both(&home) {
        let out = ws.join(format!("index-{label}"));
        let o = index::Options {
            workspace: ws.join("workspace.source.jsonl"),
            out: out.clone(),
            cache: ws.join(format!("cache-{label}")),
            offline: true,
            tool: "toa test".into(),
            ..Default::default()
        };
        index::build(g.as_ref(), &o).unwrap();
        let mut files = Vec::new();
        for e in walk(&out) {
            let mut text = std::fs::read_to_string(out.join(&e)).unwrap();
            if e == "index.json" {
                let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
                v["built"] = serde_json::Value::Null;
                text = v.to_string();
            }
            files.push((e, text));
        }
        outputs.push(files);
    }
    assert_eq!(outputs[0], outputs[1], "the two indexes differ");
}

fn walk(d: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn go(d: &Path, rel: &str, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let r = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
            if e.file_type().unwrap().is_dir() {
                go(&e.path(), &r, out);
            } else {
                out.push(r);
            }
        }
    }
    go(d, "", &mut out);
    out.sort();
    out
}

fn object(home: &Path, repo: &Path, kind: &str, bytes: &[u8]) -> String {
    let mut args = vec!["hash-object", "-w", "--stdin"];
    if kind != "blob" {
        args.extend(["-t", kind, "--literally"]);
    }
    git(home, repo, &args, Some(bytes))
}

/// One entry of a tree body: mode, name, id.
type Row<'a> = (&'a str, &'a [u8], &'a str);
/// A case: label, the entries of the tree, whether git's fsck accepts it.
type Case<'a> = (&'a str, Vec<Row<'a>>, bool);

fn body(entries: &[Row]) -> Vec<u8> {
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

/// Fetching the 11 bad trees of 9.4 and a good one: git's `fetch.fsckObjects` and the built-in
/// check (7.3 step 4) refuse the same ones, and leave nothing in the repository.
#[test]
fn both_gits_refuse_the_same_objects_when_fetching() {
    let home = temp("backends-fetch-home");
    let src = temp("backends-fetch-src");
    git(&home, &src, &["init", "--quiet", "--bare", "--object-format=sha1"], None);
    let x = object(&home, &src, "blob", b"x\n");
    let m = object(&home, &src, "blob", b"type: doc\nlicense: MIT\n");
    let sub = object(&home, &src, "tree", &body(&[("100644", b"a", &x)]));
    let cases: Vec<Case> = vec![
        ("good", vec![("100644", b"a", &x), ("100644", b"toa.yaml", &m)], true),
        ("zero-padded-mode", vec![("0100644", b"a", &x), ("100644", b"toa.yaml", &m)], false),
        ("slash-in-name", vec![("100644", b"a/b", &x), ("100644", b"toa.yaml", &m)], false),
        ("dotdot", vec![("40000", b"..", &sub), ("100644", b"toa.yaml", &m)], false),
        ("dotgit", vec![("40000", b".git", &sub), ("100644", b"toa.yaml", &m)], false),
        ("dotgit-upper", vec![("40000", b".GIT", &sub), ("100644", b"toa.yaml", &m)], false),
        ("dotgit-ntfs-short", vec![("40000", b"GIT~1", &sub), ("100644", b"toa.yaml", &m)], false),
        ("unsorted", vec![("100644", b"toa.yaml", &m), ("100644", b"a", &x)], false),
        ("duplicate", vec![("100644", b"a", &x), ("100644", b"a", &x), ("100644", b"toa.yaml", &m)], false),
        ("empty-name", vec![("100644", b"", &x), ("100644", b"toa.yaml", &m)], false),
        // git's fsck accepts these two; the listing check of step 5 refuses them later
        ("executable", vec![("100755", b"run.sh", &x), ("100644", b"toa.yaml", &m)], true),
        ("symlink", vec![("120000", b"link", &x), ("100644", b"toa.yaml", &m)], true),
    ];
    let address = src.to_string_lossy().replace('\\', "/");
    let mut failures = Vec::new();
    for (label, entries, accepted) in &cases {
        let t = object(&home, &src, "tree", &body(entries));
        let root = object(&home, &src, "tree", &body(&[("40000", b"doc", &t)]));
        let commit = git(&home, &src, &["commit-tree", &root, "-m", label], None);
        git(&home, &src, &["update-ref", &format!("refs/heads/{label}"), &commit], None);
        let mut results = Vec::new();
        for (backend, g) in both(&home) {
            let dst = temp(&format!("backends-fetch-{label}-{backend}"));
            g.init_bare(&dst).unwrap();
            let want = format!("refs/heads/{label}:refs/toa/case");
            let r = g.fetch(&dst, &address, &[want.as_str()], false);
            let has = support::system_git(&home).object_size(&dst, &t).unwrap().is_some();
            let ref_set = support::system_git(&home).resolve_ref(&dst, "refs/toa/case").unwrap();
            if r.is_ok() != *accepted {
                failures.push(format!("{label} {backend}: {r:?}"));
            }
            if r.is_err() && (has || ref_set.is_some()) {
                failures.push(format!("{label} {backend}: refused, but left the objects or the ref behind"));
            }
            if r.is_ok() && (!has || ref_set.as_deref() != Some(commit.as_str())) {
                failures.push(format!("{label} {backend}: accepted, but the objects or the ref are missing"));
            }
            results.push(r.is_ok());
            eprintln!(
                "{label} {backend}: {}",
                match &r {
                    Ok(()) => "accepted".to_string(),
                    Err(e) => e.to_string(),
                }
            );
        }
        if results[0] != results[1] {
            failures.push(format!("{label}: the two gits disagree"));
        }
    }
    // a tree by its id, shallow: the history is not needed
    for (backend, g) in both(&home) {
        let dst = temp(&format!("backends-fetch-shallow-{backend}"));
        g.init_bare(&dst).unwrap();
        let want = "refs/heads/good:refs/toa/good";
        if let Err(e) = g.fetch(&dst, &address, &[want], true) {
            failures.push(format!("shallow {backend}: {e}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Repositories the built-in git cannot read completely: refs in the reftable format, and
/// partial clones. It says that git is needed, and never answers "not found".
#[test]
fn the_builtin_git_asks_for_git_where_it_cannot_read() {
    let home = temp("backends-needs-home");
    let b = support::builtin_git();
    let sys = support::system_git(&home);
    let (major, minor) = sys.version().unwrap();

    if (major, minor) >= (2, 45) {
        let r = temp("backends-reftable");
        git(&home, &r, &["init", "--quiet", "--ref-format=reftable", "--object-format=sha1"], None);
        write(&r.join("a.txt"), b"a\n");
        let c = commit_all(&home, &r, "one");
        assert_eq!(sys.resolve_commit(&r, "HEAD").unwrap().as_deref(), Some(c.as_str()));
        for res in [
            b.resolve_commit(&r, "HEAD").map(|_| ()),
            b.list_tree(&r, &c, 10).map(|_| ()),
            b.has_commits(&r).map(|_| ()),
        ] {
            assert!(matches!(res, Err(GitError::NeedsGit(_))), "reftable: {res:?}");
        }
        match library::publication_head(&b, &r, "main") {
            Err(library::HeadError::Git(GitError::NeedsGit(_))) => {}
            other => panic!("reftable, publication head: {other:?}"),
        }
    } else {
        eprintln!("git {major}.{minor} cannot make reftable repositories; not tested here");
    }

    // a partial clone: only trees, the files fetched when needed
    let src = temp("backends-partial-src");
    library(&home, &src);
    write(&src.join("a.txt"), b"a\n");
    commit_all(&home, &src, "one");
    git(&home, &src, &["config", "uploadpack.allowFilter", "true"], None);
    let dst = temp("backends-partial");
    let url = format!("file://{}", src.to_string_lossy().replace('\\', "/"));
    let url = if url.starts_with("file:///") { url } else { url.replacen("file://", "file:///", 1) };
    git(&home, &dst, &["clone", "--quiet", "--filter=blob:none", "--no-checkout", &url, "."], None);
    let res = b.list_tree(&dst, "HEAD", 10);
    assert!(matches!(res, Err(GitError::NeedsGit(ref m)) if m.contains("partial")), "partial clone: {res:?}");
    assert!(sys.list_tree(&dst, "HEAD", 10).is_ok());
}

/// Sealing with the built-in git: the same names, and the user told that the second
/// computation by git was not made.
#[test]
fn sealing_without_the_system_git() {
    let home = temp("backends-seal-home");
    let mut names = Vec::new();
    for (label, g) in both(&home) {
        let lib = temp(&format!("backends-seal-{label}"));
        library(&home, &lib);
        write(
            &lib.join("ifc/x/ifc_draft-a/toa.yaml"),
            b"type: ifc\nlicense: MIT\nname: x\ntitle: X\npins:\n- {name: a}\n",
        );
        let opts = seal::SealOptions { dry_run: false, lookup: Vec::new(), workspace: None };
        let r = seal::seal(g.as_ref(), &lib, &opts).unwrap();
        let skipped = r.notices.iter().any(|n| n.contains("second computation"));
        assert_eq!(skipped, label == "builtin", "{label}: {:?}", r.notices);
        names.push(r.objects[0].1.clone());
    }
    assert_eq!(names[0], names[1]);
}
