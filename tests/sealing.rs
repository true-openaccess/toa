//! Sealing and unsealing (7.1) in a library made for the test, with the system git; then the
//! library check (6.4) and "published" (6.3) on what was sealed.

#[path = "support.rs"]
mod support;

use std::path::{Path, PathBuf};

use support::{commit_all, git, library, temp, write};
use toa_core::{history, name, source};
use toa_repo::seal::{self, SealOptions, Stop};

fn opts() -> SealOptions {
    SealOptions { dry_run: false, lookup: Vec::new(), workspace: None }
}

fn only_dir(parent: &Path, prefix: &str) -> String {
    let mut names: Vec<String> = std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(prefix) && !n.contains('.'))
        .collect();
    assert_eq!(names.len(), 1, "{names:?} in {}", parent.display());
    names.remove(0)
}

const IP: &str = "type: ifc\nlicense: MIT\nname: x\ntitle: X\npins:\n- {name: a}\n";
const IMP: &str = "type: ckt\nlicense: MIT\nifc: ifc_draft-a   # the IP it implements\nbase: []\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n";

#[test]
fn seal_unseal_and_seal_again() {
    let home = temp("seal-home");
    let lib = temp("seal-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    write(&lib.join("ifc/x/ifc_draft-a/toa.yaml"), IP.as_bytes());
    write(&lib.join("ckt/x/s.v1/ckt_draft-b/toa.yaml"), IMP.as_bytes());
    write(&lib.join("ckt/x/s.v1/ckt_draft-b/n.scs"), b"subckt x a\nends x\n");

    write(&lib.join("ckt/x/s.v1/ckt_draft-b/netlist.scs"), b"subckt x a b\nends x\n");
    write(&lib.join("ckt/x/s.v1/ckt_draft-b.notes/20261123-hello.md"), b"hello\n");

    // a dry run changes nothing
    let dry = seal::seal(&g, &lib, &SealOptions { dry_run: true, lookup: Vec::new(), workspace: None }).unwrap();
    assert_eq!(dry.objects.len(), 2);
    assert!(lib.join("ifc/x/ifc_draft-a").is_dir());

    let r = seal::seal(&g, &lib, &opts()).unwrap();
    assert_eq!(r.objects.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(), ["ifc_draft-a", "ckt_draft-b"]);
    assert!(r.notices.is_empty(), "{:?}", r.notices);
    assert_eq!(r.objects, dry.objects, "a dry run tells what sealing does");
    let ip = only_dir(&lib.join("ifc/x"), "ifc_");
    let imp = only_dir(&lib.join("ckt/x/s.v1"), "ckt_");
    assert!(name::is_object_name(&ip) && name::is_object_name(&imp));

    // only the characters of the value changed; the comment stays; base: [] is gone
    let text = std::fs::read_to_string(lib.join("ckt/x/s.v1").join(&imp).join("toa.yaml")).unwrap();
    assert_eq!(text, IMP.replace("ifc_draft-a", &ip).replace("base: []\n", ""));
    // the notes moved with it, and its source list names the IP with a local path
    assert!(lib.join(format!("ckt/x/s.v1/{imp}.notes/20261123-hello.md")).is_file());
    let list = std::fs::read(lib.join(format!("ckt/x/s.v1/{imp}.source.jsonl"))).unwrap();
    let read = source::read(&list, false, source::Kind::Other, source::MAX_SIDECAR_LIST, "");
    let objects = source::merged_objects(&read.lines);
    assert_eq!(objects[0].object, imp);
    assert_eq!(objects[1].object, ip);
    assert_eq!(objects[1].sources, [source::Source::Local(format!("ifc/x/{ip}"))]);
    // every object verifies from its files
    for d in [lib.join("ifc/x").join(&ip), lib.join("ckt/x/s.v1").join(&imp)] {
        let v = toa_repo::fs::verify_dir(&d).unwrap();
        assert!(v.problems.is_empty(), "{:?}", v.problems);
    }

    // a next version: base gives history.json
    let v2 = format!(
        "type: ckt\nlicense: MIT\nifc: {ip}\nbase: [{imp}]\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n"
    );
    write(&lib.join("ckt/x/s.v2/ckt_draft-c/toa.yaml"), v2.as_bytes());
    write(&lib.join("ckt/x/s.v2/ckt_draft-c/n.scs"), b"subckt x a\nends x\n");

    write(&lib.join("ckt/x/s.v2/ckt_draft-c/netlist.scs"), b"subckt x a b c\nends x\n");
    seal::seal(&g, &lib, &opts()).unwrap();
    let imp2 = only_dir(&lib.join("ckt/x/s.v2"), "ckt_");
    let h = std::fs::read(lib.join("ckt/x/s.v2").join(&imp2).join("history.json")).unwrap();
    assert_eq!(h, history::write(&history::History { history: vec![imp.clone()], generation: 2 }));

    // the library check: only the local paths of the source lists beside objects remain, which
    // publishing removes (5.3)
    let head = commit_all(&home, &lib, "seal");
    let report =
        toa_repo::library::check(support::reader(&home).as_ref(), &lib, &head, None, &Default::default()).unwrap();
    let left: Vec<_> = report.problems.iter().filter(|p| p.code != "E_SOURCE_LIST").collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(report.problems.iter().all(|p| p.message.contains("no local sources")), "{:?}", report.problems);
    assert_eq!(report.objects, 3);

    // published now (the local branch main decides, there is no remote): no unsealing
    match seal::unseal(support::reader(&home).as_ref(), &lib, &ip, "main") {
        Err(Stop::Problems(ps)) => assert_eq!(ps[0].code, "E_UNSEAL_PUBLISHED"),
        other => panic!("{other:?}"),
    }
    // a branch that does not exist in a repository with commits is an error, not "nothing"
    assert!(matches!(seal::unseal(support::reader(&home).as_ref(), &lib, &ip, "release"), Err(Stop::Git(_))));

    // on a branch of its own, main still at the empty commit: unseal the IP, and with it the
    // implementations that refer to it, level by level up
    git(&home, &lib, &["checkout", "--quiet", "-b", "work"], None);
    let empty = git(&home, &lib, &["commit-tree", "4b825dc642cb6eb9a060e54bf8d69288fbee4904", "-m", "empty"], None);
    git(&home, &lib, &["update-ref", "refs/heads/main", &empty], None);
    let u = seal::unseal(support::reader(&home).as_ref(), &lib, &ip, "main").unwrap();
    let names: Vec<&str> = u.drafts.iter().map(|d| d.1.as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    let ip_draft = seal::draft_name_for(&ip);
    assert!(lib.join("ifc/x").join(&ip_draft).is_dir());
    let text =
        std::fs::read_to_string(lib.join("ckt/x/s.v1").join(seal::draft_name_for(&imp)).join("toa.yaml")).unwrap();
    assert!(text.contains(&format!("ifc: {ip_draft}   # the IP it implements")), "{text}");

    // sealing again gives the same names
    seal::seal(&g, &lib, &opts()).unwrap();
    assert_eq!(only_dir(&lib.join("ifc/x"), "ifc_"), ip);
    assert_eq!(only_dir(&lib.join("ckt/x/s.v1"), "ckt_"), imp);
    assert_eq!(only_dir(&lib.join("ckt/x/s.v2"), "ckt_"), imp2);
}

#[test]
fn a_failure_restores_everything() {
    let home = temp("restore-home");
    let lib = temp("restore-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    write(&lib.join(".gitignore"), b"*.log\n");
    write(&lib.join("doc/a/doc_draft-a/toa.yaml"), b"type: doc\nlicense: MIT\nrefs: [ifc_draft-z]\n");
    write(&lib.join("doc/a/doc_draft-a/README.md"), b"hello\n");

    write(&lib.join("ifc/z/ifc_draft-z/toa.yaml"), b"type: ifc\nlicense: MIT\nname: z\ntitle: Z\npins:\n- {name: a}\n");
    // git ignores this file, so git's tree differs from the files (7.1 step 3.4); the interface
    // is sealed first, then the document fails, and both are restored
    write(&lib.join("doc/a/doc_draft-a/run.log"), b"x\n");
    let before = std::fs::read(lib.join("doc/a/doc_draft-a/toa.yaml")).unwrap();
    match seal::seal(&g, &lib, &opts()) {
        Err(Stop::Problems(ps)) => {
            assert_eq!(ps[0].code, "E_SEAL_GIT_HASH", "{ps:?}");
            assert!(ps[0].message.contains("run.log"), "{}", ps[0].message);
        }
        other => panic!("{other:?}"),
    }
    assert!(lib.join("doc/a/doc_draft-a/run.log").is_file());
    assert!(lib.join("ifc/z/ifc_draft-z/toa.yaml").is_file());
    assert_eq!(std::fs::read(lib.join("doc/a/doc_draft-a/toa.yaml")).unwrap(), before);

    // with the stray file gone, the interface seals first, then the document that names it
    std::fs::remove_file(lib.join("doc/a/doc_draft-a/run.log")).unwrap();
    let r = seal::seal(&g, &lib, &opts()).unwrap();
    assert_eq!(r.objects[0].0, "ifc_draft-z");
    assert_eq!(r.objects[1].0, "doc_draft-a");
}

#[test]
fn errors_stop_before_anything_changes() {
    let home = temp("stop-home");
    let lib = temp("stop-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    // a cycle
    write(&lib.join("doc/a/doc_draft-a/toa.yaml"), b"type: doc\nlicense: MIT\nrefs: [doc_draft-b]\n");
    write(&lib.join("doc/a/doc_draft-a/README.md"), b"hello\n");

    write(&lib.join("doc/b/doc_draft-b/toa.yaml"), b"type: doc\nlicense: MIT\nrefs: [doc_draft-a]\n");
    write(&lib.join("doc/b/doc_draft-b/README.md"), b"hello\n");

    match seal::seal(&g, &lib, &opts()) {
        Err(Stop::Problems(ps)) => assert_eq!(ps[0].code, "E_SEAL_CYCLE"),
        other => panic!("{other:?}"),
    }
    std::fs::remove_dir_all(lib.join("doc/b")).unwrap();
    // a changed object, and a draft naming a draft that is not there
    write(&lib.join("doc/c/doc_ea44863a8d2f242bc651caf709d45a27e7531d9d/toa.yaml"), b"type: doc\nlicense: MIT\n");
    match seal::seal(&g, &lib, &opts()) {
        Err(Stop::Problems(ps)) => {
            let codes: Vec<_> = ps.iter().map(|p| p.code).collect();
            assert!(codes.contains(&"E_SEAL_CHANGED") && codes.contains(&"E_SEAL_UNKNOWN_DRAFT"), "{codes:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(lib.join("doc/a/doc_draft-a").is_dir());
}

#[test]
fn outside_a_repository_the_user_is_told() {
    let home = temp("outside-home");
    let dir = temp("outside-dir");
    let g = support::system_git(&home);
    write(&dir.join("doc_draft-x/toa.yaml"), b"type: doc\nlicense: MIT\n");
    write(&dir.join("doc_draft-x/README.md"), b"hello\n");
    let r = seal::seal(&g, &dir, &opts()).unwrap();
    assert_eq!(r.objects[0].1, "doc_ea44863a8d2f242bc651caf709d45a27e7531d9d");
    assert!(r.notices[0].contains("not in a git repository"));
}

/// The directory as a user may spell it otherwise than git does: on Windows its 8.3 short name
/// (the temporary directory of GitHub's Windows machines is under `C:\Users\RUNNER~1`), elsewhere
/// a symbolic link to it (macOS's `/var` is `/private/var`). `None` when there is no other
/// spelling (8.3 names turned off for the volume).
fn other_spelling(dir: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg("(New-Object -ComObject Scripting.FileSystemObject).GetFolder($env:TOA_TEST_DIR).ShortPath")
            .env("TOA_TEST_DIR", dir)
            .output()
            .ok()?;
        let short = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!short.is_empty() && !short.eq_ignore_ascii_case(&dir.to_string_lossy())).then(|| PathBuf::from(short))
    }
    #[cfg(unix)]
    {
        let link = dir.with_file_name(format!("{}-link", dir.file_name()?.to_string_lossy()));
        std::os::unix::fs::symlink(dir, &link).ok()?;
        Some(link)
    }
}

#[test]
fn a_path_spelled_otherwise_than_git_does() {
    let home = temp("spelling-home");
    let lib = temp("spelling-library");
    library(&home, &lib);
    let g = support::system_git(&home);
    write(&lib.join("ifc/x/ifc_draft-a/toa.yaml"), IP.as_bytes());
    write(&lib.join("ckt/x/s.v1/ckt_draft-b/toa.yaml"), IMP.as_bytes());
    write(&lib.join("ckt/x/s.v1/ckt_draft-b/n.scs"), b"subckt x a\nends x\n");

    let Some(other) = other_spelling(&lib) else {
        eprintln!("skipped: no other spelling of {}", lib.display());
        return;
    };
    assert_ne!(other, lib);

    // sealing through it: git's second computation is made, and local paths are relative
    let r = seal::seal(&g, &other, &opts()).unwrap();
    assert_eq!(r.objects.len(), 2);
    assert!(r.notices.is_empty(), "{:?}", r.notices);
    let ip = only_dir(&lib.join("ifc/x"), "ifc_");
    let imp = only_dir(&lib.join("ckt/x/s.v1"), "ckt_");
    let list = std::fs::read(lib.join(format!("ckt/x/s.v1/{imp}.source.jsonl"))).unwrap();
    let read = source::read(&list, false, source::Kind::Other, source::MAX_SIDECAR_LIST, "");
    assert_eq!(source::merged_objects(&read.lines)[1].sources, [source::Source::Local(format!("ifc/x/{ip}"))]);

    // unsealing through it, and a draft below the root named by itself
    let u = seal::unseal(support::reader(&home).as_ref(), &other, &ip, "main").unwrap();
    assert_eq!(u.drafts.len(), 2, "{:?}", u.drafts);
    let r = seal::seal(&g, &other.join("ifc").join("x"), &opts()).unwrap();
    assert_eq!(r.objects.len(), 1);
    assert!(r.notices.is_empty(), "{:?}", r.notices);
}

#[test]
fn a_dry_run_checks_the_netlist_of_a_circuit_draft() {
    // what the type specification of ckt asks of the netlist (3.5) is found before anything is
    // sealed, so a draft can be checked with --dry-run while it is written
    let home = temp("seal-netlist-home");
    let lib = temp("seal-netlist-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    write(&lib.join("ifc/x/ifc_draft-a/toa.yaml"), IP.as_bytes());
    write(&lib.join("ckt/x/s.v1/ckt_draft-b/toa.yaml"), IMP.as_bytes());
    write(
        &lib.join("ckt/x/s.v1/ckt_draft-b/n.scs"),
        b"include \"/pdk/models.scs\" section=tt\nmodel nch bsim4 type=n\nsubckt x a\nends x\n",
    );
    let dry = SealOptions { dry_run: true, lookup: Vec::new(), workspace: None };
    match seal::seal(&g, &lib, &dry) {
        Err(Stop::Problems(ps)) => {
            let netlist: Vec<_> = ps.iter().filter(|p| p.code == "E_NETLIST").collect();
            assert_eq!(netlist.len(), 2, "{ps:?}");
            assert!(netlist[0].path.ends_with("ckt_draft-b/n.scs:1"), "{:?}", netlist[0]);
            assert!(netlist[1].path.ends_with("ckt_draft-b/n.scs:2"), "{:?}", netlist[1]);
        }
        other => panic!("{other:?}"),
    }
    assert!(lib.join("ckt/x/s.v1/ckt_draft-b").is_dir() && lib.join("ifc/x/ifc_draft-a").is_dir());
}

#[test]
fn the_same_files_without_a_declared_lineage_are_hinted_at() {
    // 8.3: a hint for people, not an error; within the objects found on this machine
    let home = temp("same-home");
    let lib = temp("same-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    let net: String = (0..40).map(|i| format!("R{i} a b 1k\n")).collect::<String>() + "subckt x a\nends x\n";
    write(&lib.join("ifc/x/ifc_draft-a/toa.yaml"), IP.as_bytes());
    let by = |who: &str, name: &str| {
        format!("type: ckt\nlicense: MIT\nauthor: {who}\nifc: ifc_draft-a\nname: {name}\nform: ideal\nfiles:\n  netlist: [n.scs]\n")
    };
    write(&lib.join("ckt/x/alice/ckt_draft-b/toa.yaml"), by("Alice", "alice").as_bytes());
    write(&lib.join("ckt/x/alice/ckt_draft-b/n.scs"), net.as_bytes());
    seal::seal(&g, &lib, &opts()).unwrap();
    commit_all(&home, &lib, "alice");
    let ip = only_dir(&lib.join("ifc/x"), "ifc_");
    let alice = only_dir(&lib.join("ckt/x/alice"), "ckt_");

    // Bob's circuit has Alice's netlist, and does not name her circuit in base
    write(&lib.join("ckt/x/bob/ckt_draft-c/toa.yaml"), by("Bob", "bob").replace("ifc_draft-a", &ip).as_bytes());
    write(&lib.join("ckt/x/bob/ckt_draft-c/n.scs"), net.as_bytes());
    seal::seal(&g, &lib, &opts()).unwrap();
    let head = commit_all(&home, &lib, "bob");
    let bob = only_dir(&lib.join("ckt/x/bob"), "ckt_");

    let reader = support::reader(&home);
    let mut found = toa_repo::same::Found::default();
    toa_repo::same::read_library(reader.as_ref(), &lib, &head, "this library", &mut found);
    let hints = toa_repo::same::hints(&found);
    assert_eq!(hints.len(), 1, "{hints:?}");
    assert_eq!(hints[0].code, "I_SAME_FILES");
    assert_eq!(hints[0].path, format!("ckt/x/bob/{bob}"));
    assert!(hints[0].message.contains(&alice) && hints[0].message.contains("n.scs"), "{}", hints[0].message);
    assert_eq!(hints[0].level, toa_core::problem::Level::Info, "never an error");

    // the index computes the same with the same code, and writes both sides into the objects
    let ws = temp("same-ws");
    let list = format!(
        "{{\"toa\": \"source/1\"}}\n{{\"include\": \"library.source.jsonl\", \"group\": \"mine\", \"sources\": [{{\"local\": \"{}\"}}]}}\n",
        lib.to_string_lossy().replace('\\', "/")
    );
    write(&ws.join("workspace.source.jsonl"), list.as_bytes());
    let out = ws.join("index");
    let o = toa_repo::index::Options {
        workspace: ws.join("workspace.source.jsonl"),
        out: out.clone(),
        cache: ws.join("cache"),
        offline: true,
        tool: "toa test".into(),
        ..Default::default()
    };
    toa_repo::index::build(reader.as_ref(), &o).unwrap();
    let object = |name: &str| -> serde_json::Value {
        let (_, h) = name.split_once('_').unwrap();
        serde_json::from_slice(&std::fs::read(out.join("objects").join(&h[..2]).join(format!("{name}.json"))).unwrap())
            .unwrap()
    };
    assert_eq!(
        object(&bob)["same_files"],
        serde_json::json!({"earlier": [{"object": alice, "author": "Alice", "basis": "library", "library": "mine",
            "files": [["n.scs", "n.scs"]]}], "later": [], "common": []})
    );
    assert_eq!(
        object(&alice)["same_files"],
        serde_json::json!({"earlier": [], "later": [{"object": bob, "author": "Bob", "basis": "library", "library": "mine",
            "files": [["n.scs", "n.scs"]]}], "common": []})
    );
    let root: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("index.json")).unwrap()).unwrap();
    assert_eq!(root["same_files"]["common_groups"], 5);
    assert_eq!(root["same_files"]["common_cap"], 200);

    // an incremental run keeps the order along first parents: Carol's later copy names both
    write(&lib.join("ckt/x/carol/ckt_draft-d/toa.yaml"), by("Carol", "carol").replace("ifc_draft-a", &ip).as_bytes());
    write(&lib.join("ckt/x/carol/ckt_draft-d/n.scs"), net.as_bytes());
    seal::seal(&g, &lib, &opts()).unwrap();
    commit_all(&home, &lib, "carol");
    let carol = only_dir(&lib.join("ckt/x/carol"), "ckt_");
    toa_repo::index::build(reader.as_ref(), &o).unwrap();
    let earlier: Vec<String> = object(&carol)["same_files"]["earlier"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["object"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(earlier, [alice.clone(), bob.clone()]);
    assert_eq!(object(&alice)["same_files"]["later"].as_array().unwrap().len(), 2);
}
