//! `toa index` (doc/index-format.md) on a library made for the test: objects, lineage, reverse
//! lookups, newer versions, notes, removal records, dates, and an incremental run.

#[path = "support.rs"]
mod support;

use std::path::Path;

use serde_json::Value;
use support::{commit_all, git, library, temp, write};
use toa_repo::index::{self, Options};
use toa_repo::seal::{self, SealOptions};

fn dir_named(parent: &Path, prefix: &str) -> String {
    std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .find(|n| n.starts_with(prefix) && !n.contains('.'))
        .unwrap()
}

fn object(out: &Path, name: &str) -> Value {
    let (_, h) = name.split_once('_').unwrap();
    let p = out.join("objects").join(&h[..2]).join(format!("{name}.json"));
    serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

#[test]
fn an_index_of_one_library() {
    let home = temp("index-home");
    let lib = temp("index-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    let reader = support::reader(&home);
    let opts = SealOptions { dry_run: false, lookup: Vec::new(), workspace: None };

    write(&lib.join("ifc/x/ifc_draft-a/toa.yaml"), b"type: ifc\nlicense: MIT\nname: x\ntitle: X\npins:\n- {name: a}\n");
    write(
        &lib.join("ckt/x/s.v1/ckt_draft-b/toa.yaml"),
        b"type: ckt\nlicense: MIT\nifc: ifc_draft-a\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\n",
    );
    write(&lib.join("ckt/x/s.v1/ckt_draft-b/n.scs"), b"subckt x a\nends x\n");
    // two documents: one titled in its manifest, one by the first heading of its README.md
    write(&lib.join("doc/x/titled/doc_draft-d/toa.yaml"), b"type: doc\nlicense: MIT\ntitle: Written title\n");
    write(&lib.join("doc/x/titled/doc_draft-d/README.md"), b"# Not this\n");
    write(&lib.join("doc/x/untitled/doc_draft-e/toa.yaml"), b"type: doc\nlicense: MIT\nrefs: [ifc_draft-a]\n");
    write(&lib.join("doc/x/untitled/doc_draft-e/README.md"), b"```\n# in code\n```\n\n## Kickback notes ##\n\nText.\n");

    seal::seal(&g, &lib, &opts).unwrap();
    let ip = dir_named(&lib.join("ifc/x"), "ifc_");
    let v1 = dir_named(&lib.join("ckt/x/s.v1"), "ckt_");
    let titled = dir_named(&lib.join("doc/x/titled"), "doc_");
    let untitled = dir_named(&lib.join("doc/x/untitled"), "doc_");
    // source lists with local paths would fail the library check; the index only records them
    commit_all(&home, &lib, "first");

    let v2_text = format!("type: ckt\nlicense: MIT\nifc: {ip}\nbase: [{v1}]\nname: sx\nform: ideal\nfiles:\n  netlist: [n.scs]\nparameters: {{rate: .inf}}\n");
    write(&lib.join("ckt/x/s.v2/ckt_draft-c/toa.yaml"), v2_text.as_bytes());
    write(&lib.join("ckt/x/s.v2/ckt_draft-c/n.scs"), b"subckt x a\nends x\n");

    seal::seal(&g, &lib, &opts).unwrap();
    let v2 = dir_named(&lib.join("ckt/x/s.v2"), "ckt_");
    write(&lib.join(format!("ckt/x/s.v1/{v1}.notes/20261123-hello.md")), b"hello\n");
    // timestamp proofs beside it (4.3): their names, kinds and sizes are recorded, not checked
    write(&lib.join(format!("ckt/x/s.v1/{v1}.stamps/20261124-btc.ots")), b"not checked");
    write(&lib.join(format!("ckt/x/s.v1/{v1}.stamps/20261124-tsa.tsr")), b"der");
    // a label in the library list wins over the one beside the object; the list marks v2 latest
    let listed = format!(
        "{{\"toa\": \"source/1\"}}\n{{\"object\": \"{v2}\", \"label\": \"x/s.v2 from the list\", \"latest\": true, \"sources\": [{{\"local\": \"ckt/x/s.v2/{v2}\"}}]}}\n"
    );
    write(&lib.join("library.source.jsonl"), listed.as_bytes());
    let beside = format!(
        "{{\"toa\": \"source/1\"}}
{{\"object\": \"{v1}\", \"label\": \"x/s.v1 beside\"}}
"
    );
    write(&lib.join(format!("ckt/x/s.v1/{v1}.source.jsonl")), beside.as_bytes());
    let second = commit_all(&home, &lib, "second");

    let ws = temp("index-ws");
    let list = format!(
        "{{\"toa\": \"source/1\"}}\n{{\"include\": \"library.source.jsonl\", \"group\": \"mine\", \"sources\": [{{\"local\": \"{}\"}}]}}\n",
        lib.to_string_lossy().replace('\\', "/")
    );
    write(&ws.join("workspace.source.jsonl"), list.as_bytes());
    let out = ws.join("index");
    let o = Options {
        workspace: ws.join("workspace.source.jsonl"),
        out: out.clone(),
        cache: ws.join("cache"),
        offline: true,
        tool: "toa test".into(),
        ..Default::default()
    };
    let r = index::build(reader.as_ref(), &o).unwrap();
    assert_eq!((r.libraries, r.objects, r.valid, r.not_fetched), (1, 5, 5, false));

    let root: Value = serde_json::from_slice(&std::fs::read(out.join("index.json")).unwrap()).unwrap();
    assert_eq!(root["toa-index"], 2);
    assert_eq!(root["libraries"][0]["commit"], second.as_str());
    assert_eq!(root["libraries"][0]["address"], Value::Null, "no local paths in an index");

    let o1 = object(&out, &v1);
    let o2 = object(&out, &v2);
    let oip = object(&out, &ip);
    // the manifest as JSON, with the non-finite float as an object
    assert_eq!(o2["manifest"]["parameters"]["rate"], serde_json::json!({"float": ".inf"}));
    assert_eq!(o2["manifest"]["name"], "sx");
    // the title: written, or for a document the first heading of README.md; else absent
    assert_eq!(oip["title"], "X");
    assert_eq!(object(&out, &titled)["title"], "Written title");
    assert_eq!(object(&out, &untitled)["title"], "Kickback notes");
    assert!(o2.get("title").is_none(), "an implementation without title has none: {o2}");
    // lineage: verified, and the reverse lookups use it
    assert_eq!(o2["history"]["state"], "verified");
    assert_eq!(o2["history"]["verified"], serde_json::json!([v1]));
    assert_eq!(
        (o2["history"]["generation"].as_u64(), o2["history"]["generation_state"].as_str()),
        (Some(2), Some("verified"))
    );
    assert_eq!(o1["history"]["generation"], 1);
    assert_eq!(o1["base_by"], serde_json::json!([v2]));
    // a newer version: v2 is marked latest, holds v1 in its history, and is of one series (7.5)
    assert_eq!(o2["locations"][0]["latest"], true);
    assert_eq!(o1["locations"][0]["latest"], false);
    assert_eq!(o1["newer"][0]["name"], v2.as_str());
    assert_eq!(o1["newer"][0]["marked_by"], serde_json::json!(["mine"]));
    assert_eq!(o1["newer"][0]["together"], serde_json::json!(["mine"]));
    assert_eq!(o1["newer"][0]["ifc_changed"], false);
    let by: Vec<&str> = oip["referenced_by"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(by.len(), 3, "two implementations and the document");
    // where and when: v1 first came with the first commit, v2 with the second
    let first = git(&home, &lib, &["rev-parse", "HEAD~1"], None);
    assert_eq!(o1["locations"][0]["first_commit"], first.as_str());
    assert_eq!(o2["locations"][0]["first_commit"], second.as_str());
    assert_eq!(o1["locations"][0]["first_date"], "2026-11-23T12:00:00Z");
    assert_eq!(o1["locations"][0]["first_signature"], Value::Null, "not signed");
    assert_eq!(o1["first"]["signature"], Value::Null);
    assert_eq!(o1["locations"][0]["notes"][0]["text"], "hello\n");
    assert_eq!(o1["locations"][0]["notes"][0]["commit"], second.as_str());
    assert_eq!(
        o1["locations"][0]["stamps"],
        serde_json::json!([
            {"file": "20261124-btc.ots", "kind": "opentimestamps", "size": 11, "commit": second, "date": "2026-11-23T12:00:00Z"},
            {"file": "20261124-tsa.tsr", "kind": "rfc3161", "size": 3, "commit": second, "date": "2026-11-23T12:00:00Z"}
        ])
    );
    assert_eq!(object(&out, &v2)["locations"][0]["stamps"], serde_json::json!([]));
    assert!(o1["locations"][0]["source_list"].as_str().unwrap().ends_with(".source.jsonl"));
    assert_eq!(o2["locations"][0]["label"], "x/s.v2 from the list");
    assert_eq!(o1["locations"][0]["label"], "x/s.v1 beside", "else the line beside the object");
    assert_eq!(oip["locations"][0]["label"], serde_json::Value::Null);
    // every file with its blob id and SHA-256
    assert_eq!(o1["files"][1]["path"], "toa.yaml", "sorted: n.scs first");
    assert_eq!(o1["files"][1]["sha256"].as_str().unwrap().len(), 64);

    // remove v1 and leave a removal record; the lineage of v2 still checks through it (4.2, 8.3)
    let rec =
        serde_json::json!({"toa": "removed/1", "object": v1, "commit": second, "path": format!("ckt/x/s.v1/{v1}")});
    std::fs::remove_dir_all(lib.join("ckt/x/s.v1").join(&v1)).unwrap();
    write(&lib.join(format!("ckt/x/s.v1/{v1}.removed.json")), format!("{rec}\n").as_bytes());
    commit_all(&home, &lib, "remove v1");
    let r = index::build(reader.as_ref(), &o).unwrap();
    assert_eq!(r.objects, 4);
    let o1 = object(&out, &v1);
    assert_eq!(o1["present"], false);
    assert_eq!(o1["removals"][0]["valid"], true, "{o1}");
    let o2 = object(&out, &v2);
    assert_eq!(o2["history"]["state"], "verified", "{o2}");
    let libjson: Value = serde_json::from_slice(&std::fs::read(out.join("libraries/mine.json")).unwrap()).unwrap();
    assert_eq!(libjson["removed"], serde_json::json!([v1]));
    assert_eq!(libjson["previous"]["ancestor"], true);
}

/// One repository, two publication branches: two libraries (5.1). The same address and branch
/// written twice is one library with both groups.
#[test]
fn two_branches_of_one_repository() {
    let home = temp("index2-home");
    let lib = temp("index2-lib");
    library(&home, &lib);
    let g = support::system_git(&home);
    let reader = support::reader(&home);
    let opts = SealOptions { dry_run: false, lookup: Vec::new(), workspace: None };
    write(&lib.join("README.md"), b"two libraries, one per branch\n");
    commit_all(&home, &lib, "main");

    git(&home, &lib, &["checkout", "--quiet", "-b", "pll"], None);
    write(&lib.join("ifc/p/ifc_draft-a/toa.yaml"), b"type: ifc\nlicense: MIT\nname: p\ntitle: P\npins:\n- {name: a}\n");
    write(&lib.join("doc/p/doc_draft-b/toa.yaml"), b"type: doc\nlicense: MIT\ntitle: About P\n");
    write(&lib.join("doc/p/doc_draft-b/README.md"), b"# P\n");
    seal::seal(&g, &lib, &opts).unwrap();
    commit_all(&home, &lib, "pll");

    git(&home, &lib, &["checkout", "--quiet", "main"], None);
    git(&home, &lib, &["checkout", "--quiet", "-b", "risca"], None);
    write(&lib.join("doc/r/doc_draft-c/toa.yaml"), b"type: doc\nlicense: MIT\ntitle: About R\n");
    write(&lib.join("doc/r/doc_draft-c/README.md"), b"# R\n");
    seal::seal(&g, &lib, &opts).unwrap();
    commit_all(&home, &lib, "risca");
    let only_r = dir_named(&lib.join("doc/r"), "doc_");
    git(&home, &lib, &["checkout", "--quiet", "main"], None);

    let ws = temp("index2-ws");
    let here = lib.to_string_lossy().replace('\\', "/");
    let line = |group: Option<&str>, branch: &str, address: &str| {
        let g = group.map(|g| format!("\"group\": \"{g}\", ")).unwrap_or_default();
        format!("{{\"include\": \"library.source.jsonl\", {g}\"branch\": \"{branch}\", \"sources\": [{{\"local\": \"{here}\"}}, {{\"git\": \"{address}\"}}]}}\n")
    };
    let run = |list: String, out: &str| {
        write(&ws.join("workspace.source.jsonl"), format!("{{\"toa\": \"source/1\"}}\n{list}").as_bytes());
        let o = Options {
            workspace: ws.join("workspace.source.jsonl"),
            out: ws.join(out),
            cache: ws.join("cache"),
            offline: true,
            tool: "toa test".into(),
            ..Default::default()
        };
        let r = index::build(reader.as_ref(), &o).unwrap();
        let root: Value = serde_json::from_slice(&std::fs::read(ws.join(out).join("index.json")).unwrap()).unwrap();
        (r, root)
    };
    let a = "https://example.org/two.git";

    // two branches: two libraries, each with its own objects; the same address and branch
    // written again (another spelling of the address) adds its group to the first
    let list = line(Some("pll"), "pll", a)
        + &line(Some("risca"), "risca", a)
        + &line(Some("again"), "pll", "https://EXAMPLE.org/two.git/");
    for _ in 0..2 {
        // the second run reuses what the first read, library by library
        let (r, root) = run(list.clone(), "index");
        assert_eq!((r.libraries, r.objects, r.valid), (2, 3, 3), "{root}");
        let libs: Vec<(&str, &str)> = root["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| (l["key"].as_str().unwrap(), l["branch"].as_str().unwrap()))
            .collect();
        assert_eq!(libs, [("pll", "pll"), ("risca", "risca")]);
        let pll: Value = serde_json::from_slice(&std::fs::read(ws.join("index/libraries/pll.json")).unwrap()).unwrap();
        assert_eq!(pll["groups"], serde_json::json!(["pll", "again"]));
    }
    let o = object(&ws.join("index"), &only_r);
    assert_eq!(o["locations"].as_array().unwrap().len(), 1);
    assert_eq!(o["locations"][0]["library"], "risca", "{o}");

    // without groups: the second branch's key has the branch added
    let (r, root) = run(line(None, "pll", a) + &line(None, "risca", a), "index-keys");
    assert_eq!((r.libraries, r.objects), (2, 3));
    let keys: Vec<&str> = root["libraries"].as_array().unwrap().iter().map(|l| l["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["example-org-two-git", "example-org-two-git-risca"]);

    // the libraries on this machine for the same-files hints of toa check: a checkout and a
    // branch; written twice, one
    let w = ws.join("workspace.source.jsonl");
    write(
        &w,
        format!(
            "{{\"toa\": \"source/1\"}}\n{}",
            line(None, "pll", a) + &line(None, "risca", a) + &line(None, "pll", a)
        )
        .as_bytes(),
    );
    // toa fetch (and toa derive by name): an object only on the branch risca, with main checked
    // out, is found in the checkout's git store, offline
    let mut f = toa_repo::fetch::Fetcher::new(reader.as_ref(), ws.join("fetch-cache"));
    f.offline = true;
    f.use_workspace(toa_repo::fetch::read_workspace(&w).unwrap());
    let got = f.fetch(&only_r, None).unwrap();
    assert!(got.from.ends_with("(git)"), "{}", got.from);
    let locals = toa_repo::same::local_libraries(&w);
    let got: Vec<(String, String)> = locals.into_iter().map(|(_, l, b)| (l, b)).collect();
    assert_eq!(got, [(here.clone(), "pll".to_string()), (format!("{here} (risca)"), "risca".to_string())]);
}
