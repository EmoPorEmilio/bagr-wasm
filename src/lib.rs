//! BagIt 0.97 reader/writer.
//!
//! Ported from the Library of Congress `bagit-python` library. The pure-Rust
//! core is in submodules and compiles for any target; the `wasm` module wires
//! a `wasm-bindgen` bridge that lets JavaScript host code provide async file
//! I/O via callbacks.

pub mod error;
pub mod spec;
pub mod hash;
pub mod path;
pub mod bagit_txt;
pub mod bag_info;
pub mod manifest;
pub mod fetch;
pub mod io;
pub mod validate;
pub mod create;

pub use error::{BagError, BagResult};
pub use hash::Algorithm;

#[cfg(target_arch = "wasm32")]
mod wasm;
