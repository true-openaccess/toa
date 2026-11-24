//! The test vectors of the standard, `spec/vectors/`, run against the reference tool's library.
//!
//! The vectors give only whether an input is valid and which section it tests, never a problem
//! code (see `spec/vectors/README.md`), so these tests check results, not messages.

use std::path::{Path, PathBuf};

use serde_json::Value;
use toa_core::hash::{self, Algorithm};
use toa_core::name::{self, Form};
use toa_core::object;

fn vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/vectors")
}

fn load(file: &str) -> Value {
    let path = vectors_dir().join(file);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn names() {
    let doc = load("names.json");
    let vectors = doc["vectors"].as_array().unwrap();
    assert!(!vectors.is_empty());
    let mut failed = Vec::new();
    for v in vectors {
        let s = v["value"].as_str().unwrap();
        let want =
            (v["object_name"].as_bool().unwrap(), v["reserved"].as_bool().unwrap(), v["draft_name"].as_bool().unwrap());
        let got = (name::is_object_name(s), name::is_reserved(s), name::is_draft_name(s));
        let form_agrees = match name::form(s) {
            Form::Object => want.0,
            Form::Reserved => want.1,
            Form::Draft => want.2,
            Form::Other => !want.0 && !want.1 && !want.2,
        };
        if got != want || !form_agrees {
            failed.push(format!("{s:?} ({}): expected {want:?}, got {got:?}", v["section"]));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn hashes_from_json_and_from_files() {
    let doc = load("hash.json");
    for v in doc["vectors"].as_array().unwrap() {
        let id = v["id"].as_str().unwrap();
        let want = v["hash"].as_str().unwrap();
        let want256 = v["sha256"].as_str().unwrap();
        // the files as written in the JSON
        let owned: Vec<(String, Vec<u8>)> = v["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(p, c)| (p.clone(), c.as_str().unwrap().as_bytes().to_vec()))
            .collect();
        let files: Vec<(&str, &[u8])> = owned.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
        assert_eq!(hash::hash_files(Algorithm::Sha1, &files).unwrap(), want, "{id}: hash");
        assert_eq!(hash::hash_files(Algorithm::Sha256, &files).unwrap(), want256, "{id}: sha256");

        // the same files as stored in spec/vectors/hash/<id>/, read byte for byte
        let dir = vectors_dir().join(v["dir"].as_str().unwrap());
        let read = toa_repo::fs::read_dir(&dir).unwrap();
        assert!(read.problems.is_empty(), "{id}: {:?}", read.problems);
        let on_disk: Vec<(&str, &[u8])> = read.files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
        assert_eq!(
            hash::hash_files(Algorithm::Sha1, &on_disk).unwrap(),
            want,
            "{id}: the files on disk differ from the JSON; were line ends converted at checkout?"
        );

        // valid_object: only a set of files with a manifest is an object (2.3)
        let named = format!("doc_{want}");
        let verified = object::verify_files(&named, &files, &[], &[]);
        assert_eq!(
            verified.problems.is_empty(),
            v["valid_object"].as_bool().unwrap(),
            "{id}: valid_object; problems {:?}",
            verified.problems
        );
    }
}

#[test]
fn noncanonical_trees() {
    let doc = load("hash.json");
    for t in doc["noncanonical_trees"].as_array().unwrap() {
        let id = t["id"].as_str().unwrap();
        let body = hash::from_hex(t["tree_body_hex"].as_str().unwrap()).unwrap();
        let tree_id = hash::hex(&hash::tree_id_of_body(Algorithm::Sha1, &body).unwrap());
        assert_eq!(tree_id, t["git_tree_id"].as_str().unwrap(), "{id}: git_tree_id");
        let valid = t["valid"].as_bool().unwrap();
        assert_eq!(hash::is_canonical_tree_body(Algorithm::Sha1, &body), valid, "{id}: canonical");

        // rebuilding the canonical tree from the listed files gives the hash of the files (7.4),
        // whatever form the stored tree has
        let mut rebuilt = hash::Dir::new();
        for e in hash::parse_tree_body(Algorithm::Sha1, &body).unwrap() {
            rebuilt.insert_file(std::str::from_utf8(&e.name).unwrap(), e.id).unwrap();
        }
        let canonical = hash::hex(&rebuilt.id(Algorithm::Sha1).unwrap());
        assert_eq!(canonical, t["hash"].as_str().unwrap(), "{id}: rebuilt");
        assert_eq!(canonical == tree_id, valid, "{id}: a tree is valid exactly when its id is the rebuilt hash");
    }
}
