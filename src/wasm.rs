//! `wasm-bindgen` bridge. Compiled only for `wasm32` targets.
//!
//! This is intentionally minimal in v0: it exposes the library version and a
//! `panic_hook` initializer. The JS-driven async I/O bridge (turning a JS
//! object with `list/open/create` methods into [`crate::io::BagSource`] /
//! [`crate::io::BagSink`]) will be filled in next; the core algorithms in
//! `validate.rs` and `create.rs` are already written against the trait so the
//! bridge is the only missing piece.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn bagit_version() -> String {
    crate::spec::BAGIT_VERSION.to_string()
}
