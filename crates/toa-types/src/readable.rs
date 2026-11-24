//! Readable names (6.2) and labels (5.1): the names a tool writes for people, from fields and the
//! generation (3.4). Free text that no tool infers anything from.

use toa_core::manifest::Node;
use toa_core::name;

/// The generation of an object (3.4): the one its `history.json` gives; an object without
/// `base`, which has no `history.json`, is of generation 1.
pub fn generation(history: Option<&toa_core::history::History>) -> u64 {
    history.map_or(toa_core::history::FIRST, |h| h.generation)
}

/// The label a type specification asks for (5.1): the readable name without the type directory
/// and the slashes at its ends. `path` is where the object is in its library (for `evd` and `doc`,
/// whose readable names hold what the manifest does not); `ifc_name` gives the `name` of an
/// interface when it can be found.
///
/// - `ifc`: `<name>`;
/// - `ckt`, `tb`: `<interface name>/<name>.g<generation>`, the interface name being the first 12
///   characters of the object name in `ifc` when it cannot be found;
/// - `lib`: `<name>.g<generation>`;
/// - `evd`, `doc`: the levels of `path` between the type directory and the object directory.
pub fn label(
    object: &str,
    manifest: Option<&Node>,
    generation: u64,
    path: &str,
    ifc_name: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let prefix = name::prefix_of(object)?;
    let m = manifest?;
    let from_path = || -> Option<String> {
        let rest = path.strip_prefix(&format!("{prefix}/"))?;
        let (dirs, _) = rest.rsplit_once('/')?;
        (!dirs.is_empty()).then(|| dirs.to_string())
    };
    let text = match prefix {
        crate::IFC => m.get("name")?.as_str()?.to_string(),
        crate::CKT | crate::TB => {
            let target = m.get("ifc")?.as_str()?;
            let ifc = ifc_name(target).unwrap_or_else(|| target.chars().take(12).collect());
            format!("{ifc}/{}.g{generation}", m.get("name")?.as_str()?)
        }
        crate::LIB => format!("{}.g{generation}", m.get("name")?.as_str()?),
        crate::EVD | crate::DOC => from_path()?,
        _ => return None,
    };
    Some(text.chars().take(toa_core::source::MAX_TEXT).collect())
}

/// The title to show for an object: its `title` (3.2.1); for a document that has none, the first
/// heading of its `README.md` (`types/doc`). `None` when neither gives one.
pub fn title(prefix: &str, manifest: Option<&Node>, readme: Option<&[u8]>) -> Option<String> {
    if let Some(t) = manifest.and_then(|m| m.get("title")).and_then(Node::as_str) {
        return Some(t.to_string());
    }
    if prefix != crate::DOC {
        return None;
    }
    first_heading(std::str::from_utf8(readme?).ok()?)
}

/// The first heading of a Markdown text, by CommonMark: an ATX heading (`#` to `######` and a
/// space) or a setext heading (a paragraph underlined with `=` or `-`), not inside a fenced code
/// block nor a YAML front matter at the top. Its text is trimmed, without the closing `#`s, its
/// lines joined by a space, and cut to the length of a title; an empty heading is passed over.
pub fn first_heading(text: &str) -> Option<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines: Vec<&str> = text.lines().collect();
    // a front matter: `---` on the first line, up to the next `---` or `...`
    if lines.first().is_some_and(|l| l.trim_end() == "---") {
        if let Some(end) = lines[1..].iter().position(|l| matches!(l.trim_end(), "---" | "...")) {
            lines.drain(..end + 2);
        }
    }
    let cut = |s: &str| -> Option<String> {
        let s = s.trim();
        (!s.is_empty()).then(|| s.chars().take(toa_core::object::MAX_TITLE).collect())
    };
    let mut fence: Option<(char, usize)> = None;
    let mut paragraph: Vec<&str> = Vec::new();
    for line in lines {
        let body = line.trim_start_matches(' ');
        let shallow = line.len() - body.len() <= 3;
        if shallow {
            if let Some(c @ ('`' | '~')) = body.chars().next() {
                let n = body.chars().take_while(|&x| x == c).count();
                if n >= 3 {
                    match fence {
                        None => {
                            fence = Some((c, n));
                            paragraph.clear();
                            continue;
                        }
                        Some((fc, fnum)) if fc == c && n >= fnum && body[n..].trim().is_empty() => {
                            fence = None;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
        }
        if fence.is_some() {
            continue;
        }
        if shallow {
            if let Some(h) = atx(body) {
                if let Some(t) = cut(h) {
                    return Some(t);
                }
                paragraph.clear();
                continue;
            }
            let under = body.trim_end();
            let setext = !under.is_empty() && (under.chars().all(|c| c == '=') || under.chars().all(|c| c == '-'));
            if setext && !paragraph.is_empty() {
                let joined: Vec<&str> = paragraph.iter().map(|l| l.trim()).collect();
                if let Some(t) = cut(&joined.join(" ")) {
                    return Some(t);
                }
                paragraph.clear();
                continue;
            }
        }
        if line.trim().is_empty() {
            paragraph.clear();
        } else if shallow && starts_block(body) {
            // a list item or a block quote is not the text of a setext heading
            paragraph.clear();
        } else if shallow || !paragraph.is_empty() {
            paragraph.push(line);
        }
    }
    None
}

/// Whether a line starts a list item (`- `, `* `, `+ `, `1. `, `1) `) or a block quote.
fn starts_block(body: &str) -> bool {
    if body.starts_with('>') || ["- ", "* ", "+ "].iter().any(|m| body.starts_with(m)) {
        return true;
    }
    let digits = body.chars().take_while(char::is_ascii_digit).count();
    (1..=9).contains(&digits) && [". ", ") "].iter().any(|m| body[digits..].starts_with(m))
}

/// The text of an ATX heading line (without its indentation), or `None` when it is not one.
fn atx(body: &str) -> Option<&str> {
    let n = body.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&n) {
        return None;
    }
    let rest = &body[n..];
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let rest = rest.trim();
    // a closing sequence of `#`s, after a space or alone
    let without = rest.trim_end_matches('#');
    if without.is_empty() {
        return Some("");
    }
    Some(if without.ends_with([' ', '\t']) { without } else { rest })
}

/// The recommended directory of an object (6.2), without the object directory: the type
/// directory and the label, such as `ckt/strongarm-comp/wrap-lee.g2`.
pub fn directory(prefix: &str, label: &str) -> String {
    format!("{prefix}/{label}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use toa_core::manifest;

    #[test]
    fn labels() {
        let m =
            manifest::parse(b"type: ckt\nifc: ifc_945cdda745e8f44886e3aa17d7752db313d91136\nname: wrap-lee\n").unwrap();
        let none = |_: &str| None;
        let ckt = "ckt_1ec6249cf01c000f19632bde9801c5af4cca1698";
        assert_eq!(label(ckt, Some(&m), 2, "ckt/x/y/ckt_1", &none).unwrap(), "ifc_945cdda7/wrap-lee.g2");
        let named = |_: &str| Some("strongarm-comp".to_string());
        assert_eq!(label(ckt, Some(&m), generation(None), "", &named).unwrap(), "strongarm-comp/wrap-lee.g1");
        assert_eq!(directory("ckt", "strongarm-comp/wrap-lee.g1"), "ckt/strongarm-comp/wrap-lee.g1");
        let v = manifest::parse(b"type: evd\n").unwrap();
        let evd = "evd_b7d7795988710dd71b9bca398248b997883b6c8b";
        assert_eq!(label(evd, Some(&v), 1, "evd/sc/20261103T1200Z-x/evd_b7", &none).unwrap(), "sc/20261103T1200Z-x");
        let ifc = manifest::parse(b"type: ifc\nname: strongarm-comp\n").unwrap();
        assert_eq!(
            label("ifc_945cdda745e8f44886e3aa17d7752db313d91136", Some(&ifc), 1, "", &none).unwrap(),
            "strongarm-comp"
        );
        let lib = manifest::parse(b"type: lib\nname: toa-official\n").unwrap();
        let h = toa_core::history::History { history: vec![ckt.into()], generation: 3 };
        assert_eq!(
            label("lib_5d0e7b3a45e8f44886e3aa17d7752db313d91136", Some(&lib), generation(Some(&h)), "", &none).unwrap(),
            "toa-official.g3"
        );
    }

    #[test]
    fn titles() {
        let doc = manifest::parse(b"type: doc\nlicense: MIT\n").unwrap();
        let titled = manifest::parse(b"type: doc\nlicense: MIT\ntitle: Written\n").unwrap();
        let readme = b"Some words first.\n\n## Kickback  ##\n\n# Later\n";
        assert_eq!(title("doc", Some(&doc), Some(readme)).unwrap(), "Kickback");
        assert_eq!(title("doc", Some(&titled), Some(readme)).unwrap(), "Written", "title wins");
        assert_eq!(title("doc", Some(&doc), None), None);
        let ckt = manifest::parse(b"type: ckt\nlicense: MIT\n").unwrap();
        assert_eq!(title("ckt", Some(&ckt), Some(readme)), None, "only a document reads its README");

        let h = |s: &str| first_heading(s);
        assert_eq!(h("# A title #\n").unwrap(), "A title");
        assert_eq!(h("# C# #\n").unwrap(), "C#");
        assert_eq!(h("#hashtag\n\nSet\ntext\n===\n").unwrap(), "Set text", "#hashtag is no heading");
        assert_eq!(h("```\n# not\n```\n#\n    # code\nReal\n---\n").unwrap(), "Real");
        assert_eq!(h("---\ntitle: front\n---\n\n# After the front matter\n").unwrap(), "After the front matter");
        assert_eq!(h("\u{feff}  ### Indented by two\n").unwrap(), "Indented by two");
        assert_eq!(h("- item\n---\n\n> quote\n===\n\n1. one\n---\nno heading\n"), None);
        assert_eq!(h(&format!("# {}\n", "x".repeat(300))).unwrap().len(), 200);
    }
}
