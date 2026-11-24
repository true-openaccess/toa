//! The hash of a set of files, and the cross-check hash (2.2).
//!
//! The hash is what git would give the directory as a tree id, computed here without git: blob ids
//! from the file bytes, tree ids from sorted entries. SHA-1 runs with collision detection (2.2), and
//! a detected collision is an error, never a name.

use std::collections::BTreeMap;

use sha1_checked::digest::Update;
use sha2::Digest;

/// The hash function: SHA-1 gives the name, SHA-256 the cross-check hash (2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
}

impl Algorithm {
    /// Length of an id in bytes: 20 or 32.
    pub fn id_len(self) -> usize {
        match self {
            Algorithm::Sha1 => 20,
            Algorithm::Sha256 => 32,
        }
    }
}

/// Why no hash could be given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashError {
    /// SHA-1 found the marks of a collision attack in the input (2.2).
    Collision,
    /// A path is empty, has an empty level, or is both a file and a directory.
    BadPath(String),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashError::Collision => write!(f, "SHA-1 detected a collision attack in the input"),
            HashError::BadPath(p) => write!(f, "cannot place '{p}' in a tree"),
        }
    }
}

impl std::error::Error for HashError {}

enum Inner {
    Sha1(Box<sha1_checked::Sha1>),
    Sha256(sha2::Sha256),
}

/// An incremental hash of either function.
pub struct Hasher(Inner);

impl Hasher {
    pub fn new(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Sha1 => Hasher(Inner::Sha1(Box::new(sha1_checked::Sha1::new()))),
            Algorithm::Sha256 => Hasher(Inner::Sha256(sha2::Sha256::new())),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match &mut self.0 {
            Inner::Sha1(h) => Update::update(h.as_mut(), data),
            Inner::Sha256(h) => Digest::update(h, data),
        }
    }

    pub fn finish(self) -> Result<Vec<u8>, HashError> {
        match self.0 {
            Inner::Sha1(h) => {
                let r = h.try_finalize();
                if r.has_collision() {
                    Err(HashError::Collision)
                } else {
                    Ok(r.hash().to_vec())
                }
            }
            Inner::Sha256(h) => Ok(h.finalize().to_vec()),
        }
    }
}

/// The hash of some bytes.
pub fn digest(algorithm: Algorithm, data: &[u8]) -> Result<Vec<u8>, HashError> {
    let mut h = Hasher::new(algorithm);
    h.update(data);
    h.finish()
}

/// The id of a file: the hash of `"blob " + length + 0 byte + bytes` (2.2, step 1).
pub fn blob_id(algorithm: Algorithm, content: &[u8]) -> Result<Vec<u8>, HashError> {
    let mut h = Hasher::new(algorithm);
    h.update(format!("blob {}\0", content.len()).as_bytes());
    h.update(content);
    h.finish()
}

/// The id of a tree from its body: the hash of `"tree " + length + 0 byte + body` (2.2, step 2).
pub fn tree_id_of_body(algorithm: Algorithm, body: &[u8]) -> Result<Vec<u8>, HashError> {
    let mut h = Hasher::new(algorithm);
    h.update(format!("tree {}\0", body.len()).as_bytes());
    h.update(body);
    h.finish()
}

/// Lowercase hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(D[(b >> 4) as usize] as char);
        s.push(D[(b & 15) as usize] as char);
    }
    s
}

/// Bytes from lowercase hexadecimal; `None` for anything else, including upper case.
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    b.chunks(2).map(|p| Some((val(p[0])? << 4) | val(p[1])?)).collect()
}

/// One entry of a directory being hashed.
#[derive(Debug, Clone)]
pub enum Node {
    /// A file, by its blob id.
    File(Vec<u8>),
    Dir(Dir),
}

/// A directory being hashed: names to entries. Files are held by id, so the same tree can be
/// built from file bytes (verifying from the files) or from a git listing (verifying from the
/// listing, 7.4).
#[derive(Debug, Clone, Default)]
pub struct Dir {
    entries: BTreeMap<String, Node>,
}

impl Dir {
    pub fn new() -> Self {
        Dir::default()
    }

    /// Place a file id at a path with `/` between levels, making the directories on the way.
    pub fn insert_file(&mut self, path: &str, id: Vec<u8>) -> Result<(), HashError> {
        let bad = || HashError::BadPath(path.to_string());
        let mut parts = path.split('/').peekable();
        let mut cur = self;
        while let Some(part) = parts.next() {
            if part.is_empty() {
                return Err(bad());
            }
            if parts.peek().is_none() {
                if cur.entries.contains_key(part) {
                    return Err(bad());
                }
                cur.entries.insert(part.to_string(), Node::File(id));
                return Ok(());
            }
            let node = cur.entries.entry(part.to_string()).or_insert_with(|| Node::Dir(Dir::new()));
            cur = match node {
                Node::Dir(d) => d,
                Node::File(_) => return Err(bad()),
            };
        }
        Err(bad())
    }

    /// The canonical tree body of this directory (2.2, step 2).
    fn body(&self, algorithm: Algorithm) -> Result<Vec<u8>, HashError> {
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(self.entries.len());
        for (name, node) in &self.entries {
            let (mode, key, id) = match node {
                Node::File(id) => ("100644", name.clone(), id.clone()),
                Node::Dir(d) => ("40000", format!("{name}/"), d.id(algorithm)?),
            };
            let mut row = format!("{mode} {name}\0").into_bytes();
            row.extend_from_slice(&id);
            rows.push((key.into_bytes(), row));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(rows.into_iter().flat_map(|r| r.1).collect())
    }

    /// The id of this directory.
    pub fn id(&self, algorithm: Algorithm) -> Result<Vec<u8>, HashError> {
        tree_id_of_body(algorithm, &self.body(algorithm)?)
    }
}

/// The hash of a set of files given as path and bytes, in hexadecimal (2.2).
pub fn hash_files(algorithm: Algorithm, files: &[(&str, &[u8])]) -> Result<String, HashError> {
    let mut dir = Dir::new();
    for (path, bytes) in files {
        dir.insert_file(path, blob_id(algorithm, bytes)?)?;
    }
    Ok(hex(&dir.id(algorithm)?))
}

/// One entry of a git tree as stored, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub mode: Vec<u8>,
    pub name: Vec<u8>,
    pub id: Vec<u8>,
}

/// The entries of a stored git tree body; `None` when the body cannot be read as entries.
pub fn parse_tree_body(algorithm: Algorithm, body: &[u8]) -> Option<Vec<RawEntry>> {
    let n = algorithm.id_len();
    let mut out = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let sp = rest.iter().position(|&c| c == b' ')?;
        let nul = sp + 1 + rest[sp + 1..].iter().position(|&c| c == 0)?;
        if rest.len() < nul + 1 + n {
            return None;
        }
        out.push(RawEntry {
            mode: rest[..sp].to_vec(),
            name: rest[sp + 1..nul].to_vec(),
            id: rest[nul + 1..nul + 1 + n].to_vec(),
        });
        rest = &rest[nul + 1 + n..];
    }
    Some(out)
}

/// Whether a stored git tree body is written canonically: only the modes `100644` and `40000`,
/// names that are not empty and hold no `/`, entries in the order of 2.2 and none twice. A body
/// that is not gives the tree an id other than the hash of the files it lists (2.2, 7.4).
pub fn is_canonical_tree_body(algorithm: Algorithm, body: &[u8]) -> bool {
    let Some(entries) = parse_tree_body(algorithm, body) else {
        return false;
    };
    let mut prev: Option<Vec<u8>> = None;
    for e in &entries {
        let dir = match e.mode.as_slice() {
            b"100644" => false,
            b"40000" => true,
            _ => return false,
        };
        if e.name.is_empty() || e.name.contains(&b'/') || e.name == b"." || e.name == b".." {
            return false;
        }
        let mut key = e.name.clone();
        if dir {
            key.push(b'/');
        }
        if prev.as_ref().is_some_and(|p| *p >= key) {
            return false;
        }
        prev = Some(key);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_of_2_3() {
        let object: &[(&str, &[u8])] = &[("toa.yaml", b"type: doc\nlicense: MIT\n"), ("README.md", b"hello\n")];
        assert_eq!(hash_files(Algorithm::Sha1, object).unwrap(), "ea44863a8d2f242bc651caf709d45a27e7531d9d");
        assert_eq!(
            hash_files(Algorithm::Sha256, object).unwrap(),
            "e282624f05b86cc1bae3ef0abf32ad2540fb72fc15d309cea0988289b9ee2bd9"
        );
        let order: &[(&str, &[u8])] = &[("a.txt", b"1\n"), ("a/z", b"2\n"), ("a-b", b"3\n"), ("a0", b"4\n")];
        assert_eq!(hash_files(Algorithm::Sha1, order).unwrap(), "497bcf8f72cdfb77f6ab7d4e0ac6cfeaafa81c8c");
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(from_hex(&hex(&[0, 1, 0xab, 0xff])).unwrap(), vec![0, 1, 0xab, 0xff]);
        assert!(from_hex("AB").is_none());
    }

    #[test]
    fn paths_that_cannot_be_placed() {
        let mut d = Dir::new();
        d.insert_file("a/b", vec![0; 20]).unwrap();
        assert!(d.insert_file("a", vec![0; 20]).is_err());
        assert!(d.insert_file("a/b/c", vec![0; 20]).is_err());
        assert!(d.insert_file("x//y", vec![0; 20]).is_err());
    }
}
