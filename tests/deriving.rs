//! `toa derive`: a draft copied from an object, its `base` pointing back; then sealed.

#[path = "support.rs"]
mod support;

use std::path::Path;

use support::{commit_all, library, temp, write};
use toa_repo::derive::{self, DeriveError, Options};
use toa_repo::seal::{self, SealOptions};

fn opts() -> SealOptions {
    SealOptions { dry_run: false, lookup: Vec::new(), workspace: None }
}

fn only_dir(parent: &Path, prefix: &str) -> String {
    let mut names: Vec<String> = std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(prefix) && !n.contains('.') && !n.contains("_draft-"))
        .collect();
    assert_eq!(names.len(), 1, "{names:?} in {}", parent.display());
    names.remove(0)
}

fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|r| r.map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn history(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("history.json")).unwrap()).unwrap()
}

fn into(dir: &Path) -> Options {
    Options { tag: None, into: dir.to_path_buf(), dry_run: false }
}

const IP: &str = "type: ifc\nlicense: MIT\nname: x\ntitle: X\npins:\n- {name: a}\n";

#[test]
fn derive_change_and_seal() {
    let home = temp("derive-home");
    let lib = temp("derive-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    let net: String = (0..20).map(|i| format!("R{i} a b 1k\n")).collect::<String>() + "subckt x a\nends x\n";
    write(&lib.join("ifc/x/ifc_draft-a/toa.yaml"), IP.as_bytes());
    let imp = "type: ckt\nlicense: MIT\nifc: ifc_draft-a   # the interface\nname: s_x\nform: ideal\nfiles:\n  netlist: [n.scs]\n";
    write(&lib.join("ckt/x/s.g1/ckt_draft-b/toa.yaml"), imp.as_bytes());
    write(&lib.join("ckt/x/s.g1/ckt_draft-b/n.scs"), net.as_bytes());
    write(&lib.join("ckt/x/s.g1/ckt_draft-b.notes/20261124-hello.md"), b"a note beside it\n");
    seal::seal(&g, &lib, &opts()).unwrap();
    commit_all(&home, &lib, "first");
    let ip = only_dir(&lib.join("ifc/x"), "ifc_");
    let g1 = only_dir(&lib.join("ckt/x/s.g1"), "ckt_");
    let g1_dir = lib.join("ckt/x/s.g1").join(&g1);
    let before = toa_repo::fs::read_dir(&g1_dir).unwrap().files;

    // a dry run tells what it would do and writes nothing
    let dry = derive::derive(&g1_dir, &Options { dry_run: true, ..into(&lib.join("ckt/x/s.g2")) }).unwrap();
    assert_eq!((dry.draft.as_str(), dry.base_replaced), ("ckt_draft-s-x", false), "the tag from name, _ as -");
    assert!(!lib.join("ckt/x/s.g2").exists());

    // derived: the files but history.json, nothing beside it, base added and no other byte changed
    let d = derive::derive(&g1_dir, &into(&lib.join("ckt/x/s.g2"))).unwrap();
    let draft = lib.join("ckt/x/s.g2/ckt_draft-s-x");
    assert_eq!(d.path, draft);
    assert_eq!(entries(&draft), ["n.scs", "toa.yaml"]);
    assert_eq!(entries(&lib.join("ckt/x/s.g2")), ["ckt_draft-s-x"], "no notes, no temporary directory");
    let sealed_text = std::fs::read_to_string(g1_dir.join("toa.yaml")).unwrap();
    assert_eq!(std::fs::read_to_string(draft.join("toa.yaml")).unwrap(), format!("{sealed_text}base: [{g1}]\n"));

    // changed and sealed: base points back, one generation more (one series)
    write(&draft.join("n.scs"), format!("{net}* changed\n").as_bytes());
    seal::seal(&g, &lib, &opts()).unwrap();
    commit_all(&home, &lib, "second");
    let g2 = only_dir(&lib.join("ckt/x/s.g2"), "ckt_");
    let g2_dir = lib.join("ckt/x/s.g2").join(&g2);
    let text = std::fs::read_to_string(g2_dir.join("toa.yaml")).unwrap();
    assert!(
        text.ends_with(&format!("base: [{g1}]\n")) && text.contains(&format!("ifc: {ip}   # the interface")),
        "{text}"
    );
    assert_eq!(history(&g2_dir), serde_json::json!({"history": [g1], "generation": 2}));

    // derived again, base replaced; with another name it is generation 1 of a new series
    let d = derive::derive(&g2_dir, &Options { tag: Some("other".into()), ..into(&lib.join("ckt/x/o.g1")) }).unwrap();
    assert!(d.base_replaced);
    let draft = lib.join("ckt/x/o.g1/ckt_draft-other");
    assert!(g2_dir.join("history.json").is_file());
    assert_eq!(entries(&draft), ["n.scs", "toa.yaml"], "history.json is written again by sealing");
    let t = std::fs::read_to_string(draft.join("toa.yaml")).unwrap();
    assert_eq!(t, text.replace(&format!("base: [{g1}]\n"), &format!("base: [{g2}]\n")));
    write(&draft.join("toa.yaml"), t.replace("name: s_x", "name: other").as_bytes());
    seal::seal(&g, &lib, &opts()).unwrap();
    let o1 = only_dir(&lib.join("ckt/x/o.g1"), "ckt_");
    assert_eq!(history(&lib.join("ckt/x/o.g1").join(&o1)), serde_json::json!({"history": [g2, g1], "generation": 1}));

    // the object derived from is not changed by a byte
    assert_eq!(toa_repo::fs::read_dir(&g1_dir).unwrap().files, before);
}

#[test]
fn what_stops_it_leaves_nothing() {
    let home = temp("derive-stop-home");
    let lib = temp("derive-stop-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    // a type this tool does not know can be derived too
    write(&lib.join("widget/w/widget_draft-a/toa.yaml"), b"type: widget\nlicense: MIT\n");
    write(&lib.join("widget/w/widget_draft-a/data.bin"), b"bytes\n");
    seal::seal(&g, &lib, &opts()).unwrap();
    let w = only_dir(&lib.join("widget/w"), "widget_");
    let w_dir = lib.join("widget/w").join(&w);
    let out = lib.join("widget/w2");
    let d = derive::derive(&w_dir, &into(&out)).unwrap();
    assert_eq!(d.draft, format!("widget_draft-from-{}", &w["widget_".len().."widget_".len() + 8]));
    assert_eq!(
        std::fs::read_to_string(d.path.join("toa.yaml")).unwrap(),
        format!("type: widget\nlicense: MIT\nbase: [{w}]\n")
    );

    // the draft name taken: here, or by a draft anywhere in the repository
    let again =
        derive::derive(&w_dir, &Options { tag: Some(d.draft["widget_draft-".len()..].to_string()), ..into(&out) });
    assert!(matches!(&again, Err(DeriveError::Problems(p)) if p[0].code == "E_DERIVE_EXISTS"), "{again:?}");
    write(&lib.join("elsewhere/widget_draft-dup/toa.yaml"), b"type: widget\nlicense: MIT\n");
    let fresh = lib.join("widget/w3");
    let r = derive::derive(&w_dir, &Options { tag: Some("dup".into()), ..into(&fresh) });
    assert!(matches!(&r, Err(DeriveError::Problems(p)) if p[0].code == "E_DERIVE_EXISTS"), "{r:?}");
    assert!(!fresh.exists(), "nothing written, not even the directory");

    // a tag that is not one
    assert!(matches!(
        derive::derive(&w_dir, &Options { tag: Some("Bad_Tag".into()), ..into(&fresh) }),
        Err(DeriveError::BadTag(_))
    ));

    // a copy that does not verify from its files (7.4), and a draft: not derived
    let bad = temp("derive-bad").join(&w);
    write(&bad.join("toa.yaml"), b"type: widget\nlicense: MIT\n");
    write(&bad.join("data.bin"), b"other bytes\n");
    let r = derive::derive(&bad, &into(&fresh));
    assert!(matches!(&r, Err(DeriveError::Problems(p)) if toa_core::problem::has_errors(p)), "{r:?}");
    let r = derive::derive(&lib.join("elsewhere/widget_draft-dup"), &into(&fresh));
    assert!(matches!(&r, Err(DeriveError::Problems(p)) if p[0].code == "E_DERIVE_NAME"), "{r:?}");
    assert!(!fresh.exists());
    assert_eq!(entries(&out), std::slice::from_ref(&d.draft), "no temporary directory left");
}
