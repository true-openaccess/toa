//! The six types of TOA 0.1 (chapter 9; `spec/types/`): the fields of their manifests, their checks
//! at sealing, which of them source lists follow (5.2), and the readable names and labels a tool
//! writes for them (6.2, 5.1).
//!
//! Pure computation, like `toa-core`: the object's files and the objects it refers to are handed
//! in. A prefix not registered here is no error: such an object is checked by the common rules
//! only (chapter 9, "a tool that meets a prefix it does not know").

mod ckt;
mod doc;
mod evd;
mod frozen;
mod ifc;
pub mod mc;
pub mod netlist;
pub mod readable;
pub mod reading;
mod tb;
mod v;

pub use evd::{expand, TreeRow};
pub use tb::{agree, METHODS, RESULTS, RUNS, SAMPLINGS, SEED_MAX};

use toa_core::manifest::Node;
use toa_core::Problem;

// ------------------------------------------------------------------ the register (chapter 9)

/// Interface.
pub const IFC: &str = "ifc";
/// Circuit, one generation.
pub const CKT: &str = "ckt";
/// Testbench, one generation.
pub const TB: &str = "tb";
/// Evidence record.
pub const EVD: &str = "evd";
/// Document.
pub const DOC: &str = "doc";
/// Frozen library.
pub const LIB: &str = "lib";

/// The types this crate knows.
pub const KNOWN: &[&str] = &[IFC, CKT, TB, EVD, DOC, LIB];

pub fn is_known(prefix: &str) -> bool {
    KNOWN.contains(&prefix)
}

/// The forms of a circuit the type specification names (`ckt`, `form`). Others may be used and
/// are shown as they are.
pub const FORMS: &[&str] = &["ideal", "va", "rtl", "transistor", "passive", "gates", "composition", "mixed"];

/// The kind of evidence record of a simulation (`evd`, `kind`).
pub const SIM: &str = "sim";
/// The kind of evidence record of a Monte Carlo simulation (`evd`).
pub const MC: &str = "mc";

/// Whether source lists follow objects of this type further (5.2): interfaces, circuits,
/// testbenches and frozen libraries are followed; evidence records, documents and types not known
/// here are not.
pub fn followed(prefix: &str) -> bool {
    matches!(prefix, IFC | CKT | TB | LIB)
}

/// What the checks of a type need of other objects, when they can be found. A check that needs
/// an object that cannot be found is skipped, never failed (the type specifications say "when the
/// objects can be found").
pub trait Lookup {
    /// The manifest of a named object.
    fn manifest(&self, name: &str) -> Option<Node>;
    /// The SHA-256 of one file of a named object, 64 lowercase hexadecimal digits.
    fn file_sha256(&self, name: &str, path: &str) -> Option<String>;
}

/// No other object can be found.
pub struct Nothing;

impl Lookup for Nothing {
    fn manifest(&self, _: &str) -> Option<Node> {
        None
    }
    fn file_sha256(&self, _: &str, _: &str) -> Option<String> {
        None
    }
}

/// One object to check: its manifest, its files (path relative to its top, `/` between levels,
/// and bytes) and its `lock.json` when it has one.
pub struct Object<'a> {
    pub manifest: &'a Node,
    pub files: &'a [(String, Vec<u8>)],
    pub lock: Option<&'a serde_json::Value>,
}

impl Object<'_> {
    pub(crate) fn has(&self, path: &str) -> bool {
        self.files.iter().any(|(p, _)| p == path)
    }
    pub(crate) fn bytes(&self, path: &str) -> Option<&[u8]> {
        self.files.iter().find(|(p, _)| p == path).map(|(_, b)| b.as_slice())
    }
}

/// The checks at sealing of the type `prefix` (its type specification), beyond the common rules
/// that `toa-core` checks. Nothing for a type not known here.
pub fn check(prefix: &str, o: &Object, lookup: &dyn Lookup) -> Vec<Problem> {
    let mut out = Vec::new();
    match prefix {
        IFC => ifc::check(o, &mut out),
        CKT => ckt::check(o, lookup, &mut out),
        TB => tb::check(o, lookup, &mut out),
        EVD => evd::check(o, lookup, &mut out),
        DOC => doc::check(o, &mut out),
        LIB => frozen::check(o, lookup, &mut out),
        _ => {}
    }
    out
}

/// The common fields every type allows (3.2.1); a type adds its own.
pub(crate) const COMMON: &[&str] = &["type", "license", "author", "base", "title", "description", "tags"];

pub(crate) fn allowed(own: &[&'static str]) -> Vec<&'static str> {
    COMMON.iter().chain(own.iter()).copied().collect()
}
