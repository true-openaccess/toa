//! Removing (6.5) with and without a removal record (4.2), and the status of a library (7.1,
//! 6.3), in a library made for the test.

#[path = "support.rs"]
mod support;

use std::path::{Path, PathBuf};

use serde_json::Value;
use support::{commit_all, git, library, temp, write};
use toa_core::source::{self, Kind, Line, Source};
use toa_repo::remove::{self, Options};
use toa_repo::seal::{self, SealOptions, Stop};
use toa_repo::status::{self, Item};
use toa_repo::{library as lib, publish};

const ADDRESS: &str = "https://example.org/demo/lib.git";
const ELSEWHERE: &str = "https://example.org/other/lib.git";

fn dir_named(parent: &Path, prefix: &str) -> String {
    std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .find(|n| n.starts_with(prefix) && !n.contains('.'))
        .unwrap()
}

struct Lib {
    home: PathBuf,
    dir: PathBuf,
    ip: String,
    v1: String,
    v2: String,
}

impl Lib {
    /// An IP, two versions of an implementation (the second based on the first) and a note on
    /// each implementation, sealed, published and committed on `main`.
    fn new(label: &str) -> Lib {
        let home = temp(&format!("{label}-home"));
        let dir = temp(&format!("{label}-lib"));
        library(&home, &dir);
        let g = support::system_git(&home);
        let opts = SealOptions { dry_run: false, lookup: Vec::new(), workspace: None };
        write(
            &dir.join("ifc/comp/ifc_draft-a/toa.yaml"),
            b"type: ifc\nlicense: MIT\nname: comp\ntitle: Comp\npins:\n- {name: a}\n",
        );
        write(
            &dir.join("ckt/comp/s.v1/ckt_draft-b/toa.yaml"),
            b"type: ckt\nlicense: MIT\nifc: ifc_draft-a\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n",
        );
        write(&dir.join("ckt/comp/s.v1/ckt_draft-b/n.scs"), b"subckt x a\nends x\n");

        write(&dir.join("ckt/comp/s.v1/ckt_draft-b.notes/20261123-a.md"), b"a note\n");
        seal::seal(&g, &dir, &opts).unwrap();
        let mut l = Lib {
            ip: dir_named(&dir.join("ifc/comp"), "ifc_"),
            v1: dir_named(&dir.join("ckt/comp/s.v1"), "ckt_"),
            v2: String::new(),
            home,
            dir,
        };
        l.publish("one");
        let v2 = format!(
            "type: ckt\nlicense: MIT\nifc: {}\nbase: [{}]\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n",
            l.ip, l.v1
        );
        write(&l.dir.join("ckt/comp/s.v2/ckt_draft-c/toa.yaml"), v2.as_bytes());
        write(&l.dir.join("ckt/comp/s.v2/ckt_draft-c/n.scs"), b"subckt x a\nends x\n");

        write(&l.dir.join("ckt/comp/s.v2/ckt_draft-c.notes/20261123-b.md"), b"another note\n");
        seal::seal(&g, &l.dir, &opts).unwrap();
        l.v2 = dir_named(&l.dir.join("ckt/comp/s.v2"), "ckt_");
        l.publish("two");
        l
    }

    /// Tidy by `toa publish`, commit, and run the library check against the commit before.
    fn publish(&self, msg: &str) -> String {
        let g = support::system_git(&self.home);
        let o = publish::Options { to: Some(ADDRESS.into()), branch: "main".into(), ..Default::default() };
        publish::publish(&g, &self.dir, &o).unwrap_or_else(|e| panic!("publishing: {e:?}"));
        let base = git(&self.home, &self.dir, &["rev-list", "--all", "--max-count=1"], None);
        let base = (!base.is_empty()).then_some(base);
        let commit = commit_all(&self.home, &self.dir, msg);
        let r = lib::check(self.reader().as_ref(), &self.dir, &commit, base.as_deref(), &Default::default()).unwrap();
        let errors: Vec<_> = r.problems.iter().filter(|p| p.is_error()).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        commit
    }

    fn reader(&self) -> Box<dyn toa_repo::git::Git> {
        support::reader(&self.home)
    }

    fn opts(&self) -> Options {
        Options { branch: "main".into(), to: Some(ADDRESS.into()), ..Default::default() }
    }

    fn status(&self) -> Vec<Item> {
        status::status(self.reader().as_ref(), &self.dir, "main").unwrap().items
    }

    fn library_line(&self, object: &str) -> Option<source::ObjectLine> {
        let b = std::fs::read(self.dir.join(source::LIBRARY_LIST)).unwrap();
        let r = source::read(&b, true, Kind::Library, source::MAX_VOLUME, "");
        source::merged_objects(&r.lines).into_iter().find(|l| l.object == object)
    }
}

fn problems(r: Result<remove::Removed, Stop>) -> Vec<String> {
    match r {
        Err(Stop::Problems(ps)) => ps.into_iter().map(|p| p.code.to_string()).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_removal_record_is_filled_from_git() {
    let l = Lib::new("rm-record");
    let head = git(&l.home, &l.dir, &["rev-parse", "HEAD"], None);
    let at = format!("ckt/comp/s.v2/{}", l.v2);

    // a git source of another place in the list beside the object: copied into the record
    let list = l.dir.join(format!("{at}.source.jsonl"));
    let b = std::fs::read(&list).unwrap();
    let mut lines = source::read(&b, false, Kind::Sidecar, source::MAX_SIDECAR_LIST, "").lines;
    for line in &mut lines {
        if let Line::Object(o) = line {
            if o.object == l.v2 {
                o.sources.push(Source::Git { git: ELSEWHERE.into(), commit: None, path: Some("x".into()) });
            }
        }
    }
    std::fs::write(&list, source::write(&lines)).unwrap();
    // a timestamp proof beside it (4.3): it goes with the object, as the notes do
    write(&l.dir.join(format!("{at}.stamps/20261124-btc.ots")), b"a proof");

    // a dry run changes nothing
    let dry = remove::remove(l.reader().as_ref(), &l.dir, &l.v2, &Options { record: true, dry_run: true, ..l.opts() })
        .unwrap();
    assert!(l.dir.join(&at).is_dir() && list.is_file());
    assert!(!l.dir.join(format!("{at}.removed.json")).exists());

    let o = Options { record: true, reason: Some("Superseded.".into()), ..l.opts() };
    let r = remove::remove(l.reader().as_ref(), &l.dir, &l.v2, &o).unwrap();
    assert_eq!(r.removed, dry.removed);
    assert_eq!(r.removed, [at.clone(), format!("{at}.notes"), format!("{at}.source.jsonl"), format!("{at}.stamps")]);
    assert_eq!(r.record.as_deref(), Some(format!("{at}.removed.json").as_str()));
    assert!(!l.dir.join(format!("{at}.stamps")).exists());
    assert!(!l.dir.join(&at).exists() && !list.exists() && !l.dir.join(format!("{at}.notes")).exists());
    assert!(!std::fs::read_dir(l.dir.join("ckt/comp/s.v2")).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".toa-removing")));

    let rec: Value = serde_json::from_slice(&std::fs::read(l.dir.join(format!("{at}.removed.json"))).unwrap()).unwrap();
    let keys: Vec<&str> = rec.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["toa", "object", "commit", "path", "license", "label", "base", "history", "reason", "sources"]);
    assert_eq!(rec["commit"], head.as_str());
    assert_eq!(rec["path"], at.as_str());
    assert_eq!(rec["license"], "MIT");
    assert_eq!(rec["label"], "comp/sx.g2");
    assert_eq!(rec["base"], serde_json::json!([l.v1]));
    assert_eq!(rec["history"], serde_json::json!([l.v1]));
    assert_eq!(rec["reason"], "Superseded.");
    // this library's own address is left out: commit and path say where it was here
    assert_eq!(rec["sources"], serde_json::json!([{"git": ELSEWHERE, "path": "x"}]));

    // published: the record is true by the library check, and the library list marks it removed
    l.publish("remove v2");
    let line = l.library_line(&l.v2).unwrap();
    assert!(line.removed);
    assert!(l.status().is_empty(), "{:?}", l.status());
}

#[test]
fn removing_without_a_record_and_the_status() {
    let l = Lib::new("rm-plain");
    assert!(l.status().is_empty(), "{:?}", l.status());
    let at = format!("ckt/comp/s.v1/{}", l.v1);

    // the notes and timestamp proofs kept; the object shown as gone until committed, its line
    // gone after publishing, and the library check content with what stays
    write(&l.dir.join(format!("{at}.stamps/20261124-btc.ots")), b"a proof");
    let r = remove::remove(l.reader().as_ref(), &l.dir, &l.v1, &Options { keep_notes: true, ..l.opts() }).unwrap();
    assert_eq!(r.record, None);
    assert_eq!(r.removed, [at.clone(), format!("{at}.source.jsonl")]);
    assert!(l.dir.join(format!("{at}.notes/20261123-a.md")).is_file());
    assert!(l.dir.join(format!("{at}.stamps/20261124-btc.ots")).is_file());
    assert!(r.notices.iter().any(|n| n.contains("notes stay")), "{:?}", r.notices);
    assert!(r.notices.iter().any(|n| n.contains("timestamp proofs stay")), "{:?}", r.notices);
    assert_eq!(l.status(), [Item::Gone { name: l.v1.clone(), path: at.clone() }]);
    l.publish("remove v1");
    assert!(l.library_line(&l.v1).is_none());
    assert!(l.status().is_empty(), "{:?}", l.status());

    // a draft, a changed object, an object sealed but in no commit
    let g = support::system_git(&l.home);
    write(&l.dir.join("doc/d/doc_draft-d/toa.yaml"), b"type: doc\nlicense: MIT\n");
    write(&l.dir.join("doc/d/doc_draft-d/README.md"), b"hello\n");

    seal::seal(&g, &l.dir, &SealOptions { dry_run: false, lookup: Vec::new(), workspace: None }).unwrap();
    let doc = dir_named(&l.dir.join("doc/d"), "doc_");
    write(&l.dir.join("doc/e/doc_draft-e/toa.yaml"), b"type: doc\nlicense: MIT\n");
    write(&l.dir.join("doc/e/doc_draft-e/README.md"), b"hello\n");

    let ip_at = format!("ifc/comp/{}", l.ip);
    write(&l.dir.join(format!("{ip_at}/extra.txt")), b"changed\n");
    let items = l.status();
    assert!(items.contains(&Item::Draft { path: "doc/e/doc_draft-e".into() }), "{items:?}");
    assert!(items.contains(&Item::Unpublished { name: doc.clone(), path: format!("doc/d/{doc}") }), "{items:?}");
    assert!(items.iter().any(|i| matches!(i, Item::Changed { path, .. } if *path == ip_at)), "{items:?}");

    // in no commit: its files would be lost
    assert_eq!(problems(remove::remove(l.reader().as_ref(), &l.dir, &doc, &l.opts())), ["E_REMOVE_UNCOMMITTED"]);
    // committed on another branch only: no record, as no commit of the publication branch holds it
    git(&l.home, &l.dir, &["checkout", "--quiet", "-b", "work"], None);
    std::fs::remove_dir_all(l.dir.join("doc/e")).unwrap();
    std::fs::remove_file(l.dir.join(format!("{ip_at}/extra.txt"))).unwrap();
    commit_all(&l.home, &l.dir, "a document, not published");
    let rec = Options { record: true, ..l.opts() };
    assert_eq!(problems(remove::remove(l.reader().as_ref(), &l.dir, &doc, &rec)), ["E_REMOVE_UNPUBLISHED"]);
    assert!(l.dir.join(format!("doc/d/{doc}")).is_dir());
    remove::remove(l.reader().as_ref(), &l.dir, &doc, &l.opts()).unwrap();
    assert!(!l.dir.join(format!("doc/d/{doc}")).exists());
}

#[test]
fn a_copy_is_named_by_its_path() {
    let l = Lib::new("rm-copies");
    let from = l.dir.join(format!("ifc/comp/{}", l.ip));
    let to = l.dir.join(format!("ifc/copy/{}", l.ip));
    write(&to.join("toa.yaml"), &std::fs::read(from.join("toa.yaml")).unwrap());
    commit_all(&l.home, &l.dir, "a second copy");
    assert_eq!(problems(remove::remove(l.reader().as_ref(), &l.dir, &l.ip, &l.opts())), ["E_REMOVE_COPIES"]);
    let rel = format!("ifc/copy/{}", l.ip);
    let r = remove::remove(l.reader().as_ref(), &l.dir, &rel, &Options { record: true, ..l.opts() }).unwrap();
    assert_eq!(r.path, rel);
    assert!(from.is_dir() && !to.exists());
    // the record names this copy where the publication branch holds it
    let rec: Value =
        serde_json::from_slice(&std::fs::read(l.dir.join(format!("{rel}.removed.json"))).unwrap()).unwrap();
    assert_eq!(rec["path"], rel.as_str());
    assert_eq!(rec["label"], "comp");
    // a name that is no object here
    let none = "doc_0123456789abcdef0123456789abcdef01234567";
    assert_eq!(problems(remove::remove(l.reader().as_ref(), &l.dir, none, &l.opts())), ["E_REMOVE_MISSING"]);
}

#[test]
fn the_library_check_looks_at_stamps_directories() {
    // 4.3 and 6.4 item 5: what a stamps directory holds keeps only the rules of every path of a
    // library (decisions 11-25); what a proof says is not read
    let l = Lib::new("rm-stamps");
    let at = format!("ckt/comp/s.v2/{}", l.v2);
    write(&l.dir.join(format!("{at}.stamps/20261124-btc.ots")), b"a proof");
    write(&l.dir.join(format!("{at}.stamps/20261124-rfc.tsr")), &vec![0u8; 64 * 1024]);
    write(&l.dir.join(format!("{at}.stamps/20261124-any.unknown")), b"left as it is");
    // nothing in a sidecar directory is a sidecar (6.4 item 5): names that start with an object
    // name and a dot are free there, in a notes directory too
    write(&l.dir.join(format!("{at}.stamps/{}.ots", l.v2)), b"a proof of the object name");
    write(&l.dir.join(format!("{at}.stamps/{}.source.jsonl", l.v2)), b"not a source list");
    write(&l.dir.join(format!("{at}.notes/{}.md", l.v2)), b"a note named by the object\n");
    let good = commit_all(&l.home, &l.dir, "stamps");
    let r = lib::check(l.reader().as_ref(), &l.dir, &good, None, &Default::default()).unwrap();
    assert!(r.problems.iter().all(|p| !p.is_error()), "{:#?}", r.problems);
    let objects = r.objects;

    // what a stamps directory holds is not restricted (decisions 11-25): large, in directories,
    // any name the rules of every path allow
    write(&l.dir.join(format!("{at}.stamps/20261124-big.ots")), &vec![0u8; 64 * 1024 + 1]);
    write(&l.dir.join(format!("{at}.stamps/sub/x.ots")), b"in a directory");
    write(&l.dir.join(format!("{at}.stamps/Proof_TSA.DER")), b"a proof of another kind");
    // the rules of every path still hold in it
    write(&l.dir.join(format!("{at}.stamps/sub/x y.ots")), b"a name no path may have");
    // nor may anything there be taken for an object: a directory named as one is an error, and
    // is not checked as an object
    write(&l.dir.join(format!("{at}.stamps/copy/{}/toa.yaml", l.v1)), b"not an object\n");
    write(&l.dir.join(format!("ckt/comp/s.v1/{}.stamps", l.v1)), b"a file, not a directory");
    let bad = commit_all(&l.home, &l.dir, "bad stamps");
    let r = lib::check(l.reader().as_ref(), &l.dir, &bad, Some(&good), &Default::default()).unwrap();
    assert_eq!(r.objects, objects, "nothing in a stamps directory is an object");
    // nor for the tools that look for objects and drafts in a work tree
    let scanned = seal::scan(&l.dir).unwrap();
    assert!(
        scanned.objects.iter().chain(&scanned.drafts).all(|p| !p.to_string_lossy().contains(".stamps")),
        "{scanned:?}"
    );
    let mut found: Vec<(String, String)> = r
        .problems
        .iter()
        .filter(|p| p.is_error())
        .map(|p| (p.code.to_string(), p.path.rsplit('/').next().unwrap().to_string()))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            ("E_LIB_NAME".to_string(), "x y.ots".to_string()),
            ("E_LIB_SIDECAR".to_string(), l.v1.clone()),
            ("E_LIB_SIDECAR".to_string(), format!("{}.stamps", l.v1)),
        ],
        "{:#?}",
        r.problems
    );
}

#[test]
fn publishing_commits_what_was_removed() {
    let l = Lib::new("rm-publish");
    let at1 = format!("ckt/comp/s.v1/{}", l.v1);
    let at2 = format!("ckt/comp/s.v2/{}", l.v2);
    // one removed with a record, one without; something else of the work tree, not to be committed
    remove::remove(l.reader().as_ref(), &l.dir, &l.v2, &Options { record: true, ..l.opts() }).unwrap();
    remove::remove(l.reader().as_ref(), &l.dir, &l.v1, &l.opts()).unwrap();
    write(&l.dir.join("scratch.txt"), b"not part of the library\n");
    write(&l.dir.join(format!("ifc/comp/{}.notes/20261124-later.md", l.ip)), b"a note not yet committed\n");

    let g = support::system_git(&l.home);
    let o = publish::Options { to: Some(ADDRESS.into()), branch: "main".into(), commit: true, ..Default::default() };
    let r = publish::publish(&g, &l.dir, &o).unwrap_or_else(|e| panic!("publishing: {e:?}"));
    assert!(r.commit.is_some(), "{r:?}");
    let errors: Vec<_> = r.check.iter().filter(|p| p.is_error()).collect();
    assert!(errors.is_empty(), "{errors:#?}");

    // the commit no longer holds the objects removed, nor their notes and source lists
    let tree = git(&l.home, &l.dir, &["ls-tree", "-r", "--name-only", "HEAD"], None);
    for gone in [&at1, &at2] {
        for prefix in [format!("{gone}/"), format!("{gone}.notes/"), format!("{gone}.source.jsonl")] {
            assert!(!tree.lines().any(|f| f.starts_with(&prefix)), "{prefix} is still in\n{tree}");
        }
    }
    assert!(tree.lines().any(|f| f.starts_with(&format!("{at2}.removed"))), "the record is committed:\n{tree}");
    // and nothing else of the work tree went with it
    let status = git(&l.home, &l.dir, &["status", "--porcelain", "--untracked-files=all"], None);
    assert!(status.contains("?? scratch.txt"), "{status}");
    assert!(status.contains(".notes/20261124-later.md"), "{status}");
    assert_eq!(status.lines().count(), 2, "{status}");
}
