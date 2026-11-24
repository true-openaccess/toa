//! Type `ifc`, the interface (`spec/types/en/ifc.md`).

use std::collections::BTreeSet;

use crate::v::{self, V};
use crate::Object;
use toa_core::Problem;

/// The short name of an interface or a frozen library: `[a-z][a-z0-9]{0,39}(-[a-z0-9]+)*`.
pub(crate) fn short_name(s: &str) -> bool {
    let mut parts = s.split('-');
    let first = parts.next().unwrap_or("");
    v::word(first, v::lower, v::lower_digit, 0, 39) && parts.all(|p| !p.is_empty() && p.chars().all(v::lower_digit))
}

/// A pin name: `[a-zA-Z][a-zA-Z0-9_]{0,31}`.
fn pin_name(s: &str) -> bool {
    v::word(s, v::letter, v::alnum_us, 0, 31)
}

pub(crate) fn check(o: &Object, out: &mut Vec<Problem>) {
    let mut v = V { out, section: "types/ifc", file: "toa.yaml" };
    let m = o.manifest;
    if !v.fields(m, "", &crate::allowed(&["name", "pins"]), &["name", "title", "pins"]) {
        return;
    }
    if let Some(n) = m.get("name") {
        v.shaped(n, "name", short_name, "a short name such as strongarm-comp: [a-z][a-z0-9]{0,39}(-[a-z0-9]+)*");
    }
    // an interface holds no other files than images README.md shows
    let shown: Vec<String> = o
        .bytes("README.md")
        .map(|b| crate::doc::relative_links(&String::from_utf8_lossy(b)))
        .unwrap_or_default()
        .iter()
        .filter_map(|t| crate::doc::resolve(t))
        .collect();
    for (p, _) in o.files {
        let fixed = matches!(p.as_str(), "toa.yaml" | "symbol.rnl" | "README.md") || p.starts_with("LICENSE");
        if !fixed && !(is_image(p) && shown.contains(p)) {
            out_file(&mut v, p);
        }
    }
    let Some(pins) = m.get("pins").and_then(|n| v.list(n, "pins", 1)) else { return };
    let mut seen = BTreeSet::new();
    for (i, p) in pins.iter().enumerate() {
        let at = format!("pins/{i}");
        if !v.fields(p, &at, &["name", "kind", "direction", "width", "description"], &["name"]) {
            continue;
        }
        if let Some(n) = p.get("name") {
            if let Some(name) = v.shaped(n, &format!("{at}/name"), pin_name, "a port name: [a-zA-Z][a-zA-Z0-9_]{0,31}")
            {
                // unique without regard to case (checks at sealing)
                if !seen.insert(name.to_ascii_lowercase()) {
                    v.err("E_PIN_TWICE", &format!("{at}/name"), format!("the pin '{name}' is named twice"));
                }
            }
        }
        if let Some(n) = p.get("kind") {
            v.one_of(n, &format!("{at}/kind"), &["analog", "digital", "supply", "ground"]);
        }
        if let Some(n) = p.get("direction") {
            v.one_of(n, &format!("{at}/direction"), &["in", "out", "inout"]);
        }
        if let Some(n) = p.get("width") {
            if !matches!(n.value, toa_core::manifest::Value::Int(w) if w >= 1) {
                v.err("E_FIELD", &format!("{at}/width"), "width is an integer from 1");
            }
        }
        if let Some(n) = p.get("description") {
            v.string(n, &format!("{at}/description"));
        }
    }
}

/// Whether a path names an image by its extension.
fn is_image(p: &str) -> bool {
    let ext = p.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp")
}

fn out_file(v: &mut V, p: &str) {
    v.out.push(Problem::error(
        "E_IFC_FILE",
        "types/ifc",
        p,
        "an interface holds only toa.yaml, symbol.rnl, README.md, LICENSE files and images README.md shows; \
         models, netlists and longer texts belong in circuits, testbenches and documents",
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use toa_core::manifest;

    fn problems(text: &str) -> Vec<&'static str> {
        let m = manifest::parse(text.as_bytes()).unwrap();
        let files = vec![("toa.yaml".to_string(), text.as_bytes().to_vec())];
        crate::check("ifc", &Object { manifest: &m, files: &files, lock: None }, &crate::Nothing)
            .iter()
            .map(|p| p.code)
            .collect()
    }

    #[test]
    fn the_example_passes_and_mistakes_are_found() {
        let ok = "type: ifc\nlicense: MIT\nname: strongarm-comp\ntitle: StrongARM latched comparator\npins:\n\
                  - {name: inp, kind: analog, direction: in}\n- {name: vdd, kind: supply}\n";
        assert!(problems(ok).is_empty(), "{:?}", problems(ok));
        assert!(
            short_name("strongarm-comp")
                && short_name("x")
                && !short_name("Strong")
                && !short_name("a--b")
                && !short_name("a-")
        );
        let bad =
            "type: ifc\nlicense: MIT\nname: Strong\npins:\n- {name: inp, kind: wire}\n- {name: INP}\nversion: 2\n";
        assert_eq!(problems(bad), ["E_FIELD_UNKNOWN", "E_FIELD_MISSING", "E_FIELD", "E_FIELD", "E_PIN_TWICE"]);
    }

    #[test]
    fn only_images_that_readme_shows() {
        let text = "type: ifc\nlicense: MIT\nname: x\ntitle: X\npins:\n- {name: a}\n";
        let m = manifest::parse(text.as_bytes()).unwrap();
        let files: Vec<(String, Vec<u8>)> = [
            ("toa.yaml", ""),
            ("README.md", "![block](img/block.svg)\n"),
            ("img/block.svg", ""),
            ("other.png", ""),
            ("model.va", ""),
            ("symbol.rnl", ""),
        ]
        .iter()
        .map(|(p, b)| (p.to_string(), b.as_bytes().to_vec()))
        .collect();
        let r = crate::check("ifc", &Object { manifest: &m, files: &files, lock: None }, &crate::Nothing);
        let paths: Vec<&str> = r.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, ["other.png", "model.va"], "{r:?}");
    }
}
