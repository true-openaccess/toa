//! Publishing (7.2, 5.3) in a library made for the test: source lists tidied, the library list
//! written again, the commit checked by 6.4.

#[path = "support.rs"]
mod support;

use std::path::Path;

use support::{commit_all, git, library, temp, write};
use toa_core::source::{self, Kind, Source};
use toa_repo::publish::{self, Options};
use toa_repo::seal::{self, SealOptions};

fn dir_named(parent: &Path, prefix: &str) -> String {
    std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .find(|n| n.starts_with(prefix) && !n.contains('.'))
        .unwrap()
}

fn objects_of(path: &Path) -> Vec<source::ObjectLine> {
    let b = std::fs::read(path).unwrap();
    let kind = if path.ends_with(source::LIBRARY_LIST) { Kind::Library } else { Kind::Other };
    let r = source::read(&b, true, kind, source::MAX_VOLUME, "");
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    source::merged_objects(&r.lines)
}

const ADDRESS: &str = "https://example.org/demo/lib.git";
const OTHER: &str = "ckt_0123456789abcdef0123456789abcdef01234567";
const OTHER_IP: &str = "ifc_0123456789abcdef0123456789abcdef01234567";

#[test]
fn publish_tidies_lists_and_the_commit_passes_the_check() {
    let home = temp("pub-home");
    let lib = temp("pub-lib");
    library(&home, &lib);
    commit_all(&home, &lib, "empty library");
    git(&home, &lib, &["checkout", "--quiet", "-b", "work"], None);
    let g = support::system_git(&home);
    let reader = support::reader(&home);
    let opts = SealOptions { dry_run: false, lookup: Vec::new(), workspace: None };

    write(
        &lib.join("ifc/x/ifc_draft-a/toa.yaml"),
        b"type: ifc\nlicense: MIT\nname: comp\ntitle: Comp\npins:\n- {name: a}\n",
    );
    let imp =
        format!("type: ckt\nlicense: MIT\nifc: ifc_draft-a\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\nuses: {{u: {{ifc: {OTHER_IP}, ckt: {OTHER}}}}}\n");
    write(&lib.join("ckt/comp/s.v1/ckt_draft-b/toa.yaml"), imp.as_bytes());
    write(&lib.join("ckt/comp/s.v1/ckt_draft-b/n.scs"), b"subckt x a\nends x\n");

    seal::seal(&g, &lib, &opts).unwrap();
    let ip = dir_named(&lib.join("ifc/x"), "ifc_");
    let v1 = dir_named(&lib.join("ckt/comp/s.v1"), "ckt_");
    let sealed = commit_all(&home, &lib, "seal");

    // before publishing, the check asks for toa publish
    let r = toa_repo::library::check(reader.as_ref(), &lib, &sealed, None, &Default::default()).unwrap();
    assert!(r.problems.iter().any(|p| p.message.contains("run `toa publish`")), "{:?}", r.problems);

    // a dry run changes nothing
    let o = Options {
        to: Some("https://me:secret@Example.org/demo/lib.git".into()),
        branch: "main".into(),
        dry_run: true,
        ..Default::default()
    };
    let before = std::fs::read(lib.join(format!("ckt/comp/s.v1/{v1}.source.jsonl"))).unwrap();
    let dry = publish::publish(&g, &lib, &o).unwrap();
    assert!(!dry.changed.is_empty());
    assert_eq!(std::fs::read(lib.join(format!("ckt/comp/s.v1/{v1}.source.jsonl"))).unwrap(), before);

    let o = Options { dry_run: false, commit: true, ..o };
    let r = publish::publish(&g, &lib, &o).unwrap();
    assert_eq!(r.address.as_deref(), Some(ADDRESS), "no user name or password in the address");
    assert_eq!(r.objects, [v1.clone(), ip.clone()], "sorted by name");
    assert_eq!(r.name_only, [OTHER, OTHER_IP], "an object with no address left keeps only its name");
    assert!(r.check.is_empty(), "the library check of the commit: {:?}", r.check);
    let commit = r.commit.clone().unwrap();

    // the list beside the implementation: itself first, this library's address and path, no
    // local paths; the IP with its label; the object from elsewhere with its name only
    let list = objects_of(&lib.join(format!("ckt/comp/s.v1/{v1}.source.jsonl")));
    assert_eq!(list[0].object, v1);
    assert_eq!(
        list[0].sources,
        [Source::Git { git: ADDRESS.into(), commit: None, path: Some(format!("ckt/comp/s.v1/{v1}")) }]
    );
    assert_eq!(list[0].label.as_deref(), Some("comp/sx.g1"));
    let ip_line = list.iter().find(|o| o.object == ip).unwrap();
    assert_eq!(ip_line.label.as_deref(), Some("comp"));
    let other = list.iter().find(|o| o.object == OTHER).unwrap();
    assert!(other.sources.is_empty() && other.refs.is_none() && other.label.is_none());
    // the library list
    let all = objects_of(&lib.join("library.source.jsonl"));
    assert_eq!(all.iter().map(|o| o.object.as_str()).collect::<Vec<_>>(), [v1.as_str(), ip.as_str()]);
    assert_eq!(all[0].sources, [Source::Local(format!("ckt/comp/s.v1/{v1}"))]);
    // the group: the IP name of the recommended layout, the second level of the path
    assert_eq!(all[0].group.as_deref(), Some("comp"));
    assert_eq!(all[1].group.as_deref(), Some("x"));
    // latest (5.2): each is the first of its series here, so marked by default
    assert!(all[0].latest && all[1].latest);
    assert_eq!(r.latest, [(None, v1.clone()), (None, ip.clone())]);

    // merged: now published. A next version lists v1 with the commit that first held it
    git(&home, &lib, &["update-ref", "refs/heads/main", &commit], None);
    let v2_text = format!(
        "type: ckt\nlicense: MIT\nifc: {ip}\nbase: [{v1}]\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n"
    );
    write(&lib.join("ckt/comp/s.v2/ckt_draft-c/toa.yaml"), v2_text.as_bytes());
    write(&lib.join("ckt/comp/s.v2/ckt_draft-c/n.scs"), b"subckt x a\nends x\n");

    seal::seal(&g, &lib, &opts).unwrap();
    let v2 = dir_named(&lib.join("ckt/comp/s.v2"), "ckt_");
    commit_all(&home, &lib, "v2");
    // --keep-latest leaves the marks as they are
    let kept =
        publish::publish(&g, &lib, &Options { keep_latest: true, dry_run: true, commit: false, ..o.clone() }).unwrap();
    assert!(kept.latest.is_empty());
    let r = publish::publish(&g, &lib, &o).unwrap();
    assert_eq!(r.objects, std::slice::from_ref(&v2));
    // the mark moves from v1 to the next generation of its series
    assert_eq!(r.latest, [(Some(v1.clone()), v2.clone())]);
    let all = objects_of(&lib.join("library.source.jsonl"));
    let marked: Vec<&str> = all.iter().filter(|o| o.latest).map(|o| o.object.as_str()).collect();
    assert_eq!(marked.len(), 2);
    assert!(marked.contains(&v2.as_str()) && marked.contains(&ip.as_str()), "{marked:?}");
    assert!(r.check.is_empty(), "{:?}", r.check);
    let list = objects_of(&lib.join(format!("ckt/comp/s.v2/{v2}.source.jsonl")));
    let v1_line = list.iter().find(|x| x.object == v1).unwrap();
    assert_eq!(
        v1_line.sources,
        [Source::Git { git: ADDRESS.into(), commit: Some(sealed.clone()), path: Some(format!("ckt/comp/s.v1/{v1}")) }],
        "the earliest commit along first parents that holds it (5.1)"
    );
    // the list beside v1 was published already and is not changed (5.3)
    assert!(!r.changed.iter().any(|c| c.contains(&format!("{v1}.source"))));
}

#[test]
fn drafts_stop_publishing() {
    let home = temp("pub-draft-home");
    let lib = temp("pub-draft-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    write(&lib.join("doc/a/doc_draft-a/toa.yaml"), b"type: doc\nlicense: MIT\n");
    write(&lib.join("doc/a/doc_draft-a/README.md"), b"hello\n");

    commit_all(&home, &lib, "draft");
    match publish::publish(&g, &lib, &Options { branch: "main".into(), ..Default::default() }) {
        Err(seal::Stop::Problems(ps)) => assert_eq!(ps[0].code, "E_PUBLISH_DRAFT"),
        other => panic!("{other:?}"),
    }
}

/// A library with one sealed document, committed on `main`, for publishing with `--commit`.
fn one_document(label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let home = temp(&format!("{label}-home"));
    let lib = temp(&format!("{label}-lib"));
    library(&home, &lib);
    let g = support::system_git(&home);
    write(&lib.join("doc/a/doc_draft-a/toa.yaml"), b"type: doc\nlicense: MIT\n");
    write(&lib.join("doc/a/doc_draft-a/README.md"), b"# A\n");
    seal::seal(&g, &lib, &SealOptions { dry_run: false, lookup: Vec::new(), workspace: None }).unwrap();
    commit_all(&home, &lib, "seal");
    (home, lib)
}

#[test]
fn the_commit_is_signed_as_git_is_set() {
    // 6.3: the commit that publishes objects should be signed, by the user's git settings
    let keygen = std::process::Command::new("ssh-keygen")
        .arg("-?")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if keygen.is_err() {
        eprintln!("skipped: no ssh-keygen on this machine");
        return;
    }
    let (home, lib) = one_document("pub-sign");
    let key = home.join("signing-key");
    git(&home, &lib, &["config", "gpg.format", "ssh"], None);
    // asked for, but there is no key: the commit fails and nothing is committed
    let before = git(&home, &lib, &["rev-parse", "HEAD"], None);
    let o = Options { branch: "main".into(), to: Some(ADDRESS.into()), commit: true, sign: true, ..Default::default() };
    let g = support::system_git(&home);
    assert!(publish::publish(&g, &lib, &o).is_err());
    assert_eq!(git(&home, &lib, &["rev-parse", "HEAD"], None), before);

    let made = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "test", "-f"])
        .arg(&key)
        .stdin(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(made.success());
    git(&home, &lib, &["config", "user.signingkey", &key.to_string_lossy()], None);
    git(&home, &lib, &["config", "commit.gpgsign", "true"], None);
    // a second document, sealed and committed by the user's git, which now signs too
    write(&lib.join("doc/b/doc_draft-b/toa.yaml"), b"type: doc\nlicense: MIT\n");
    write(&lib.join("doc/b/doc_draft-b/README.md"), b"# B\n");
    seal::seal(&g, &lib, &SealOptions { dry_run: false, lookup: Vec::new(), workspace: None }).unwrap();
    commit_all(&home, &lib, "seal b");
    let o = Options { sign: false, ..o };
    let r = publish::publish(&g, &lib, &o).unwrap();
    assert_eq!(r.signature, Some(toa_repo::git::SignatureFormat::Ssh), "{:?}", r.notices);
    assert!(!r.notices.iter().any(|n| n.contains("not signed")), "{:?}", r.notices);
    assert!(r.check.is_empty(), "{:?}", r.check);
    let c = r.commit.unwrap();
    assert!(git(&home, &lib, &["cat-file", "commit", &c], None).contains("-----BEGIN SSH SIGNATURE-----"));
    // the built-in git reads the same
    use toa_repo::git::Git;
    assert_eq!(support::builtin_git().commit_signature(&lib, &c).unwrap(), Some(toa_repo::git::SignatureFormat::Ssh));
    assert!(support::builtin_git().signs_commits(&lib).unwrap());

    // the index records the kind of signature of the commit that first held the object (8.3)
    let doc = dir_named(&lib.join("doc/b"), "doc_");
    let unsigned = dir_named(&lib.join("doc/a"), "doc_");
    git(&home, &lib, &["update-ref", "refs/heads/main", &c], None);
    let ws = temp("pub-sign-ws");
    let list = format!(
        "{{\"toa\": \"source/1\"}}\n{{\"include\": \"library.source.jsonl\", \"group\": \"mine\", \"sources\": [{{\"local\": \"{}\"}}]}}\n",
        lib.to_string_lossy().replace('\\', "/")
    );
    write(&ws.join("workspace.source.jsonl"), list.as_bytes());
    let o = toa_repo::index::Options {
        workspace: ws.join("workspace.source.jsonl"),
        out: ws.join("index"),
        cache: ws.join("cache"),
        offline: true,
        tool: "toa test".into(),
        ..Default::default()
    };
    toa_repo::index::build(support::reader(&home).as_ref(), &o).unwrap();
    let p = ws.join("index/objects").join(&doc[4..6]).join(format!("{doc}.json"));
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    assert_eq!(v["locations"][0]["first_signature"], "ssh", "{v}");
    assert_eq!(v["first"]["signature"], "ssh");
    // the commit that first held the other was not signed
    let p = ws.join("index/objects").join(&unsigned[4..6]).join(format!("{unsigned}.json"));
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    assert_eq!(v["locations"][0]["first_signature"], serde_json::Value::Null);
}

#[test]
fn an_unsigned_commit_is_told() {
    let (home, lib) = one_document("pub-unsigned");
    let g = support::system_git(&home);
    let o = Options { branch: "main".into(), to: Some(ADDRESS.into()), commit: true, ..Default::default() };
    let r = publish::publish(&g, &lib, &o).unwrap();
    assert!(r.commit.is_some() && r.signature.is_none());
    assert!(r.notices.iter().any(|n| n.contains("not signed") && n.contains("--sign")), "{:?}", r.notices);
}

#[test]
fn a_new_local_library_is_published_in_one_step_and_marked_later() {
    // a library with no remote: main is the publication branch (6.3). One toa publish --commit
    // puts the sealed objects, their lists and the library files in one commit that passes 6.4
    let home = temp("pub-new-home");
    let lib = temp("pub-new-lib");
    library(&home, &lib);
    write(&lib.join("library.yaml"), b"toa: library/1\nname: demo\n");
    let g = support::system_git(&home);
    let seal_opts = SealOptions { dry_run: false, lookup: Vec::new(), workspace: None };
    write(
        &lib.join("ifc/x/ifc_draft-a/toa.yaml"),
        b"type: ifc\nlicense: MIT\nname: comp\ntitle: Comp\npins:\n- {name: a}\n",
    );
    write(
        &lib.join("ckt/comp/s.g1/ckt_draft-b/toa.yaml"),
        b"type: ckt\nlicense: MIT\nifc: ifc_draft-a\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n",
    );
    write(&lib.join("ckt/comp/s.g1/ckt_draft-b/n.scs"), b"subckt x a\nends x\n");
    write(&lib.join("ckt/comp/s.g1/ckt_draft-b.notes/20261124-a.md"), b"a note\n");
    // another circuit that only shares the name: a series of its own (3.4)
    write(
        &lib.join("ckt/comp/t.g1/ckt_draft-d/toa.yaml"),
        b"type: ckt\nlicense: MIT\nifc: ifc_draft-a\nname: sx\nform: ideal\nfiles:\n  netlist: [t.scs]\n",
    );
    write(&lib.join("ckt/comp/t.g1/ckt_draft-d/t.scs"), b"subckt x a\nends x\n");
    seal::seal(&g, &lib, &seal_opts).unwrap();
    let other = dir_named(&lib.join("ckt/comp/t.g1"), "ckt_");
    let ip = dir_named(&lib.join("ifc/x"), "ifc_");
    let v1 = dir_named(&lib.join("ckt/comp/s.g1"), "ckt_");
    // as a library published before toa publish gave marks
    let o = Options {
        branch: "main".into(),
        to: Some(ADDRESS.into()),
        commit: true,
        keep_latest: true,
        ..Default::default()
    };
    let r = publish::publish(&g, &lib, &o).unwrap();
    assert!(r.check.is_empty(), "{:?}", r.check);
    assert!(
        r.committed.contains(&format!("ckt/comp/s.g1/{v1}")) && r.committed.contains(&".gitattributes".to_string())
    );
    assert_eq!(git(&home, &lib, &["status", "--porcelain"], None), "", "everything is committed");

    let v2_text = format!(
        "type: ckt\nlicense: MIT\nifc: {ip}\nbase: [{v1}]\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n"
    );
    write(&lib.join("ckt/comp/s.g2/ckt_draft-c/toa.yaml"), v2_text.as_bytes());
    write(&lib.join("ckt/comp/s.g2/ckt_draft-c/n.scs"), b"subckt x a\nends x\n");
    seal::seal(&g, &lib, &seal_opts).unwrap();
    let v2 = dir_named(&lib.join("ckt/comp/s.g2"), "ckt_");
    let r = publish::publish(&g, &lib, &o).unwrap();
    assert!(r.check.is_empty(), "{:?}", r.check);
    let marked = || -> Vec<String> {
        objects_of(&lib.join("library.source.jsonl")).into_iter().filter(|x| x.latest).map(|x| x.object).collect()
    };
    assert!(marked().is_empty());

    // toa latest: the marks given later; marking v2 takes the mark from v1, its ancestor
    use toa_repo::latest::{self, Options as LatestOptions};
    let lo = LatestOptions { branch: "main".into(), commit: true, ..Default::default() };
    let r = latest::latest(&g, &lib, &[ip.clone(), format!("ckt/comp/s.g1/{v1}")], &lo).unwrap();
    assert_eq!(r.marked, {
        let mut v = vec![ip.clone(), v1.clone()];
        v.sort();
        v
    });
    assert!(r.check.is_empty(), "{:?}", r.check);
    let r = latest::latest(&g, &lib, std::slice::from_ref(&v2), &lo).unwrap();
    assert_eq!((r.marked.clone(), r.unmarked.clone()), (vec![v2.clone()], vec![v1.clone()]));
    assert!(r.check.is_empty() && r.commit.is_some(), "{:?}", r.check);
    let mut now = marked();
    now.sort();
    let mut want = vec![ip.clone(), v2.clone()];
    want.sort();
    assert_eq!(now, want);
    // --off, as a dry run, changes nothing; a name not in the list is refused
    let r = latest::latest(
        &g,
        &lib,
        std::slice::from_ref(&v2),
        &LatestOptions { off: true, dry_run: true, commit: false, ..lo.clone() },
    )
    .unwrap();
    assert_eq!(r.unmarked, std::slice::from_ref(&v2));
    assert_eq!(marked().len(), 2);
    assert!(latest::latest(&g, &lib, &[OTHER.to_string()], &lo).is_err());
    // --all marks the newest of every series without a mark: here only the unrelated circuit;
    // two marked circuits that only share a name are no branches of one series
    let r = latest::latest(&g, &lib, &[], &LatestOptions { all: true, ..lo.clone() }).unwrap();
    assert_eq!(r.marked, std::slice::from_ref(&other));
    assert!(r.check.is_empty(), "{:?}", r.check);
}

#[test]
fn objects_committed_before_publishing_get_their_lists_tidied() {
    // sealed objects committed straight to main of a library with no remote: their lists still
    // hold local paths there; toa publish tidies them as if they were published now
    let home = temp("pub-early-home");
    let lib = temp("pub-early-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    write(
        &lib.join("ifc/x/ifc_draft-a/toa.yaml"),
        b"type: ifc\nlicense: MIT\nname: comp\ntitle: Comp\npins:\n- {name: a}\n",
    );
    write(
        &lib.join("ckt/comp/s.g1/ckt_draft-b/toa.yaml"),
        b"type: ckt\nlicense: MIT\nifc: ifc_draft-a\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n",
    );
    write(&lib.join("ckt/comp/s.g1/ckt_draft-b/n.scs"), b"subckt x a\nends x\n");
    seal::seal(&g, &lib, &SealOptions { dry_run: false, lookup: Vec::new(), workspace: None }).unwrap();
    let sealed = commit_all(&home, &lib, "sealed, committed before publishing");
    let v1 = dir_named(&lib.join("ckt/comp/s.g1"), "ckt_");
    let o = Options { branch: "main".into(), to: Some(ADDRESS.into()), commit: true, ..Default::default() };
    let r = publish::publish(&g, &lib, &o).unwrap();
    assert!(r.objects.is_empty(), "on main already");
    assert!(r.repaired.contains(&v1), "{:?}", r.repaired);
    assert!(r.check.is_empty(), "{:?}", r.check);
    let list = objects_of(&lib.join(format!("ckt/comp/s.g1/{v1}.source.jsonl")));
    assert_eq!(
        list[0].sources,
        [Source::Git { git: ADDRESS.into(), commit: Some(sealed), path: Some(format!("ckt/comp/s.g1/{v1}")) }]
    );
    // and they get the marks toa publish gives objects new to the library list
    assert!(objects_of(&lib.join("library.source.jsonl")).iter().all(|x| x.latest));
}
