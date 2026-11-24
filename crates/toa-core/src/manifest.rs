//! Reading a manifest, `toa.yaml` (3.2).
//!
//! A manifest is written in a subset of YAML that this module reads by itself, without a YAML
//! library, so that what a value means depends only on the standard:
//!
//! - UTF-8 without a byte order mark, LF line ends only, a final LF, no CR, NUL, U+0085, U+2028 or
//!   U+2029, at most 256 KiB (rule 1);
//! - one document whose top is a mapping, nested at most 16 deep; no directives, no `---`, no `...`
//!   (rule 2 and the tightening of the spec line's answer 2);
//! - no anchors, aliases, tags or complex keys; no duplicate keys; keys are strings (rule 3);
//! - plain scalars read by the fixed table of rule 4, and strings that must be quoted (rule 5);
//! - quoted and plain scalars, and flow collections, on a single line; block scalars `|`, `|-`,
//!   `>`, `>-` without an indentation indicator; no tab outside quoted strings and block scalar
//!   bodies (the tightening).
//!
//! Every scalar keeps the bytes it was read from, so sealing can replace a draft name by changing
//! only those characters (7.1).

use crate::problem::Problem;

/// Largest manifest (rule 1).
pub const MAX_BYTES: usize = 256 * 1024;
/// Deepest nesting; the top mapping is level 1 (rule 2).
pub const MAX_DEPTH: usize = 16;

/// A byte range in the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// How a scalar was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

/// A value read from a manifest.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Node>),
    Map(Vec<(Key, Node)>),
}

/// A value and where it was written. `span` and `style` are those of a scalar; a collection has
/// the span of its first line and the style `Plain`.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub value: Value,
    pub span: Span,
    pub style: Style,
}

/// A mapping key: always a string (rule 3).
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub text: String,
    pub span: Span,
}

impl Node {
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Node]> {
        match &self.value {
            Value::Seq(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&[(Key, Node)]> {
        match &self.value {
            Value::Map(v) => Some(v),
            _ => None,
        }
    }

    /// The value of a key of a mapping.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.as_map()?.iter().find(|(k, _)| k.text == key).map(|(_, v)| v)
    }

    /// Visit every node, depth first, with the path of keys and indexes that leads to it.
    pub fn walk<'a>(&'a self, path: &mut Vec<String>, f: &mut dyn FnMut(&[String], &'a Node)) {
        f(path, self);
        match &self.value {
            Value::Seq(items) => {
                for (i, n) in items.iter().enumerate() {
                    path.push(i.to_string());
                    n.walk(path, f);
                    path.pop();
                }
            }
            Value::Map(entries) => {
                for (k, n) in entries {
                    path.push(k.text.clone());
                    n.walk(path, f);
                    path.pop();
                }
            }
            _ => {}
        }
    }
}

/// Read a manifest. On failure the problems say what breaks which rule; reading stops at the first
/// error of structure.
pub fn parse(bytes: &[u8]) -> Result<Node, Vec<Problem>> {
    let text = check_bytes(bytes)?;
    let lines = split_lines(text);
    let mut p = Parser { text, lines, depth: 0 };
    p.document().map_err(|e| vec![e.into_problem()])
}

struct Error {
    line: usize,
    message: String,
}

impl Error {
    fn into_problem(self) -> Problem {
        Problem::error("E_MANIFEST_SYNTAX", "3.2", format!("toa.yaml:{}", self.line), self.message)
    }
}

type R<T> = Result<T, Error>;

fn problem(message: &str) -> Vec<Problem> {
    vec![Problem::error("E_MANIFEST_BYTES", "3.2", "toa.yaml", message)]
}

/// Rule 1: the bytes of the whole file.
fn check_bytes(bytes: &[u8]) -> Result<&str, Vec<Problem>> {
    if bytes.len() > MAX_BYTES {
        return Err(problem("a manifest has at most 256 KiB"));
    }
    if bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(problem("a manifest has no byte order mark"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| problem("a manifest is UTF-8"))?;
    if text.contains('\r') {
        return Err(problem("a manifest has LF line ends only, no CR"));
    }
    if let Some(c) = text.chars().find(|c| matches!(c, '\0' | '\u{85}' | '\u{2028}' | '\u{2029}')) {
        return Err(problem(&format!("a manifest holds no {:?}; write it escaped in a double-quoted string", c)));
    }
    if !text.ends_with('\n') {
        return Err(problem("a manifest ends with a line end"));
    }
    Ok(text)
}

#[derive(Debug, Clone, Copy)]
struct Line {
    /// Byte offset of the line in the text.
    offset: usize,
    /// Length without the line end.
    len: usize,
    /// Column where the content starts: the indentation, or more inside a compact sequence entry.
    start: usize,
}

fn split_lines(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut offset = 0;
    for l in text.split_inclusive('\n') {
        let len = l.len() - 1;
        let start = l.bytes().take_while(|&c| c == b' ').count();
        out.push(Line { offset, len, start });
        offset += l.len();
    }
    out
}

struct Parser<'a> {
    text: &'a str,
    lines: Vec<Line>,
    depth: usize,
}

const WORDS_TO_QUOTE: [&str; 9] = ["y", "n", "yes", "no", "on", "off", "true", "false", "null"];

/// What an unquoted scalar means by the table of rule 4; `Err` when it is a string that rule 5
/// says must be quoted.
pub fn plain_value(s: &str) -> Result<Value, String> {
    match s {
        "" | "~" | "null" => return Ok(Value::Null),
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        ".inf" => return Ok(Value::Float(f64::INFINITY)),
        "-.inf" => return Ok(Value::Float(f64::NEG_INFINITY)),
        ".nan" => return Ok(Value::Float(f64::NAN)),
        _ => {}
    }
    if is_int(s) {
        return s.parse::<i64>().map(Value::Int).map_err(|_| "integer out of range".into());
    }
    if s.len() <= 32 && is_float(s) {
        if let Ok(f) = s.parse::<f64>() {
            if f.is_finite() {
                return Ok(Value::Float(f));
            }
        }
    }
    let first = s.as_bytes()[0];
    if first.is_ascii_digit() || matches!(first, b'+' | b'-' | b'.') {
        return Err(format!("'{s}' starts with a digit, '+', '-' or '.': quote it"));
    }
    if WORDS_TO_QUOTE.contains(&s.to_ascii_lowercase().as_str()) || s == "=" || s == "<<" {
        return Err(format!("'{s}' must be quoted"));
    }
    Ok(Value::Str(s.to_string()))
}

fn is_int(s: &str) -> bool {
    let d = s.strip_prefix(['-', '+']).unwrap_or(s);
    d == "0" || (!d.is_empty() && d.len() <= 18 && d.as_bytes()[0] != b'0' && d.bytes().all(|c| c.is_ascii_digit()))
}

fn is_float(s: &str) -> bool {
    let d = s.strip_prefix(['-', '+']).unwrap_or(s);
    let (mantissa, exponent) = match d.find(['e', 'E']) {
        Some(i) => (&d[..i], Some(&d[i + 1..])),
        None => (d, None),
    };
    let Some((int, frac)) = mantissa.split_once('.') else {
        return false;
    };
    let int_ok =
        int == "0" || (!int.is_empty() && int.as_bytes()[0] != b'0' && int.bytes().all(|c| c.is_ascii_digit()));
    let frac_ok = !frac.is_empty() && frac.bytes().all(|c| c.is_ascii_digit());
    let exp_ok = match exponent {
        None => true,
        Some(e) => {
            let Some(digits) = e.strip_prefix(['-', '+']) else {
                return false;
            };
            !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
        }
    };
    int_ok && frac_ok && exp_ok
}

impl<'a> Parser<'a> {
    fn err<T>(&self, line: usize, message: impl Into<String>) -> R<T> {
        Err(Error { line: line + 1, message: message.into() })
    }

    /// The content of a line from its start column.
    fn content(&self, i: usize) -> &'a str {
        let l = self.lines[i];
        &self.text[l.offset + l.start..l.offset + l.len]
    }

    fn raw(&self, i: usize) -> &'a str {
        let l = self.lines[i];
        &self.text[l.offset..l.offset + l.len]
    }

    /// Whether a line is blank or holds only a comment.
    fn is_blank(&self, i: usize) -> bool {
        let c = self.raw(i).trim_start_matches(' ');
        c.is_empty() || c.starts_with('#')
    }

    /// The next line at or after `i` that is not blank.
    fn next_content(&self, mut i: usize) -> Option<usize> {
        while i < self.lines.len() {
            if !self.is_blank(i) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn enter(&mut self, line: usize) -> R<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.err(line, format!("nested deeper than {MAX_DEPTH} levels"));
        }
        Ok(())
    }

    fn document(&mut self) -> R<Node> {
        // lines outside block scalars: tabs, document markers and directives are checked as
        // they are reached; check the markers up front so that the message is clear
        for i in 0..self.lines.len() {
            let raw = self.raw(i);
            if raw == "---" || raw.starts_with("--- ") || raw == "..." || raw.starts_with("... ") {
                return self.err(i, "a manifest has no document markers '---' or '...'");
            }
            if raw.starts_with('%') {
                return self.err(i, "a manifest has no directives");
            }
        }
        let Some(first) = self.next_content(0) else {
            return self.err(0, "the top of a manifest is a mapping");
        };
        if self.lines[first].start != 0 {
            return self.err(first, "the top mapping starts in the first column");
        }
        if !self.is_key_line(first)? {
            return self.err(first, "the top of a manifest is a mapping");
        }
        let (node, next) = self.mapping(first, 0)?;
        if let Some(i) = self.next_content(next) {
            return self.err(i, "this line belongs to no mapping or sequence");
        }
        Ok(node)
    }

    fn check_no_tab(&self, i: usize, s: &str) -> R<()> {
        if s.contains('\t') {
            return self.err(i, "no tab outside quoted strings and block scalars");
        }
        Ok(())
    }

    /// Whether the content of line `i` is `key:` followed by a space or the end.
    fn is_key_line(&self, i: usize) -> R<bool> {
        let c = self.content(i);
        if c.starts_with("- ") || c == "-" {
            return Ok(false);
        }
        Ok(self.split_key(i)?.is_some())
    }

    /// Split a key line into the key and the offset (in the content) after the colon.
    fn split_key(&self, i: usize) -> R<Option<(Key, usize)>> {
        let c = self.content(i);
        let base = self.lines[i].offset + self.lines[i].start;
        if c.starts_with('?') && (c.len() == 1 || c.as_bytes()[1] == b' ') {
            return self.err(i, "no complex keys ('?')");
        }
        if c.starts_with('"') || c.starts_with('\'') {
            let (text, end) = self.quoted(i, c, 0)?;
            let rest = &c[end..];
            let after = rest.trim_start_matches(' ');
            if let Some(r) = after.strip_prefix(':') {
                if r.is_empty() || r.starts_with(' ') {
                    let colon = end + (rest.len() - after.len()) + 1;
                    return Ok(Some((Key { text, span: Span { start: base, end: base + end } }, colon)));
                }
            }
            return Ok(None);
        }
        // a plain key ends at the first ': ' or a final ':'
        let bytes = c.as_bytes();
        let mut j = 0;
        while j < bytes.len() {
            if bytes[j] == b'#' && j > 0 && bytes[j - 1] == b' ' {
                return Ok(None);
            }
            if bytes[j] == b':' && (j + 1 == bytes.len() || bytes[j + 1] == b' ') {
                let raw = c[..j].trim_end_matches(' ');
                if raw.is_empty() {
                    return self.err(i, "a key is not empty");
                }
                self.check_no_tab(i, raw)?;
                self.check_plain_start(i, raw)?;
                let text = match plain_value(raw) {
                    Ok(Value::Str(s)) => s,
                    Ok(_) => return self.err(i, format!("the key '{raw}' is not a string: quote it")),
                    Err(m) => return self.err(i, m),
                };
                return Ok(Some((Key { text, span: Span { start: base, end: base + raw.len() } }, j + 1)));
            }
            j += 1;
        }
        Ok(None)
    }

    fn check_plain_start(&self, i: usize, s: &str) -> R<()> {
        let b = s.as_bytes();
        let c = b[0];
        if matches!(c, b'&' | b'*' | b'!') {
            return self.err(i, "no anchors, aliases or tags");
        }
        if matches!(c, b',' | b'[' | b']' | b'{' | b'}' | b'#' | b'|' | b'>' | b'\'' | b'"' | b'%' | b'@' | b'`') {
            return self.err(i, format!("a plain scalar cannot start with {:?}", c as char));
        }
        if matches!(c, b'-' | b'?' | b':') && (b.len() == 1 || b[1] == b' ') {
            return self.err(i, format!("a plain scalar cannot start with {:?} and a space", c as char));
        }
        Ok(())
    }

    /// A block mapping whose keys are at column `indent`, starting at line `i`.
    /// Returns the node and the first line after it.
    fn mapping(&mut self, i: usize, indent: usize) -> R<(Node, usize)> {
        self.enter(i)?;
        let span = Span { start: self.lines[i].offset, end: self.lines[i].offset + self.lines[i].len };
        let mut entries: Vec<(Key, Node)> = Vec::new();
        let mut cur = i;
        loop {
            let Some(k) = self.next_content(cur) else {
                cur = self.lines.len();
                break;
            };
            let start = self.lines[k].start;
            if start < indent {
                cur = k;
                break;
            }
            if start > indent {
                return self.err(k, "this line is indented more than the keys of its mapping");
            }
            let Some((key, after)) = self.split_key(k)? else {
                if self.content(k).starts_with('-') {
                    // an indentless sequence entry ends the mapping; the caller decides
                    cur = k;
                    break;
                }
                return self.err(k, "expected 'key: value'");
            };
            if entries.iter().any(|(e, _)| e.text == key.text) {
                return self.err(k, format!("the key '{}' appears twice", key.text));
            }
            let (value, next) = self.value_after(k, after, indent, true)?;
            entries.push((key, value));
            cur = next;
        }
        self.depth -= 1;
        Ok((Node { value: Value::Map(entries), span, style: Style::Plain }, cur))
    }

    /// The value that follows a key or a sequence dash on line `k` at content offset `after`.
    /// `indent` is the column of the key or the dash; `in_map` allows an indentless sequence.
    fn value_after(&mut self, k: usize, after: usize, indent: usize, in_map: bool) -> R<(Node, usize)> {
        let content = self.content(k);
        let rest_full = &content[after..];
        let rest = rest_full.trim_start_matches(' ');
        let col = self.lines[k].start + after + (rest_full.len() - rest.len());
        let abs = self.lines[k].offset + col;
        if rest.is_empty() || rest.starts_with('#') {
            if !rest.is_empty() && !rest_full.starts_with(' ') {
                return self.err(k, "a comment is separated by a space");
            }
            // the value is on the following lines, or empty
            return match self.next_content(k + 1) {
                Some(n) if self.lines[n].start > indent => self.block_node(n, self.lines[n].start),
                Some(n)
                    if in_map
                        && self.lines[n].start == indent
                        && (self.content(n).starts_with("- ") || self.content(n) == "-") =>
                {
                    self.sequence(n, indent)
                }
                _ => Ok((Node { value: Value::Null, span: Span { start: abs, end: abs }, style: Style::Plain }, k + 1)),
            };
        }
        if rest.starts_with('|') || rest.starts_with('>') {
            return self.block_scalar(k, rest, abs, indent);
        }
        let node = self.inline(k, rest, abs)?;
        if let Some(n) = self.next_content(k + 1) {
            if self.lines[n].start > indent {
                return self.err(n, "a value continues on the next line: scalars are written on one line");
            }
        }
        Ok((node, k + 1))
    }

    /// A block collection or a lone scalar starting at line `n`, column `indent`.
    fn block_node(&mut self, n: usize, indent: usize) -> R<(Node, usize)> {
        let c = self.content(n);
        if c.starts_with("- ") || c == "-" {
            return self.sequence(n, indent);
        }
        if self.is_key_line(n)? {
            return self.mapping(n, indent);
        }
        let abs = self.lines[n].offset + self.lines[n].start;
        let node = self.inline(n, c, abs)?;
        if let Some(m) = self.next_content(n + 1) {
            if self.lines[m].start >= indent {
                return self.err(m, "a value continues on the next line: scalars are written on one line");
            }
        }
        Ok((node, n + 1))
    }

    /// A block sequence whose dashes are at column `indent`.
    fn sequence(&mut self, i: usize, indent: usize) -> R<(Node, usize)> {
        self.enter(i)?;
        let span = Span { start: self.lines[i].offset, end: self.lines[i].offset + self.lines[i].len };
        let mut items = Vec::new();
        let mut cur = i;
        loop {
            let Some(k) = self.next_content(cur) else {
                cur = self.lines.len();
                break;
            };
            let start = self.lines[k].start;
            if start < indent {
                cur = k;
                break;
            }
            if start > indent {
                return self.err(k, "this line is indented more than the entries of its sequence");
            }
            let c = self.content(k);
            if !(c.starts_with("- ") || c == "-") {
                cur = k;
                break;
            }
            let rest = &c[1..];
            let trimmed = rest.trim_start_matches(' ');
            let inner_col = start + 1 + (rest.len() - trimmed.len());
            let compact = trimmed.starts_with("- ") || trimmed == "-" || {
                // a compact mapping: '- key: value'
                let saved = self.lines[k].start;
                self.lines[k].start = inner_col;
                // a flow collection is never a key: '- {name: a, kind: b}' is one flow mapping
                let is_key = !trimmed.is_empty() && !trimmed.starts_with(['#', '[', '{']) && self.is_key_line(k)?;
                self.lines[k].start = saved;
                is_key
            };
            if compact {
                self.lines[k].start = inner_col;
                let (node, next) = self.block_node(k, inner_col)?;
                items.push(node);
                cur = next;
            } else {
                let (node, next) = self.value_after(k, 1, indent, false)?;
                items.push(node);
                cur = next;
            }
        }
        self.depth -= 1;
        Ok((Node { value: Value::Seq(items), span, style: Style::Plain }, cur))
    }

    /// A block scalar whose header `rest` is on line `k`; its parent is at column `indent`.
    fn block_scalar(&mut self, k: usize, rest: &str, abs: usize, indent: usize) -> R<(Node, usize)> {
        let header_end = rest.find(' ').unwrap_or(rest.len());
        let header = &rest[..header_end];
        let tail = rest[header_end..].trim_start_matches(' ');
        if !tail.is_empty() && !tail.starts_with('#') {
            return self.err(k, "nothing follows a block scalar header but a comment");
        }
        let (folded, strip) = match header {
            "|" => (false, false),
            "|-" => (false, true),
            ">" => (true, false),
            ">-" => (true, true),
            _ => return self.err(k, format!("the block scalar header '{header}' is not one of |, |-, >, >-")),
        };
        // the body: following lines that are empty or indented more than the parent
        let mut body: Vec<&str> = Vec::new();
        let mut content_indent: Option<usize> = None;
        let mut j = k + 1;
        while j < self.lines.len() {
            let raw = self.raw(j);
            let spaces = raw.bytes().take_while(|&c| c == b' ').count();
            if raw.trim_start_matches(' ').is_empty() {
                body.push("");
                j += 1;
                continue;
            }
            if spaces <= indent {
                break;
            }
            let ci = *content_indent.get_or_insert(spaces);
            if spaces < ci {
                return self.err(j, "a block scalar line is indented less than its first line");
            }
            body.push(&raw[ci..]);
            j += 1;
        }
        // trailing empty lines only matter to chomping, which clips or strips them
        while body.last() == Some(&"") {
            body.pop();
        }
        let mut text = if folded { fold(&body) } else { body.join("\n") };
        if !strip && !body.is_empty() {
            text.push('\n');
        }
        let style = if folded { Style::Folded } else { Style::Literal };
        let span = Span { start: abs, end: abs + header.len() };
        Ok((Node { value: Value::Str(text), span, style }, j))
    }

    /// A value written on one line: a quoted scalar, a flow collection or a plain scalar, then
    /// optionally a comment.
    fn inline(&mut self, i: usize, s: &str, abs: usize) -> R<Node> {
        let (node, used) = match s.as_bytes()[0] {
            b'"' | b'\'' => {
                let (text, end) = self.quoted(i, s, 0)?;
                let style = if s.starts_with('"') { Style::DoubleQuoted } else { Style::SingleQuoted };
                (Node { value: Value::Str(text), span: Span { start: abs, end: abs + end }, style }, end)
            }
            b'[' | b'{' => {
                let (node, end) = self.flow(i, s, 0, abs)?;
                (node, end)
            }
            _ => {
                let end = plain_end(s, false);
                let raw = s[..end].trim_end_matches(' ');
                self.check_no_tab(i, raw)?;
                self.check_plain_start(i, raw)?;
                if raw.contains(": ") {
                    return self.err(i, "a value holds ': ': quote it, or put the mapping on its own lines");
                }
                if raw.ends_with(':') {
                    return self.err(i, "a value ends with ':': quote it");
                }
                let value = plain_value(raw).or_else(|m| self.err(i, m))?;
                // what follows starts after the value, so that `value   # comment` keeps its space
                (Node { value, span: Span { start: abs, end: abs + raw.len() }, style: Style::Plain }, raw.len())
            }
        };
        self.after_value(i, &s[used..])?;
        Ok(node)
    }

    /// What may follow a value on its line: spaces, then nothing or a comment.
    fn after_value(&self, i: usize, rest: &str) -> R<()> {
        let t = rest.trim_start_matches(' ');
        if t.is_empty() {
            return Ok(());
        }
        if t.starts_with('#') && t.len() < rest.len() {
            return Ok(());
        }
        if t.starts_with('\t') {
            return self.err(i, "no tab outside quoted strings and block scalars");
        }
        self.err(i, format!("unexpected '{}' after the value", t.chars().next().unwrap_or(' ')))
    }

    /// A quoted scalar starting at byte `at` of `s`: the string and the offset after the quote.
    fn quoted(&self, i: usize, s: &str, at: usize) -> R<(String, usize)> {
        let q = s.as_bytes()[at];
        let body = &s[at + 1..];
        let mut out = String::new();
        let mut chars = body.char_indices();
        while let Some((j, c)) = chars.next() {
            if q == b'\'' {
                if c == '\'' {
                    if body[j + 1..].starts_with('\'') {
                        out.push('\'');
                        chars.next();
                        continue;
                    }
                    return Ok((out, at + 1 + j + 1));
                }
                out.push(c);
            } else {
                match c {
                    '"' => return Ok((out, at + 1 + j + 1)),
                    '\\' => {
                        let Some((_, e)) = chars.next() else {
                            return self.err(i, "a quoted string ends on its own line");
                        };
                        let simple = match e {
                            '0' => Some('\0'),
                            'a' => Some('\x07'),
                            'b' => Some('\x08'),
                            't' | '\t' => Some('\t'),
                            'n' => Some('\n'),
                            'v' => Some('\x0b'),
                            'f' => Some('\x0c'),
                            'r' => Some('\r'),
                            'e' => Some('\x1b'),
                            ' ' => Some(' '),
                            '"' => Some('"'),
                            '/' => Some('/'),
                            '\\' => Some('\\'),
                            'N' => Some('\u{85}'),
                            '_' => Some('\u{a0}'),
                            'L' => Some('\u{2028}'),
                            'P' => Some('\u{2029}'),
                            _ => None,
                        };
                        if let Some(ch) = simple {
                            out.push(ch);
                            continue;
                        }
                        let n = match e {
                            'x' => 2,
                            'u' => 4,
                            'U' => 8,
                            _ => return self.err(i, format!("unknown escape '\\{e}'")),
                        };
                        let mut code = String::new();
                        for _ in 0..n {
                            match chars.next() {
                                Some((_, h)) if h.is_ascii_hexdigit() => code.push(h),
                                _ => return self.err(i, "a broken escape"),
                            }
                        }
                        match u32::from_str_radix(&code, 16).ok().and_then(char::from_u32) {
                            Some(ch) => out.push(ch),
                            None => return self.err(i, "an escape that is no character"),
                        }
                    }
                    _ => out.push(c),
                }
            }
        }
        self.err(i, "a quoted string ends on its own line")
    }

    /// A flow collection starting at byte `at` of `s` (one line); `abs` is the offset of `s`.
    fn flow(&mut self, i: usize, s: &str, at: usize, abs: usize) -> R<(Node, usize)> {
        self.enter(i)?;
        let open = s.as_bytes()[at];
        let close = if open == b'[' { b']' } else { b'}' };
        let mut j = at + 1;
        let mut items = Vec::new();
        let mut entries: Vec<(Key, Node)> = Vec::new();
        loop {
            j = skip_spaces(s, j);
            if j >= s.len() {
                return self.err(i, "a flow collection is closed on its own line");
            }
            if s.as_bytes()[j] == close {
                j += 1;
                break;
            }
            // one entry: for a mapping 'key: value', for a sequence a node
            let (first, after) = self.flow_node(i, s, j, abs)?;
            j = skip_spaces(s, after);
            if open == b'{' {
                if s.as_bytes().get(j) != Some(&b':') {
                    return self.err(i, "an entry of a flow mapping is 'key: value'");
                }
                let key = match (&first.value, first.style) {
                    (Value::Str(t), _) => Key { text: t.clone(), span: first.span },
                    (Value::Seq(_) | Value::Map(_), _) => return self.err(i, "a key is a string"),
                    _ => return self.err(i, "a key is a string: quote it"),
                };
                if entries.iter().any(|(e, _)| e.text == key.text) {
                    return self.err(i, format!("the key '{}' appears twice", key.text));
                }
                j += 1;
                let next = skip_spaces(s, j);
                let value = if matches!(s.as_bytes().get(next), Some(b',') | Some(b'}')) {
                    Node { value: Value::Null, span: Span { start: abs + next, end: abs + next }, style: Style::Plain }
                } else {
                    if next == j {
                        return self.err(i, "a space follows the ':' of a flow mapping");
                    }
                    let (v, after) = self.flow_node(i, s, next, abs)?;
                    j = after;
                    v
                };
                entries.push((key, value));
            } else {
                if s.as_bytes().get(j) == Some(&b':') {
                    return self.err(i, "no mappings inside a flow sequence without braces");
                }
                items.push(first);
            }
            j = skip_spaces(s, j);
            match s.as_bytes().get(j) {
                Some(b',') => {
                    j += 1;
                    let k = skip_spaces(s, j);
                    if s.as_bytes().get(k) == Some(&b',') {
                        return self.err(i, "an empty entry in a flow collection");
                    }
                }
                Some(&c) if c == close => {
                    j += 1;
                    break;
                }
                _ => return self.err(i, "expected ',' or the end of the flow collection"),
            }
        }
        self.depth -= 1;
        let span = Span { start: abs + at, end: abs + j };
        let value = if open == b'[' { Value::Seq(items) } else { Value::Map(entries) };
        Ok((Node { value, span, style: Style::Plain }, j))
    }

    /// One node inside a flow collection.
    fn flow_node(&mut self, i: usize, s: &str, j: usize, abs: usize) -> R<(Node, usize)> {
        match s.as_bytes()[j] {
            b'"' | b'\'' => {
                let (text, end) = self.quoted(i, s, j)?;
                let style = if s.as_bytes()[j] == b'"' { Style::DoubleQuoted } else { Style::SingleQuoted };
                Ok((Node { value: Value::Str(text), span: Span { start: abs + j, end: abs + end }, style }, end))
            }
            b'[' | b'{' => self.flow(i, s, j, abs),
            _ => {
                let end = j + plain_end(&s[j..], true);
                let raw = s[j..end].trim_end_matches(' ');
                if raw.is_empty() {
                    return self.err(i, "an empty entry in a flow collection");
                }
                self.check_no_tab(i, raw)?;
                self.check_plain_start(i, raw)?;
                let value = plain_value(raw).or_else(|m| self.err(i, m))?;
                Ok((
                    Node { value, span: Span { start: abs + j, end: abs + j + raw.len() }, style: Style::Plain },
                    j + raw.len(),
                ))
            }
        }
    }
}

fn skip_spaces(s: &str, mut j: usize) -> usize {
    while s.as_bytes().get(j) == Some(&b' ') {
        j += 1;
    }
    j
}

/// Where a plain scalar ends: at a comment (' #'), and in flow context also at ',', '[', ']',
/// '{', '}', '#' or ': '.
fn plain_end(s: &str, flow: bool) -> usize {
    let b = s.as_bytes();
    let mut j = 0;
    while j < b.len() {
        let c = b[j];
        if c == b'#' && j > 0 && b[j - 1] == b' ' {
            return j;
        }
        if flow {
            if matches!(c, b',' | b'[' | b']' | b'{' | b'}' | b'#') {
                return j;
            }
            if c == b':' && (j + 1 == b.len() || matches!(b[j + 1], b' ' | b',' | b']' | b'}')) {
                return j;
            }
        }
        j += 1;
    }
    b.len()
}

/// Line folding of a `>` block scalar (YAML 1.2, 8.1.3), without the final line end.
fn fold(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut prev_normal: Option<bool> = None;
    let mut empties = 0;
    for l in lines {
        if l.is_empty() {
            empties += 1;
            continue;
        }
        let normal = !(l.starts_with(' ') || l.starts_with('\t'));
        match prev_normal {
            None => out.push_str(&"\n".repeat(empties)),
            Some(p) => {
                if empties > 0 {
                    let n = if p && normal { empties } else { empties + 1 };
                    out.push_str(&"\n".repeat(n));
                } else if p && normal {
                    out.push(' ');
                } else {
                    out.push('\n');
                }
            }
        }
        empties = 0;
        out.push_str(l);
        prev_normal = Some(normal);
    }
    out
}

/// A manifest as JSON, by the table of 3.2.3 rule 9: integers and finite floats as numbers,
/// `.inf`, `-.inf` and `.nan` as `{"float": …}`, mappings in the order written. The style a value
/// was written in is not kept.
pub fn to_json(node: &Node) -> serde_json::Value {
    use serde_json::Value as J;
    match &node.value {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Int(i) => J::from(*i),
        Value::Float(f) => match serde_json::Number::from_f64(*f) {
            Some(n) => J::Number(n),
            None => {
                let w = if f.is_nan() {
                    ".nan"
                } else if *f > 0.0 {
                    ".inf"
                } else {
                    "-.inf"
                };
                serde_json::json!({ "float": w })
            }
        },
        Value::Str(s) => J::String(s.clone()),
        Value::Seq(v) => J::Array(v.iter().map(to_json).collect()),
        Value::Map(m) => J::Object(m.iter().map(|(k, v)| (k.text.clone(), to_json(v))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> Node {
        match parse(s.as_bytes()) {
            Ok(n) => n,
            Err(p) => panic!("{s:?}: {}", p[0]),
        }
    }

    fn bad(s: &str) -> String {
        match parse(s.as_bytes()) {
            Ok(n) => panic!("{s:?} read as {n:?}"),
            Err(p) => p[0].to_string(),
        }
    }

    #[test]
    fn as_json() {
        let n = ok("a: 1
b: 2.5
c: .inf
d: [x, 'y']
e: {f: null}
");
        assert_eq!(to_json(&n).to_string(), r#"{"a":1,"b":2.5,"c":{"float":".inf"},"d":["x","y"],"e":{"f":null}}"#);
    }

    #[test]
    fn flow_mappings_as_entries_of_a_sequence() {
        let n = ok("pins:
- {name: inp, kind: analog}
- [a, b]
- {}
");
        let pins = n.get("pins").unwrap().as_seq().unwrap();
        assert_eq!(pins.len(), 3);
        assert_eq!(pins[0].get("kind").unwrap().as_str(), Some("analog"));
    }

    #[test]
    fn a_comment_after_a_plain_value() {
        let n = ok("type: ver   # a comment
k: a#b
");
        assert_eq!(n.get("type").and_then(Node::as_str), Some("ver"));
        assert_eq!(n.get("k").and_then(Node::as_str), Some("a#b"));
        assert!(bad("k: [a] b
")
        .contains("after the value"));
    }

    #[test]
    fn the_table_of_rule_4() {
        assert_eq!(plain_value("").unwrap(), Value::Null);
        assert_eq!(plain_value("~").unwrap(), Value::Null);
        assert_eq!(plain_value("true").unwrap(), Value::Bool(true));
        assert_eq!(plain_value("-12").unwrap(), Value::Int(-12));
        assert_eq!(plain_value("1.5e-9").unwrap(), Value::Float(1.5e-9));
        assert_eq!(plain_value("abc").unwrap(), Value::Str("abc".into()));
        for s in ["1e-9", "23.1.0", "2026-10-30", "007", "0x1F", "../lib", "on", "True", "NULL", "=", "<<", ".5"] {
            assert!(plain_value(s).is_err(), "{s}");
        }
        assert!(plain_value("1234567890123456789").is_err());
        assert_eq!(plain_value("123456789012345678").unwrap(), Value::Int(123456789012345678));
    }

    #[test]
    fn a_manifest() {
        let n = ok("type: imp\nlicense: MIT\n# a comment\nip: ip_ea44863a8d2f242bc651caf709d45a27e7531d9d\n\
                    uses:\n  dp:\n    ip: x\n    impl: z\nsupply: [1.62, 1.8, 1.98]\nmetric: {abs: 0.05, unit: LSB}\n\
                    list:\n- a\n- 'b # not a comment'\n- k: 1\n  j: 2\n- - deep\nempty:\ntext: |\n  one\n  two\n\nnext: \"x\\ty\"\n");
        assert_eq!(n.get("type").unwrap().as_str(), Some("imp"));
        assert_eq!(n.get("uses").unwrap().get("dp").unwrap().get("impl").unwrap().as_str(), Some("z"));
        assert_eq!(n.get("supply").unwrap().as_seq().unwrap().len(), 3);
        assert_eq!(n.get("metric").unwrap().get("unit").unwrap().as_str(), Some("LSB"));
        let list = n.get("list").unwrap().as_seq().unwrap();
        assert_eq!(list[1].as_str(), Some("b # not a comment"));
        assert_eq!(list[2].get("j").unwrap().value, Value::Int(2));
        assert_eq!(list[3].as_seq().unwrap()[0].as_str(), Some("deep"));
        assert_eq!(n.get("empty").unwrap().value, Value::Null);
        assert_eq!(n.get("text").unwrap().as_str(), Some("one\ntwo\n"));
        assert_eq!(n.get("next").unwrap().as_str(), Some("x\ty"));
    }

    #[test]
    fn spans_point_at_the_written_characters() {
        let s = "type: imp\nip: 'imp_draft-a'\n";
        let n = ok(s);
        let ip = n.get("ip").unwrap();
        assert_eq!(&s[ip.span.start..ip.span.end], "'imp_draft-a'");
        assert_eq!(ip.style, Style::SingleQuoted);
    }

    #[test]
    fn folded() {
        let n = ok("a: >-\n  one\n  two\n\n  three\n    more\nb: 1\n");
        assert_eq!(n.get("a").unwrap().as_str(), Some("one two\nthree\n  more"));
        assert_eq!(n.get("b").unwrap().value, Value::Int(1));
    }

    #[test]
    fn rejected() {
        for s in [
            "a: 1",
            "\u{feff}a: 1\n",
            "a: 1\r\n",
            "---\na: 1\n",
            "a: 1\n...\n",
            "%YAML 1.2\na: 1\n",
            "- a\n",
            "a: &x 1\n",
            "a: *x\n",
            "a: !!str 1\n",
            "a: 1\na: 2\n",
            "1: a\n",
            "on: a\n",
            "a: on\n",
            "a: 23.1.0\n",
            "a: long\n  continued\n",
            "a: 'open\n  close'\n",
            "a: [1,\n  2]\n",
            "a:\tb\n",
            "a: b # ok\n\tc: 1\n",
            "? a\n: b\n",
            "a: |2\n   x\n",
            "a: b: c\n",
            "a: [x, , y]\n",
            "a: {b}\n",
        ] {
            bad(s);
        }
        assert!(bad("a: on\n").contains("toa.yaml:1"));
    }

    #[test]
    fn depth() {
        let mut s = String::new();
        for d in 0..16 {
            s.push_str(&" ".repeat(d * 2));
            s.push_str("k:\n");
        }
        s.push_str(&" ".repeat(32));
        s.push_str("v: 1\n");
        bad(&s);
        let mut s = String::new();
        for d in 0..15 {
            s.push_str(&" ".repeat(d * 2));
            s.push_str("k:\n");
        }
        s.push_str(&" ".repeat(30));
        s.push_str("v: 1\n");
        ok(&s);
    }
}
