//! The rules of TOA 0.1 as pure computation.
//!
//! Nothing here reads files, starts processes or opens connections, so the crate builds for
//! WebAssembly as it is. Section numbers such as (2.2) refer to the standard.

pub mod draft;
pub mod fsck;
pub mod hash;
pub mod history;
pub mod library;
pub mod manifest;
pub mod members;
pub mod name;
pub mod object;
pub mod problem;
pub mod source;
pub mod spdx;
pub mod tree;

pub use problem::{Level, Problem};
