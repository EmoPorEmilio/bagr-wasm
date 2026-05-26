# bagr-wasm

A Rust port of the Library of Congress
[`bagit-python`](https://github.com/LibraryOfCongress/bagit-python) library,
targeting **BagIt 0.97** only and compiled to WebAssembly for use in the
browser.

## Status

Early scaffolding. The pure-Rust core (`bagit.txt`, `bag-info.txt`,
`manifest-*.txt`, `tagmanifest-*.txt`, `fetch.txt`, validation, creation) is
in place with native unit + integration tests. The browser-facing
`wasm-bindgen` bridge that lets JS code pass in async file readers/writers is
still TODO.

## Layout

- `src/spec.rs` — 0.97 constants.
- `src/bagit_txt.rs`, `src/bag_info.rs`, `src/manifest.rs`, `src/fetch.rs` —
  tag-file parsers/serializers.
- `src/hash.rs` — MD5, SHA-256, SHA-512 streaming hashers.
- `src/io.rs` — `BagSource` / `BagSink` async traits, plus an in-memory fake
  used by tests.
- `src/validate.rs` — validation algorithm.
- `src/create.rs` — bag-creation algorithm.
- `src/wasm.rs` — `wasm-bindgen` bridge (stub for now).

## Building

```sh
cargo test                              # native tests
cargo build --target wasm32-unknown-unknown --release
```
