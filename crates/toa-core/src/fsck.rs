//! Checks of objects received from elsewhere, equal to what git does with `fetch.fsckObjects`
//! (`git fsck --strict`, where warnings count as errors): trees, commits and tags as stored,
//! byte by byte (7.3, step 4). The built-in git of `toa` runs them on everything it receives;
//! the system git runs its own.
//!
//! What git checks and this does too: the form of tree entries (modes, zero-padded modes, names
//! with `/`, empty, `.`, `..`, `.git` in the spellings of HFS+ and NTFS, null ids, duplicates,
//! order), the headers of commits and tags and the identities in them. One check is stricter:
//! git reads the content of `.gitmodules` files; a tree holding one, in any spelling, is refused
//! here, as no TOA library has one (6.4).

/// One entry of a tree as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry<'a> {
    /// The mode as written, octal digits.
    pub mode_raw: &'a [u8],
    /// The mode as a number.
    pub mode: u32,
    pub name: &'a [u8],
    pub id: &'a [u8],
}

impl TreeEntry<'_> {
    pub fn is_dir(&self) -> bool {
        self.mode & 0o170000 == 0o040000
    }
    pub fn is_gitlink(&self) -> bool {
        self.mode & 0o170000 == 0o160000
    }
    pub fn is_symlink(&self) -> bool {
        self.mode & 0o170000 == 0o120000
    }
    /// The mode as `git ls-tree` prints it: git's canonical form of the mode (`canon_mode`).
    pub fn canonical_mode(&self) -> u32 {
        match self.mode & 0o170000 {
            0o100000 => {
                if self.mode & 0o100 != 0 {
                    0o100755
                } else {
                    0o100644
                }
            }
            0o040000 => 0o040000,
            0o120000 => 0o120000,
            _ => 0o160000,
        }
    }
    /// `blob`, `tree` or `commit`, as git reads the mode.
    pub fn kind(&self) -> &'static str {
        match self.canonical_mode() {
            0o040000 => "tree",
            0o160000 => "commit",
            _ => "blob",
        }
    }
}

/// Read the entries of a tree; `None` when it is not a tree at all (git: `badTree`).
pub fn parse_tree(raw: &[u8], id_len: usize) -> Option<Vec<TreeEntry<'_>>> {
    let mut out = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        let sp = rest.iter().position(|&b| b == b' ')?;
        let mode_raw = &rest[..sp];
        if mode_raw.is_empty() || !mode_raw.iter().all(|b| (b'0'..=b'7').contains(b)) || mode_raw.len() > 7 {
            return None;
        }
        let mode = mode_raw.iter().fold(0u32, |m, b| m * 8 + u32::from(b - b'0'));
        let after = &rest[sp + 1..];
        let nul = after.iter().position(|&b| b == 0)?;
        let name = &after[..nul];
        let tail = &after[nul + 1..];
        if tail.len() < id_len {
            return None;
        }
        out.push(TreeEntry { mode_raw, mode, name, id: &tail[..id_len] });
        rest = &tail[id_len..];
    }
    Some(out)
}

/// The HFS+ characters that are ignored when names are compared (git: `is_hfs_dotgit`).
fn hfs_ignorable(c: char) -> bool {
    matches!(c as u32, 0x200c..=0x200f | 0x202a..=0x202e | 0x206a..=0x206f | 0xfeff)
}

/// Whether HFS+ would take `name` for `.<word>` (`word` in lower case).
fn is_hfs_dot(name: &[u8], word: &str) -> bool {
    let Ok(s) = std::str::from_utf8(name) else { return false };
    let folded: String = s.chars().filter(|c| !hfs_ignorable(*c)).flat_map(char::to_lowercase).collect();
    folded == format!(".{word}")
}

/// Whether NTFS would take `name` for `.git` (git: `is_ntfs_dotgit`): `.git` or its short name
/// `git~1`, in any case, followed by nothing but dots and spaces, or by `:` (a stream).
pub fn is_ntfs_dotgit(name: &[u8]) -> bool {
    let lower: Vec<u8> = name.iter().map(u8::to_ascii_lowercase).collect();
    let rest = if let Some(r) = lower.strip_prefix(b".git") {
        r
    } else if let Some(r) = lower.strip_prefix(b"git~1") {
        r
    } else {
        return false;
    };
    for &c in rest {
        if c == b'\\' || c == b'/' || c == b':' {
            return true;
        }
        if c != b'.' && c != b' ' {
            return false;
        }
    }
    true
}

/// Whether a file system would take `name` for `.gitmodules`: on HFS+, or on NTFS with its short
/// names (`gitmod~1`, or `gi7eba~n` for hashed short names).
fn is_dotgitmodules(name: &[u8]) -> bool {
    if is_hfs_dot(name, "gitmodules") {
        return true;
    }
    let lower: Vec<u8> = name.iter().map(u8::to_ascii_lowercase).collect();
    let trimmed: Vec<u8> = {
        let mut v = lower.clone();
        while matches!(v.last(), Some(b'.' | b' ')) {
            v.pop();
        }
        v
    };
    trimmed == b".gitmodules"
        || trimmed.starts_with(b"gitmod~")
        || (trimmed.starts_with(b"gi7eba~") && trimmed.len() == 8)
}

/// The problems of a tree as git's strict fsck reports them (message ids as git names them).
pub fn check_tree(raw: &[u8], id_len: usize) -> Vec<String> {
    let Some(entries) = parse_tree(raw, id_len) else { return vec!["badTree: the tree cannot be read".into()] };
    let mut out = Vec::new();
    let mut seen: std::collections::BTreeSet<&[u8]> = std::collections::BTreeSet::new();
    let mut prev: Option<(Vec<u8>, &[u8])> = None;
    for e in &entries {
        let shown = String::from_utf8_lossy(e.name);
        if e.id.iter().all(|&b| b == 0) {
            out.push(format!("nullSha1: {shown:?} has the null id"));
        }
        if e.name.contains(&b'/') {
            out.push(format!("fullPathname: {shown:?} holds '/'"));
        }
        if e.name.is_empty() {
            out.push("emptyName: an entry has an empty name".into());
        }
        if e.name == b"." {
            out.push("hasDot: an entry is named '.'".into());
        }
        if e.name == b".." {
            out.push("hasDotdot: an entry is named '..'".into());
        }
        if is_hfs_dot(e.name, "git") || is_ntfs_dotgit(e.name) {
            out.push(format!("hasDotgit: {shown:?} is taken for .git on some file system"));
        }
        if is_dotgitmodules(e.name) {
            out.push(format!("gitmodules: {shown:?} is a .gitmodules file; toa accepts none"));
        }
        if e.mode_raw.first() == Some(&b'0') {
            out.push(format!("zeroPaddedFilemode: {shown:?} has the mode {}", String::from_utf8_lossy(e.mode_raw)));
        }
        if !matches!(e.mode, 0o100644 | 0o100755 | 0o120000 | 0o040000 | 0o160000) {
            out.push(format!("badFilemode: {shown:?} has the mode {:o}", e.mode));
        }
        if !seen.insert(e.name) {
            out.push(format!("duplicateEntries: {shown:?} is there twice"));
        }
        // the order git writes: names compared bytewise, a directory as if followed by '/'
        let mut key = e.name.to_vec();
        if e.is_dir() {
            key.push(b'/');
        }
        if let Some((p, pname)) = &prev {
            if *p > key && *pname != e.name {
                out.push(format!("treeNotSorted: {shown:?} comes after {:?}", String::from_utf8_lossy(pname)));
            }
        }
        prev = Some((key, e.name));
    }
    out.sort();
    out.dedup();
    out
}

/// What a commit names: its tree and parents (hexadecimal), and its committer line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Commit {
    pub tree: String,
    pub parents: Vec<String>,
    pub committer: String,
}

fn is_hex(s: &[u8], len: usize) -> bool {
    s.len() == len && s.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The header lines of a commit or tag: up to the first empty line. Problems of the header as a
/// whole (git: `verify_headers`).
fn header(raw: &[u8]) -> Result<Vec<&[u8]>, String> {
    let end = raw.windows(2).position(|w| w == b"\n\n").map(|p| p + 1);
    let head = match end {
        Some(e) => &raw[..e],
        None if raw.ends_with(b"\n") => raw,
        None => return Err("unterminatedHeader: the header does not end".into()),
    };
    if head.contains(&0) {
        return Err("nulInHeader: a NUL in the header".into());
    }
    Ok(head.split(|&b| b == b'\n').filter(|l| !l.is_empty()).collect())
}

/// The problems of an identity, `Name <email> <seconds> <+hhmm>` (git: `fsck_ident`).
pub fn check_ident(id: &[u8]) -> Option<String> {
    if id.first() == Some(&b'<') {
        return Some("missingNameBeforeEmail".into());
    }
    let lt = id.iter().position(|&b| b == b'<' || b == b'>');
    let Some(lt) = lt else { return Some("missingEmail".into()) };
    if id[lt] == b'>' {
        return Some("badName".into());
    }
    if lt == 0 || id[lt - 1] != b' ' {
        return Some("missingSpaceBeforeEmail".into());
    }
    let rest = &id[lt + 1..];
    let gt = rest.iter().position(|&b| b == b'<' || b == b'>');
    let Some(gt) = gt.filter(|&g| rest[g] == b'>') else { return Some("badEmail".into()) };
    let rest = &rest[gt + 1..];
    let Some(rest) = rest.strip_prefix(b" ") else { return Some("missingSpaceBeforeDate".into()) };
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if rest.first() == Some(&b'0') && rest.get(1) != Some(&b' ') {
        return Some("zeroPaddedDate".into());
    }
    if digits == 0 || rest.get(digits) != Some(&b' ') {
        return Some("badDate".into());
    }
    let seconds: Option<u64> = std::str::from_utf8(&rest[..digits]).ok().and_then(|s| s.parse().ok());
    // git: `date_overflows`, beyond what its time type holds
    if seconds.is_none_or(|s| s > i64::MAX as u64) {
        return Some("badDateOverflow".into());
    }
    let tz = &rest[digits + 1..];
    if tz.len() != 5 || !matches!(tz[0], b'+' | b'-') || !tz[1..].iter().all(u8::is_ascii_digit) {
        return Some("badTimezone".into());
    }
    None
}

/// Read and check a commit (git: `fsck_commit`); ids have `hex_len` hexadecimal digits.
pub fn check_commit(raw: &[u8], hex_len: usize) -> Result<Commit, Vec<String>> {
    let lines = header(raw).map_err(|e| vec![e])?;
    let mut it = lines.into_iter().peekable();
    let mut c = Commit::default();
    let mut problems = Vec::new();
    match it.next().and_then(|l| l.strip_prefix(b"tree ")) {
        Some(t) if is_hex(t, hex_len) => c.tree = String::from_utf8_lossy(t).into_owned(),
        Some(_) => problems.push("badTreeSha1: the tree id is not one".into()),
        None => problems.push("missingTree: no tree line first".into()),
    }
    while let Some(p) = it.peek().and_then(|l| l.strip_prefix(b"parent ")) {
        if is_hex(p, hex_len) {
            c.parents.push(String::from_utf8_lossy(p).into_owned());
        } else {
            problems.push("badParentSha1: a parent id is not one".into());
        }
        it.next();
    }
    let mut authors = 0;
    while let Some(a) = it.peek().and_then(|l| l.strip_prefix(b"author ")) {
        authors += 1;
        if let Some(m) = check_ident(a) {
            problems.push(format!("{m}: in the author"));
        }
        it.next();
    }
    match authors {
        0 => problems.push("missingAuthor: no author line".into()),
        1 => {}
        _ => problems.push("multipleAuthors: more than one author line".into()),
    }
    match it.next().and_then(|l| l.strip_prefix(b"committer ")) {
        Some(m) => {
            if let Some(p) = check_ident(m) {
                problems.push(format!("{p}: in the committer"));
            }
            c.committer = String::from_utf8_lossy(m).into_owned();
        }
        None => problems.push("missingCommitter: no committer line after the author".into()),
    }
    if problems.is_empty() {
        Ok(c)
    } else {
        Err(problems)
    }
}

/// Read and check an annotated tag (git: `fsck_tag`): the object it points to and its type.
pub fn check_tag(raw: &[u8], hex_len: usize) -> Result<(String, String), Vec<String>> {
    let lines = header(raw).map_err(|e| vec![e])?;
    let mut it = lines.into_iter();
    let mut problems = Vec::new();
    let object = match it.next().and_then(|l| l.strip_prefix(b"object ")) {
        Some(o) if is_hex(o, hex_len) => String::from_utf8_lossy(o).into_owned(),
        _ => {
            problems.push("missingObject: no object line".into());
            String::new()
        }
    };
    let kind = match it.next().and_then(|l| l.strip_prefix(b"type ")) {
        Some(t) if matches!(t, b"commit" | b"tree" | b"blob" | b"tag") => String::from_utf8_lossy(t).into_owned(),
        Some(_) => {
            problems.push("badType: the type is not one".into());
            String::new()
        }
        None => {
            problems.push("missingTypeEntry: no type line".into());
            String::new()
        }
    };
    match it.next().and_then(|l| l.strip_prefix(b"tag ")) {
        Some(n) if !n.is_empty() => {}
        _ => problems.push("missingTagEntry: no tag line".into()),
    }
    if let Some(t) = it.next().and_then(|l| l.strip_prefix(b"tagger ")) {
        if let Some(m) = check_ident(t) {
            problems.push(format!("{m}: in the tagger"));
        }
    }
    if problems.is_empty() {
        Ok((object, kind))
    } else {
        Err(problems)
    }
}

/// The committer's time of a commit line `Name <email> <seconds> <+hhmm>` as git's `%cI` prints
/// it: ISO 8601 in the committer's offset, `Z` for an offset of zero.
pub fn iso_date(ident: &str) -> Option<String> {
    let gt = ident.rfind('>')?;
    let mut parts = ident[gt + 1..].split_ascii_whitespace();
    let secs: i64 = parts.next()?.parse().ok()?;
    let tz = parts.next()?;
    let (sign, hh, mm) =
        (tz.as_bytes().first()?, tz.get(1..3)?.parse::<i64>().ok()?, tz.get(3..5)?.parse::<i64>().ok()?);
    let off = (hh * 60 + mm) * 60 * if *sign == b'-' { -1 } else { 1 };
    let t = secs + off;
    let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let zone = if off == 0 { "Z".to_string() } else { format!("{}{:02}:{:02}", *sign as char, hh, mm) };
    Some(format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{zone}", rem / 3600, rem % 3600 / 60, rem % 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = Vec::new();
        for (mode, name) in entries {
            b.extend_from_slice(mode.as_bytes());
            b.push(b' ');
            b.extend_from_slice(name);
            b.push(0);
            b.extend_from_slice(&[7u8; 20]);
        }
        b
    }

    fn codes(raw: &[u8]) -> Vec<String> {
        check_tree(raw, 20).into_iter().map(|m| m.split(':').next().unwrap().to_string()).collect()
    }

    #[test]
    fn trees() {
        assert!(
            codes(&tree(&[("100644", b"a"), ("40000", b"b"), ("100644", b"b.c")])).contains(&"treeNotSorted".into())
        );
        assert!(codes(&tree(&[("100644", b"a"), ("100644", b"b.c"), ("40000", b"b")])).is_empty());
        assert_eq!(codes(&tree(&[("0100644", b"a")])), ["zeroPaddedFilemode"]);
        assert_eq!(codes(&tree(&[("100664", b"a")])), ["badFilemode"]);
        assert_eq!(codes(&tree(&[("100644", b"a/b")])), ["fullPathname"]);
        assert_eq!(codes(&tree(&[("40000", b"..")])), ["hasDotdot"]);
        for n in [&b".git"[..], b".GIT", b"GIT~1", b".git. ", b".git::$INDEX_ALLOCATION", ".g\u{200c}it".as_bytes()] {
            assert_eq!(codes(&tree(&[("40000", n)])), ["hasDotgit"], "{n:?}");
        }
        assert!(codes(&tree(&[("40000", b".github")])).is_empty());
        assert_eq!(codes(&tree(&[("100644", b"a"), ("100644", b"a")])), ["duplicateEntries"]);
        assert_eq!(codes(&tree(&[("100644", b"")])), ["emptyName"]);
        assert_eq!(codes(&tree(&[("100644", b".gitmodules")])), ["gitmodules"]);
        assert_eq!(codes(b"100644 a"), ["badTree"]);
        assert!(codes(&tree(&[("100755", b"x"), ("120000", b"y")])).is_empty());
    }

    #[test]
    fn commits_and_identities() {
        let t = "0123456789012345678901234567890123456789";
        let good = format!(
            "tree {t}\nparent {t}\nauthor A <a@b> 1700000000 +0800\ncommitter A <a@b> 1700000000 +0800\n\nmsg\n"
        );
        let c = check_commit(good.as_bytes(), 40).unwrap();
        assert_eq!((c.tree.as_str(), c.parents.len()), (t, 1));
        assert!(check_commit(format!("tree {t}\ncommitter A <a@b> 1 +0000\n\n").as_bytes(), 40).is_err());
        assert_eq!(check_ident(b"A <a@b> 01 +0000").as_deref(), Some("zeroPaddedDate"));
        assert_eq!(check_ident(b"A<a@b> 1 +0000").as_deref(), Some("missingSpaceBeforeEmail"));
        assert_eq!(check_ident(b"A <a@b> 1 +00").as_deref(), Some("badTimezone"));
        assert_eq!(check_ident(b"A <a@b> 0 +0000"), None);
        assert!(
            check_tag(format!("object {t}\ntype commit\ntag v1\ntagger A <a@b> 1 +0000\n\nm").as_bytes(), 40).is_ok()
        );
    }

    #[test]
    fn dates_as_git_prints_them() {
        assert_eq!(iso_date("T <t@e> 1795435200 +0000").as_deref(), Some("2026-11-23T12:00:00Z"));
        assert_eq!(iso_date("T <t@e> 1793494800 +0800").as_deref(), Some("2026-11-01T09:00:00+08:00"));
        assert_eq!(iso_date("T <t@e> 0 -0130").as_deref(), Some("1969-12-31T22:30:00-01:30"));
    }
}
