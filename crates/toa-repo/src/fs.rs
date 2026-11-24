//! Reading an object directory from disk, with the limits of 3.1 applied while reading.

use std::path::Path;

use toa_core::object::{self, Verified};
use toa_core::tree;
use toa_core::Problem;

/// `dir` spelled from `root`, the root of its working directory as git gives it, so that `root`
/// is a prefix of it; `None` when `dir` is not inside `root`. The user may spell a path otherwise
/// than git does: an 8.3 short name or other letter case on Windows, or through a symbolic link
/// (macOS's `/var` is `/private/var`); comparing the two spellings as they are would find
/// nothing inside.
pub fn under(root: &Path, dir: &Path) -> Option<std::path::PathBuf> {
    let from = |rel: &Path| if rel.as_os_str().is_empty() { root.to_path_buf() } else { root.join(rel) };
    if let Ok(rel) = std::path::absolute(dir).ok()?.strip_prefix(root) {
        return Some(from(rel));
    }
    let (r, d) = (std::fs::canonicalize(root).ok()?, std::fs::canonicalize(dir).ok()?);
    d.strip_prefix(&r).ok().map(from)
}

/// Verify an object directory from its files (7.4), then check what its type specification asks
/// (`toa-types`), with no other object to look at: the checks that need other objects are skipped.
pub fn verify_dir_with_type(dir: &Path) -> std::io::Result<Verified> {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let mut v = verify_dir(dir)?;
    if toa_core::problem::has_errors(&v.problems) {
        return Ok(v);
    }
    let Some(m) = v.manifest.clone() else { return Ok(v) };
    let c = read_dir(dir)?;
    let lock = c
        .files
        .iter()
        .find(|(p, _)| p == tree::LOCK)
        .and_then(|(_, b)| serde_json::from_slice::<serde_json::Value>(b).ok());
    let prefix = toa_core::name::prefix_of(&name).unwrap_or("");
    let o = toa_types::Object { manifest: &m, files: &c.files, lock: lock.as_ref() };
    v.problems.extend(toa_types::check(prefix, &o, &toa_types::Nothing));
    Ok(v)
}

/// The members of a directory as read from disk.
#[derive(Debug, Default)]
pub struct DirContents {
    /// Files: path relative to the directory, with `/` between levels, and bytes.
    pub files: Vec<(String, Vec<u8>)>,
    pub dirs: Vec<String>,
    /// Files with the executable bit (never on Windows, which cannot say, 3.1).
    pub executable: Vec<String>,
    /// Problems met while reading: links, names that are not UTF-8, limits passed.
    pub problems: Vec<Problem>,
}

/// Read every member of `dir`. Symbolic links and junctions are reported, never followed; reading
/// stops at the limits of 3.1 rather than reading everything first.
pub fn read_dir(dir: &Path) -> std::io::Result<DirContents> {
    let mut out = DirContents::default();
    let mut total: u64 = 0;
    walk(dir, "", 0, &mut out, &mut total)?;
    out.files.sort();
    out.dirs.sort();
    Ok(out)
}

fn walk(dir: &Path, rel: &str, depth: usize, out: &mut DirContents, total: &mut u64) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            out.problems.push(Problem::error("E_TREE_NAME", "3.1", rel, "a name that is not UTF-8"));
            continue;
        };
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let meta = std::fs::symlink_metadata(e.path())?;
        let ft = meta.file_type();
        if ft.is_symlink() || is_reparse_point(&meta) {
            out.problems.push(Problem::error("E_TREE_KIND", "3.1", &path, "a link has no place in an object"));
        } else if ft.is_dir() {
            if depth > tree::MAX_DEPTH {
                out.problems.push(Problem::error("E_TREE_DEPTH", "3.1", &path, "too deep; not read further"));
                continue;
            }
            out.dirs.push(path.clone());
            walk(&e.path(), &path, depth + 1, out, total)?;
        } else if ft.is_file() {
            if out.files.len() >= tree::MAX_FILES {
                out.problems.push(Problem::error(
                    "E_TREE_FILE_COUNT",
                    "3.1",
                    &path,
                    "too many files; not read further",
                ));
                return Ok(());
            }
            let len = meta.len();
            if len > tree::MAX_FILE_BYTES || *total + len > tree::MAX_OBJECT_BYTES {
                out.problems.push(Problem::error("E_TREE_FILE_SIZE", "3.1", &path, "too large; not read"));
                continue;
            }
            *total += len;
            if is_executable(&meta) {
                out.executable.push(path.clone());
            }
            out.files.push((path, std::fs::read(e.path())?));
        } else {
            out.problems.push(Problem::error("E_TREE_KIND", "3.1", &path, "only plain files and directories"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &std::fs::Metadata) -> bool {
    false
}

/// Verify an object directory from its files (7.4). The name is the directory's own name.
pub fn verify_dir(dir: &Path) -> std::io::Result<Verified> {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let c = read_dir(dir)?;
    let files: Vec<(&str, &[u8])> = c.files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let dirs: Vec<&str> = c.dirs.iter().map(String::as_str).collect();
    let exe: Vec<&str> = c.executable.iter().map(String::as_str).collect();
    let mut v = object::verify_files(&name, &files, &dirs, &exe);
    if !c.problems.is_empty() {
        let mut p = c.problems;
        p.append(&mut v.problems);
        v.problems = p;
        v.hash = None;
    }
    Ok(v)
}
