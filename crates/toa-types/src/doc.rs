//! Type `doc`, the document (`spec/types/en/doc.md`).

use crate::v::V;
use crate::Object;
use toa_core::Problem;

pub(crate) fn check(o: &Object, out: &mut Vec<Problem>) {
    let mut v = V { out, section: "types/doc", file: "toa.yaml" };
    let m = o.manifest;
    if !v.fields(m, "", &crate::allowed(&["refs", "date"]), &[]) {
        return;
    }
    if let Some(refs) = m.get("refs").and_then(|n| v.list(n, "refs", 0)) {
        for (i, r) in refs.iter().enumerate() {
            let at = format!("refs/{i}");
            let ok = r.as_str().is_some_and(|s| {
                matches!(toa_core::name::form(s), toa_core::name::Form::Object | toa_core::name::Form::Draft)
            });
            if !ok {
                v.err("E_FIELD", &at, "refs lists object names");
            }
        }
    }
    if let Some(n) = m.get("date") {
        v.shaped(n, "date", date, "a date 'YYYY-MM-DD', in quotes");
    }

    // 1. README.md exists; 2. its relative links and images point to files in the object
    let mut f = V { out: v.out, section: "types/doc", file: "README.md" };
    match o.bytes("README.md") {
        None => f.err("E_DOC_README", "", "a document's body is its README.md, which is missing"),
        Some(b) => {
            let text = String::from_utf8_lossy(b);
            for target in relative_links(&text) {
                match resolve(&target) {
                    Some(p) if o.has(&p) => {}
                    _ => f.err("E_DOC_LINK", "", format!("the link '{target}' points to no file in the object")),
                }
            }
        }
    }
}

fn date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// The targets of Markdown links and images, `[text](target)` and `![alt](target)`, that are
/// relative paths: not a URL, not an anchor of this page.
pub(crate) fn relative_links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        let end = rest.find(')').unwrap_or(rest.len());
        let raw = rest[..end].trim();
        // `(path "title")` and `(<path>)`
        let target = raw.split_whitespace().next().unwrap_or("").trim_start_matches('<').trim_end_matches('>');
        let is_url = target.contains("://") || target.starts_with("mailto:") || target.starts_with("data:");
        if !target.is_empty() && !is_url && !target.starts_with('#') {
            out.push(target.to_string());
        }
        rest = &rest[end.min(rest.len())..];
    }
    out
}

/// A relative link as a path of the object, without `#anchor` or `?query`; `None` when it leaves
/// the object or is absolute.
pub(crate) fn resolve(target: &str) -> Option<String> {
    let t = target.split(['#', '?']).next().unwrap_or("");
    if t.starts_with('/') || t.contains('\\') || t.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for p in t.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            x => parts.push(x),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use toa_core::manifest;

    #[test]
    fn readme_and_links() {
        let text =
            "type: doc\nlicense: CC-BY-4.0\nrefs: [ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5]\ndate: '2026-10-28'\n";
        let m = manifest::parse(text.as_bytes()).unwrap();
        let readme =
            b"# T\n![plot](img/a.png) see [x](https://e.org/) and [y](#top) and [z](./b.md#s) [w](../out.md)\n";
        let files = vec![
            ("toa.yaml".to_string(), text.as_bytes().to_vec()),
            ("README.md".to_string(), readme.to_vec()),
            ("img/a.png".to_string(), vec![]),
        ];
        let codes: Vec<_> = crate::check("doc", &Object { manifest: &m, files: &files, lock: None }, &crate::Nothing)
            .iter()
            .map(|p| (p.code, p.message.clone()))
            .collect();
        assert_eq!(codes.len(), 2, "{codes:?}");
        assert!(codes[0].1.contains("./b.md#s") && codes[1].1.contains("../out.md"));
        assert!(date("2026-10-28") && !date("2026-1-28"));
        let none = crate::check("doc", &Object { manifest: &m, files: &files[..1], lock: None }, &crate::Nothing);
        assert_eq!(none[0].code, "E_DOC_README");
    }
}
