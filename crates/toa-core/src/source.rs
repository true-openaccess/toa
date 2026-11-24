//! Source lists, files ending in `.source.jsonl` (5.1).
//!
//! Two readings. The lenient one is for lists written by others: it drops what is wrong, from the
//! whole list down to one source, and keeps the rest. The strict one is for lists on a publication
//! branch (6.4 item 6): everything must be right, and every problem is reported.

use serde::ser::Serialize;
use serde_json::{Map, Value};

use crate::name;
use crate::problem::Problem;
use crate::tree::component_problem;

/// The header line, the first line of a list.
pub const HEADER: &str = "source/1";
/// Suffix of every source list.
pub const SUFFIX: &str = ".source.jsonl";
/// The library list at the root of a library, and the directory of its volumes (5.2).
pub const LIBRARY_LIST: &str = "library.source.jsonl";
pub const LIBRARY_VOLUMES: &str = "library.source";
/// The default name of a workspace list (8.1).
pub const WORKSPACE_LIST: &str = "workspace.source.jsonl";

/// Limits of 5.1 and 5.2.
pub const MAX_LINE: usize = 64 * 1024;
pub const MAX_SIDECAR_LIST: usize = 1024 * 1024;
pub const MAX_VOLUME: usize = 16 * 1024 * 1024;
pub const MAX_SOURCES: usize = 3;
pub const MAX_ADDRESS: usize = 512;
pub const MAX_LOCAL: usize = 1024;
pub const MAX_TEXT: usize = 256;
pub const MAX_INCLUDE: usize = 256;
pub const MAX_GROUP: usize = 128;
pub const MAX_INCLUDE_DEPTH: usize = 8;

/// Where an object or a list can be had (5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Git { git: String, commit: Option<String>, path: Option<String> },
    Local(String),
}

/// A line that says one object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjectLine {
    pub object: String,
    pub label: Option<String>,
    pub refs: Option<Vec<String>>,
    pub license: Option<String>,
    pub group: Option<String>,
    pub removed: bool,
    /// Only in a library list: the library recommends this object in place of the earlier
    /// generations of its series (5.2).
    pub latest: bool,
    pub sources: Vec<Source>,
}

/// A line that brings in another list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IncludeLine {
    pub include: String,
    pub sources: Vec<Source>,
    pub group: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Object(ObjectLine),
    Include(IncludeLine),
}

/// What kind of list, for the strict reading: which local paths and fields it may hold (5.1, 5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `<object name>.source.jsonl` beside an object: no local sources on a publication branch.
    Sidecar,
    /// `library.source.jsonl` or one of its volumes: local sources are paths inside the library,
    /// and `removed` may appear.
    Library,
    /// Any other list on a publication branch.
    Other,
    /// `members.source.jsonl` at the top of an object (3.3): object lines only, no `removed` or
    /// `latest`, only `git` sources; read strictly within the limits of an object.
    Members,
}

// ------------------------------------------------------------------ syntax of the fields

/// A git address (5.1): `https://host[:port]/path` or `ssh://[user@]host[:port]/path`.
pub fn address_problem(a: &str) -> Option<String> {
    if a.len() > MAX_ADDRESS {
        return Some(format!("an address has at most {MAX_ADDRESS} characters"));
    }
    let (rest, ssh) = if let Some(r) = a.strip_prefix("https://") {
        (r, false)
    } else if let Some(r) = a.strip_prefix("ssh://") {
        (r, true)
    } else {
        return Some("an address starts with https:// or ssh://".into());
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => return Some("an address has a path".into()),
    };
    let host_port = match authority.rsplit_once('@') {
        Some((user, hp)) => {
            if !ssh {
                return Some("only an ssh address has a user".into());
            }
            let ok = !user.is_empty()
                && user.len() <= 64
                && !user.starts_with('-')
                && user.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'));
            if !ok {
                return Some("the user of an address is letters, digits, '.', '_' or '-'".into());
            }
            hp
        }
        None => authority,
    };
    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (host_port, None),
    };
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    };
    if host.is_empty() || !host.split('.').all(label_ok) {
        return Some("the host of an address is labels of letters, digits and '-' between dots".into());
    }
    if let Some(p) = port {
        if !(!p.is_empty() && p.len() <= 5 && p.bytes().all(|c| c.is_ascii_digit()))
            || !(1..=65535).contains(&p.parse::<u32>().unwrap_or(0))
        {
            return Some("the port of an address is 1 to 65535".into());
        }
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return Some("an address has a path".into());
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Some("the path of an address has no empty, '.' or '..' levels".into());
        }
        if !seg.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'~' | b'-')) {
            return Some("the path of an address is letters, digits, '.', '_', '~', '-' and '/'".into());
        }
    }
    None
}

/// The host of a valid address, in lower case.
pub fn host_of(address: &str) -> Option<String> {
    if address_problem(address).is_some() {
        return None;
    }
    let rest = address.split_once("://")?.1;
    let authority = rest.split('/').next()?;
    let hp = authority.rsplit_once('@').map_or(authority, |p| p.1);
    Some(hp.split(':').next()?.to_ascii_lowercase())
}

/// Whether two addresses are the same (5.1): the host in lower case and no final `/`.
pub fn same_address(a: &str, b: &str) -> bool {
    fn norm(a: &str) -> String {
        let a = a.strip_suffix('/').unwrap_or(a);
        match a.split_once("://") {
            Some((scheme, rest)) => {
                let (auth, path) = rest.split_once('/').unwrap_or((rest, ""));
                let (user, hp) = match auth.rsplit_once('@') {
                    Some((u, h)) => (format!("{u}@"), h),
                    None => (String::new(), auth),
                };
                format!("{scheme}://{user}{}/{path}", hp.to_ascii_lowercase())
            }
            None => a.to_string(),
        }
    }
    norm(a) == norm(b)
}

pub fn is_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// A path of names, each by 3.1 item 3.
pub fn names_path_problem(p: &str) -> Option<String> {
    if p.is_empty() {
        return Some("a path is not empty".into());
    }
    p.split('/').find_map(|seg| component_problem(seg, false).map(|w| format!("'{seg}': {w}")))
}

/// A group (5.1): 1 to 4 levels, each by 3.1 item 3, at most 128 characters.
pub fn group_problem(g: &str) -> Option<String> {
    if g.len() > MAX_GROUP {
        return Some(format!("a group has at most {MAX_GROUP} characters"));
    }
    if g.split('/').count() > 4 {
        return Some("a group has 1 to 4 levels".into());
    }
    names_path_problem(g)
}

/// The path of an include line (5.1).
pub fn include_problem(p: &str) -> Option<String> {
    if p.len() > MAX_INCLUDE {
        return Some(format!("an include path has at most {MAX_INCLUDE} characters"));
    }
    if !p.ends_with(SUFFIX) {
        return Some(format!("an include path ends with {SUFFIX}"));
    }
    names_path_problem(p)
}

/// A publication branch (6.3).
pub fn branch_problem(b: &str) -> Option<String> {
    let bytes = b.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 100
        && bytes[0].is_ascii_alphanumeric()
        && bytes.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'/' | b'-'))
        && !b.contains("..")
        && !b.ends_with('/')
        && !b.ends_with(".lock");
    (!ok).then(|| "a branch is letters, digits, '.', '_', '/', '-', starting with a letter or digit".into())
}

/// A local path (5.1): at most 1024 characters, `/` between levels, no control or format characters.
pub fn local_problem(p: &str) -> Option<String> {
    if p.is_empty() || p.len() > MAX_LOCAL {
        return Some(format!("a local path has 1 to {MAX_LOCAL} characters"));
    }
    if p.contains('\\') {
        return Some("a local path has '/' between levels".into());
    }
    if p.chars().any(|c| c.is_control() || is_format_char(c)) {
        return Some("a local path has no control or invisible format characters".into());
    }
    None
}

/// Unicode general category Cf, the invisible format characters that matter in paths.
fn is_format_char(c: char) -> bool {
    matches!(c as u32,
        0xAD | 0x600..=0x605 | 0x61C | 0x6DD | 0x70F | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E
        | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB | 0xE0001 | 0xE0020..=0xE007F)
}

/// Whether a local path is absolute: `/…`, `X:/…`, or `//host/…`.
pub fn is_absolute_local(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/') || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/')
}

// ------------------------------------------------------------------ reading

/// Why one source is wrong, or the source.
pub fn read_source(v: &Value, include: bool) -> Result<Option<Source>, String> {
    let Some(m) = v.as_object() else {
        return Err("a source is a JSON object".into());
    };
    if let Some(g) = m.get("git") {
        if m.keys().any(|k| !matches!(k.as_str(), "git" | "commit" | "path")) {
            return Err("a git source has only git, commit and path".into());
        }
        let Some(g) = g.as_str() else { return Err("git is a string".into()) };
        if let Some(w) = address_problem(g) {
            return Err(w);
        }
        let commit = match m.get("commit") {
            None => None,
            Some(Value::String(c)) if is_commit(c) => Some(c.clone()),
            Some(_) => return Err("commit is 40 lowercase hexadecimal digits".into()),
        };
        let path = match m.get("path") {
            None => None,
            Some(_) if include => return Err("a git source of an include line has no path".into()),
            Some(Value::String(p)) => match names_path_problem(p) {
                None => Some(p.clone()),
                Some(w) => return Err(w),
            },
            Some(_) => return Err("path is a string".into()),
        };
        return Ok(Some(Source::Git { git: g.to_string(), commit, path }));
    }
    if let Some(l) = m.get("local") {
        if m.len() != 1 {
            return Err("a local source has only local".into());
        }
        let Some(l) = l.as_str() else { return Err("local is a string".into()) };
        if let Some(w) = local_problem(l) {
            return Err(w);
        }
        return Ok(Some(Source::Local(l.to_string())));
    }
    // a later form of source: ignored (5.1)
    Ok(None)
}

/// Reading a list collects lines and, for the strict reading, problems.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    pub lines: Vec<Line>,
    /// True when the whole list was set aside (too large, unknown header version).
    pub ignored: bool,
    pub problems: Vec<Problem>,
}

struct Ctx<'a> {
    strict: bool,
    kind: Kind,
    file: &'a str,
    problems: Vec<Problem>,
}

impl Ctx<'_> {
    fn problem(&mut self, line: usize, msg: impl Into<String>) {
        if self.strict {
            self.problems.push(Problem::error("E_SOURCE_LIST", "5.1", format!("{}:{line}", self.file), msg));
        }
    }
}

/// Read a list. `strict` gives the reading of a publication branch; `limit` is the largest list
/// size for this kind of list; `file` names it in problems.
pub fn read(bytes: &[u8], strict: bool, kind: Kind, limit: usize, file: &str) -> Reading {
    let mut cx = Ctx { strict, kind, file, problems: Vec::new() };
    let mut out = Reading::default();
    if bytes.len() > limit {
        out.ignored = true;
        cx.problem(0, format!("a list of this kind has at most {limit} bytes"));
        out.problems = cx.problems;
        return out;
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => {
            out.ignored = true;
            cx.problem(0, "a list is UTF-8");
            out.problems = cx.problems;
            return out;
        }
    };
    if text.starts_with('\u{feff}') {
        cx.problem(1, "a list has no byte order mark");
    }
    if text.contains('\r') {
        cx.problem(0, "a list has LF line ends only");
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split('\n').enumerate().peekable();
    // the header
    let mut first_is_header = false;
    if let Some((_, first)) = lines.peek() {
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(first.trim_end_matches('\r')) {
            if let Some(v) = m.get("toa") {
                first_is_header = true;
                if v.as_str() != Some(HEADER) {
                    out.ignored = true;
                    cx.problem(1, format!("the header is {{\"toa\": \"{HEADER}\"}}; this version is not known"));
                    out.problems = cx.problems;
                    return out;
                }
                if m.len() != 1 {
                    cx.problem(1, format!("the header is only {{\"toa\": \"{HEADER}\"}}"));
                }
            }
        }
    }
    if first_is_header {
        lines.next();
    } else {
        cx.problem(1, format!("the first line is the header {{\"toa\": \"{HEADER}\"}}"));
    }
    let last = text.split('\n').count();
    for (i, raw) in lines {
        let n = i + 1;
        let raw = raw.trim_end_matches('\r');
        if raw.is_empty() {
            // the end of the file after its last line end is no line; any other empty line is
            if n < last {
                cx.problem(n, "an empty line");
            }
            continue;
        }
        if raw.len() > MAX_LINE {
            cx.problem(n, format!("a line has at most {MAX_LINE} bytes"));
            continue;
        }
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(raw) else {
            cx.problem(n, "a line is a JSON object");
            continue;
        };
        if let Some(line) = read_line(&m, n, &mut cx) {
            out.lines.push(line);
        }
    }
    out.problems = cx.problems;
    out
}

fn read_sources(v: Option<&Value>, include: bool, n: usize, cx: &mut Ctx) -> Vec<Source> {
    let mut out = Vec::new();
    let Some(v) = v else { return out };
    let Some(items) = v.as_array() else {
        cx.problem(n, "sources is a list");
        return out;
    };
    if items.len() > MAX_SOURCES {
        cx.problem(n, format!("at most {MAX_SOURCES} sources on a line"));
    }
    for s in items {
        match read_source(s, include) {
            Ok(Some(src)) => out.push(src),
            Ok(None) => cx.problem(n, "a source is a git or a local source"),
            Err(w) => cx.problem(n, w),
        }
    }
    // the lenient reading takes the first three (5.1)
    out.truncate(MAX_SOURCES);
    out
}

fn check_locals(sources: &[Source], n: usize, cx: &mut Ctx) {
    for s in sources {
        if let Source::Local(p) = s {
            match cx.kind {
                Kind::Sidecar => cx.problem(n, "a list beside an object has no local sources on a publication branch; run `toa publish` to tidy it before publishing (5.3)"),
                Kind::Library => {
                    if let Some(w) = names_path_problem(p) {
                        cx.problem(n, format!("a local source in a library list is a path inside the library: {w}"));
                    }
                }
                Kind::Members => cx.problem(n, "a list of members has only git sources (3.3)"),
                Kind::Other => {}
            }
        }
    }
}

fn read_line(m: &Map<String, Value>, n: usize, cx: &mut Ctx) -> Option<Line> {
    match (m.get("object"), m.get("include")) {
        (Some(_), Some(_)) | (None, None) => {
            cx.problem(n, "a line has either object or include");
            None
        }
        (Some(o), None) => {
            let Some(object) = o.as_str().filter(|s| name::is_object_name(s)) else {
                cx.problem(n, "object is an object name");
                return None;
            };
            let mut line = ObjectLine { object: object.to_string(), ..Default::default() };
            for key in ["label", "license"] {
                if let Some(v) = m.get(key) {
                    match v.as_str() {
                        Some(s) if s.chars().count() <= MAX_TEXT => {
                            if key == "label" {
                                line.label = Some(s.to_string());
                            } else {
                                line.license = Some(s.to_string());
                            }
                        }
                        _ => cx.problem(n, format!("{key} is a string of at most {MAX_TEXT} characters")),
                    }
                }
            }
            if let Some(v) = m.get("refs") {
                match v.as_array().and_then(|a| {
                    a.iter().map(|x| x.as_str().filter(|s| name::is_object_name(s)).map(str::to_string)).collect()
                }) {
                    Some(refs) => line.refs = Some(refs),
                    None => cx.problem(n, "refs is a list of object names"),
                }
            }
            if let Some(v) = m.get("group") {
                match v.as_str().map(|g| (g, group_problem(g))) {
                    Some((g, None)) => line.group = Some(g.to_string()),
                    Some((_, Some(w))) => cx.problem(n, w),
                    None => cx.problem(n, "group is a string"),
                }
            }
            if let Some(v) = m.get("removed") {
                match v {
                    Value::Bool(true) if cx.kind == Kind::Library || !cx.strict => line.removed = true,
                    Value::Bool(true) => cx.problem(n, "removed appears only in a library list"),
                    _ => cx.problem(n, "removed is true"),
                }
            }
            if let Some(v) = m.get("latest") {
                // written anywhere but in a library list, it means nothing (5.2)
                match v {
                    Value::Bool(true) if cx.kind == Kind::Library => line.latest = true,
                    Value::Bool(true) if !cx.strict => {}
                    Value::Bool(true) => cx.problem(n, "latest appears only in a library list"),
                    _ => cx.problem(n, "latest is true"),
                }
            }
            line.sources = read_sources(m.get("sources"), false, n, cx);
            check_locals(&line.sources, n, cx);
            Some(Line::Object(line))
        }
        (None, Some(_)) if cx.kind == Kind::Members => {
            cx.problem(n, "a list of members has no include lines: an object that never changes cannot take in a list that changes (3.3)");
            None
        }
        (None, Some(inc)) => {
            let Some(include) = inc.as_str() else {
                cx.problem(n, "include is a string");
                return None;
            };
            if let Some(w) = include_problem(include) {
                // a bad include line is dropped whole (5.1)
                cx.problem(n, w);
                return None;
            }
            let mut line = IncludeLine { include: include.to_string(), ..Default::default() };
            if let Some(v) = m.get("group") {
                match v.as_str().map(|g| (g, group_problem(g))) {
                    Some((g, None)) => line.group = Some(g.to_string()),
                    Some((_, Some(w))) => cx.problem(n, w),
                    None => cx.problem(n, "group is a string"),
                }
            }
            if let Some(v) = m.get("branch") {
                match v.as_str().map(|b| (b, branch_problem(b))) {
                    Some((b, None)) => line.branch = Some(b.to_string()),
                    Some((_, Some(w))) => cx.problem(n, w),
                    None => cx.problem(n, "branch is a string"),
                }
            }
            line.sources = read_sources(m.get("sources"), true, n, cx);
            check_locals(&line.sources, n, cx);
            Some(Line::Include(line))
        }
    }
}

/// Merge the object lines of a reading by object name (5.1): sources in order without repeats,
/// at most three; the first value of each cached field; `removed` and `latest` if any line says
/// so.
pub fn merged_objects(lines: &[Line]) -> Vec<ObjectLine> {
    let mut out: Vec<ObjectLine> = Vec::new();
    for l in lines {
        let Line::Object(o) = l else { continue };
        match out.iter_mut().find(|x| x.object == o.object) {
            None => out.push(o.clone()),
            Some(x) => {
                x.label = x.label.take().or_else(|| o.label.clone());
                x.refs = x.refs.take().or_else(|| o.refs.clone());
                x.license = x.license.take().or_else(|| o.license.clone());
                x.group = x.group.take().or_else(|| o.group.clone());
                x.removed |= o.removed;
                x.latest |= o.latest;
                for s in &o.sources {
                    if !x.sources.contains(s) {
                        x.sources.push(s.clone());
                    }
                }
                x.sources.truncate(MAX_SOURCES);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ writing

/// JSON with the separators `", "` and `": "` on one line, as the sample lists are written.
struct Spaced;

impl serde_json::ser::Formatter for Spaced {
    fn begin_array_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            w.write_all(b", ")
        }
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            w.write_all(b", ")
        }
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        w.write_all(b": ")
    }
}

fn spaced(v: &Value) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, Spaced);
    v.serialize(&mut ser).expect("JSON values always serialize");
    String::from_utf8(buf).expect("serde_json writes UTF-8")
}

fn source_value(s: &Source) -> Value {
    let mut m = Map::new();
    match s {
        Source::Git { git, commit, path } => {
            m.insert("git".into(), git.clone().into());
            if let Some(c) = commit {
                m.insert("commit".into(), c.clone().into());
            }
            if let Some(p) = path {
                m.insert("path".into(), p.clone().into());
            }
        }
        Source::Local(l) => {
            m.insert("local".into(), l.clone().into());
        }
    }
    Value::Object(m)
}

/// One line as JSON, fields in a fixed order.
pub fn line_text(line: &Line) -> String {
    let mut m = Map::new();
    let sources = |s: &[Source]| Value::Array(s.iter().map(source_value).collect());
    match line {
        Line::Object(o) => {
            m.insert("object".into(), o.object.clone().into());
            if let Some(v) = &o.label {
                m.insert("label".into(), v.clone().into());
            }
            if let Some(v) = &o.refs {
                m.insert("refs".into(), v.clone().into());
            }
            if let Some(v) = &o.license {
                m.insert("license".into(), v.clone().into());
            }
            if let Some(v) = &o.group {
                m.insert("group".into(), v.clone().into());
            }
            if o.removed {
                m.insert("removed".into(), true.into());
            }
            if o.latest {
                m.insert("latest".into(), true.into());
            }
            if !o.sources.is_empty() {
                m.insert("sources".into(), sources(&o.sources));
            }
        }
        Line::Include(i) => {
            m.insert("include".into(), i.include.clone().into());
            if let Some(v) = &i.group {
                m.insert("group".into(), v.clone().into());
            }
            if let Some(v) = &i.branch {
                m.insert("branch".into(), v.clone().into());
            }
            if !i.sources.is_empty() {
                m.insert("sources".into(), sources(&i.sources));
            }
        }
    }
    spaced(&Value::Object(m))
}

/// A whole list: the header, then one line each, each ending in LF.
pub fn write(lines: &[Line]) -> String {
    let mut s = format!("{{\"toa\": \"{HEADER}\"}}\n");
    for l in lines {
        s.push_str(&line_text(l));
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "imp_1ec6249cf01c000f19632bde9801c5af4cca1698";
    const B: &str = "ip_945cdda745e8f44886e3aa17d7752db313d91136";

    #[test]
    fn addresses() {
        for a in ["https://example.org/lib.git", "ssh://git@example.org:2222/a/b", "https://h.example.org/x/"] {
            assert_eq!(address_problem(a), None, "{a}");
        }
        for a in [
            "http://example.org/x",
            "https://user@example.org/x",
            "https://example.org",
            "https://example.org/a/../b",
            "https://example.org/a?x=1",
            "https://-bad.org/x",
            "https://example.org:0/x",
            "git@example.org:x",
            "ssh://-oProxyCommand=x@h/p",
            "--upload-pack=x",
        ] {
            assert!(address_problem(a).is_some(), "{a}");
        }
        assert!(same_address("https://Example.ORG/x/", "https://example.org/x"));
        assert!(!same_address("https://example.org/X", "https://example.org/x"));
        assert_eq!(host_of("ssh://git@Example.org:22/x").as_deref(), Some("example.org"));
    }

    #[test]
    fn round_trip() {
        let lines = vec![
            Line::Object(ObjectLine {
                object: A.into(),
                label: Some("strongarm-comp@wrap-lee/v2".into()),
                refs: Some(vec![B.into()]),
                sources: vec![
                    Source::Git { git: "https://example.org/lib.git".into(), commit: None, path: Some("imp/x".into()) },
                    Source::Local("imp/x".into()),
                ],
                ..Default::default()
            }),
            Line::Include(IncludeLine {
                include: LIBRARY_LIST.into(),
                group: Some("official".into()),
                branch: Some("main".into()),
                sources: vec![Source::Git { git: "https://example.org/lib.git".into(), commit: None, path: None }],
            }),
        ];
        let text = write(&lines);
        assert!(text.starts_with("{\"toa\": \"source/1\"}\n{\"object\": \""));
        let r = read(text.as_bytes(), true, Kind::Library, MAX_VOLUME, "x");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.lines, lines);
    }

    #[test]
    fn lenient_drops_only_what_is_wrong() {
        let text = format!(
            "{{\"toa\": \"source/1\"}}\nnot json\n{{\"object\": \"x\"}}\n{{\"object\": \"{A}\", \"label\": 5, \
             \"sources\": [{{\"git\": \"http://bad/x\"}}, {{\"local\": \"a/b\"}}, {{\"later\": 1}}]}}\n"
        );
        let r = read(text.as_bytes(), false, Kind::Other, MAX_VOLUME, "x");
        assert!(r.problems.is_empty());
        assert_eq!(r.lines.len(), 1);
        let Line::Object(o) = &r.lines[0] else { panic!() };
        assert_eq!(o.label, None);
        assert_eq!(o.sources, vec![Source::Local("a/b".into())]);
        let strict = read(text.as_bytes(), true, Kind::Other, MAX_VOLUME, "x");
        assert_eq!(strict.problems.len(), 5);
    }

    #[test]
    fn header_and_kinds() {
        let r = read(b"{\"toa\": \"source/2\"}\n", false, Kind::Other, MAX_VOLUME, "x");
        assert!(r.ignored);
        let no_header = format!("{{\"object\": \"{A}\"}}\n");
        let r = read(no_header.as_bytes(), false, Kind::Other, MAX_VOLUME, "x");
        assert_eq!(r.lines.len(), 1);
        assert_eq!(read(no_header.as_bytes(), true, Kind::Other, MAX_VOLUME, "x").problems.len(), 1);
        let local =
            format!("{{\"toa\": \"source/1\"}}\n{{\"object\": \"{A}\", \"sources\": [{{\"local\": \"../x\"}}]}}\n");
        assert_eq!(read(local.as_bytes(), true, Kind::Sidecar, MAX_VOLUME, "x").problems.len(), 1);
        assert_eq!(read(local.as_bytes(), true, Kind::Library, MAX_VOLUME, "x").problems.len(), 1);
        let removed = format!("{{\"toa\": \"source/1\"}}\n{{\"object\": \"{A}\", \"removed\": true}}\n");
        assert_eq!(read(removed.as_bytes(), true, Kind::Sidecar, MAX_VOLUME, "x").problems.len(), 1);
        assert!(read(removed.as_bytes(), true, Kind::Library, MAX_VOLUME, "x").problems.is_empty());
        // latest counts only in a library list (5.2)
        let latest = removed.replace("removed", "latest");
        assert_eq!(read(latest.as_bytes(), true, Kind::Sidecar, MAX_VOLUME, "x").problems.len(), 1);
        let lib = read(latest.as_bytes(), true, Kind::Library, MAX_VOLUME, "x");
        assert!(lib.problems.is_empty());
        assert!(matches!(&lib.lines[0], Line::Object(o) if o.latest));
        assert!(write(&lib.lines).contains("\"latest\": true"));
        let other = read(latest.as_bytes(), false, Kind::Other, MAX_VOLUME, "x");
        assert!(matches!(&other.lines[0], Line::Object(o) if !o.latest), "elsewhere it means nothing");
    }
}
