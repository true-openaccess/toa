//! Type `lib`, the frozen library (`spec/types/en/lib.md`): a set of objects frozen as an object,
//! listed in its `members.source.jsonl`. The rules of that file in any object (3.3) are checked
//! with the common rules (`toa_core::members`); here what the type adds.

use std::collections::BTreeSet;

use crate::v::V;
use crate::{Lookup, Object};
use toa_core::members;
use toa_core::name;
use toa_core::object;
use toa_core::Problem;

pub(crate) fn check(o: &Object, lookup: &dyn Lookup, out: &mut Vec<Problem>) {
    let mut v = V { out, section: "types/lib", file: "toa.yaml" };
    let m = o.manifest;
    if v.fields(m, "", &crate::allowed(&["name"]), &["name"]) {
        if let Some(n) = m.get("name") {
            v.shaped(
                n,
                "name",
                crate::ifc::short_name,
                "a short name such as toa-official: [a-z][a-z0-9]{0,39}(-[a-z0-9]+)*",
            );
        }
    }
    // 1. the list of members exists
    let Some(bytes) = o.bytes(members::FILE) else {
        v.out.push(Problem::error(
            "E_LIB_MEMBERS",
            "types/lib",
            members::FILE,
            "a frozen library has its list of members, members.source.jsonl",
        ));
        return;
    };
    let (lines, _) = members::read(bytes);
    // 2. no object name on two lines
    let mut listed = BTreeSet::new();
    for l in &lines {
        if !listed.insert(l.object.as_str()) {
            v.out.push(Problem::error(
                "E_LIB_TWICE",
                "types/lib",
                members::FILE,
                format!("{} is on two lines", l.object),
            ));
        }
    }
    // 3. when found: the objects the members need (5.2) should be listed too
    for need in needed(&lines.iter().map(|l| l.object.clone()).collect::<Vec<_>>(), lookup) {
        if !listed.contains(need.as_str()) {
            v.out.push(Problem::warning(
                "W_LIB_UNLISTED",
                "types/lib",
                members::FILE,
                format!("{need} is needed by a member but not listed, so whoever fetches this frozen library looks elsewhere for it"),
            ));
        }
    }
}

/// The objects the members need, as far as their manifests can be found (5.2, step 2): the
/// references of every object of a type that is followed, except its `base`, again and again.
fn needed(members: &[String], lookup: &dyn Lookup) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = members.iter().cloned().collect();
    let mut out = BTreeSet::new();
    let mut todo: Vec<String> = members.to_vec();
    while let Some(n) = todo.pop() {
        if !name::prefix_of(&n).is_some_and(crate::followed) {
            continue;
        }
        let Some(m) = lookup.manifest(&n) else { continue };
        for r in object::references(&m) {
            if r.at.starts_with("base/") || r.draft {
                continue;
            }
            out.insert(r.name.clone());
            if seen.insert(r.name.clone()) {
                todo.push(r.name);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use toa_core::manifest::{self, Node};

    const CKT: &str = "ckt_1ec6249cf01c000f19632bde9801c5af4cca1698";
    const IFC: &str = "ifc_945cdda745e8f44886e3aa17d7752db313d91136";

    struct One;
    impl Lookup for One {
        fn manifest(&self, name: &str) -> Option<Node> {
            (name == CKT).then(|| manifest::parse(format!("type: ckt\nifc: {IFC}\n").as_bytes()).unwrap())
        }
        fn file_sha256(&self, _: &str, _: &str) -> Option<String> {
            None
        }
    }

    fn run(members: Option<&str>, lookup: &dyn Lookup) -> Vec<&'static str> {
        let text = "type: lib\nlicense: CC0-1.0\nname: toa-official\ntitle: TOA official library, 2026-12\n";
        let m = manifest::parse(text.as_bytes()).unwrap();
        let mut files = vec![("toa.yaml".to_string(), text.as_bytes().to_vec())];
        if let Some(b) = members {
            files.push((members::FILE.to_string(), b.as_bytes().to_vec()));
        }
        crate::check("lib", &Object { manifest: &m, files: &files, lock: None }, lookup)
            .iter()
            .map(|p| p.code)
            .collect()
    }

    #[test]
    fn members_listed_once_and_in_full() {
        assert_eq!(run(None, &crate::Nothing), ["E_LIB_MEMBERS"]);
        let one = format!("{{\"toa\": \"source/1\"}}\n{{\"object\": \"{CKT}\"}}\n");
        assert!(run(Some(&one), &crate::Nothing).is_empty());
        // the circuit needs its interface
        assert_eq!(run(Some(&one), &One), ["W_LIB_UNLISTED"]);
        let both = format!("{one}{{\"object\": \"{IFC}\"}}\n");
        assert!(run(Some(&both), &One).is_empty());
        let twice = format!("{both}{{\"object\": \"{IFC}\"}}\n");
        assert_eq!(run(Some(&twice), &One), ["E_LIB_TWICE"]);
    }
}
