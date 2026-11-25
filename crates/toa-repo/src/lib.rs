//! TOA on disk and in git.
//!
//! Every call of git goes through one interface, so that the system's `git` and a compiled-in
//! implementation can stand behind it.

pub mod assemble;
#[cfg(feature = "builtin-git")]
pub mod builtin;
pub mod derive;
pub mod fetch;
pub mod fs;
pub mod git;
pub mod index;
pub mod latest;
pub mod library;
pub mod publish;
pub mod remove;
pub mod run;
pub mod same;
pub mod seal;
pub mod status;

/// The type specifications, as this crate uses them.
pub use toa_types as types;
