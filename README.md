# bagr-wasm

A Rust port of the Library of Congress
[`bagit-python`](https://github.com/LibraryOfCongress/bagit-python) library,
**BagIt 0.97** only, compiled to WebAssembly for browser use.

Two services are exposed to JavaScript:

- **`Validator`** — reads an existing bag and verifies it against the 0.97
  spec (structure, manifests, tag manifests, checksums, `Payload-Oxum`).
- **`BagBuilder`** — assembles a fresh, spec-compliant bag from a flat list
  of payload files.

## Building

```sh
bash scripts/build.sh
```

Produces:
- `pkg/`     — `--target web` (ES modules, importable directly in browsers)
- `pkg-node/` — `--target nodejs` (CommonJS; used by the smoke test)

## Testing

```sh
bash scripts/test.sh
```

Runs four suites in sequence:

1. **`cargo test`** — 34 native unit + integration tests.
2. **`tests/node-smoke/run.mjs`** — 17 checks driving the wasm bridge from
   real JS in Node: create, validate, corruption detection, fast mode,
   fetch hook, error surfaces.
3. **`tests/conformance/run.mjs`** — runs the Library of Congress
   `bagit-conformance-suite` v0.97 fixtures through the validator. The
   fixtures are vendored at `tests/fixtures/conformance/v0.97/` (CC0,
   public domain — see `LICENSE.md` there). 12 valid bags must pass, 11
   invalid bags must fail.
4. **`tests/cross-validate/run.mjs`** — *optional*. If the `bagit` CLI is on
   `PATH` (`pip install bagit`), round-trips bags between bagr-wasm and
   bagit-python: builds with bagr-wasm in 5 algorithm combos and asks
   bagit-python to validate, builds with bagit-python in 6 algorithm combos
   and asks bagr-wasm to validate, and corrupts one bag to check both
   implementations reject it. The script skips this suite when `bagit`
   isn't installed.

Prerequisites for the full run: a Rust toolchain with the
`wasm32-unknown-unknown` target installed, `wasm-bindgen-cli` (matching
crate version), Node 18+, and optionally Python with `bagit` for suite 4.

## Using from JavaScript

The host (your code) plays the role of the file system. You provide two
small objects:

```js
import init, { Validator, BagBuilder } from "./pkg/bagr_wasm.js";
await init();

// Read side: list files, optionally report a size, open a pull-style async
// reader yielding Uint8Array chunks, and optionally fetch URLs from fetch.txt.
const source = {
  async list()       { return [...files.keys()]; },
  async size(path)   { return files.get(path).size; },   // optional, enables fast=true
  async open(path) {
    const stream = files.get(path).stream().getReader();
    return { async next() {
      const { value, done } = await stream.read();
      return done ? { done: true } : { value, done: false };
    }};
  },
  async fetch(url) {                                     // optional
    const res = await window.fetch(url);
    const reader = res.body.getReader();
    return { async next() {
      const { value, done } = await reader.read();
      return done ? { done: true } : { value, done: false };
    }};
  },
};

// Write side: create returns a writer with write(chunk)/close().
const sink = {
  files: new Map(),
  async create(path) {
    const chunks = [];
    return {
      async write(chunk) { chunks.push(chunk); },
      async close()      { this.files.set(path, concat(chunks)); },
    };
  },
};

await new BagBuilder().build(source, sink, {
  algorithms: ["sha256", "sha512"],   // default if omitted
  bagging_date: new Date().toISOString().slice(0, 10),
  bag_info: { "Source-Organization": "Example" },
});

const report = await new Validator().validate(source, { fast: false });
// { payload_files, payload_octets, payload_manifests, tag_manifests, held_files }
```

## Spec parity with bagit-python

Behaviors matched byte-for-byte where they affect the on-disk bag:

- `bagit.txt` written with `BagIt-Version: 0.97`, no BOM. BOM rejected on read.
- `manifest-*.txt` lines use **two** spaces between digest and path; paths
  encode CR→`%0D`, LF→`%0A`, nothing else.
- `bag-info.txt` headers sorted alphabetically, embedded CR/LF stripped from
  values, RFC 2822 line folding supported on read, repeated keys preserved.
- `make_bag` defaults: `Bag-Software-Agent`, `Bagging-Date`, `Payload-Oxum`
  auto-populated. (`Bagging-Date` must be supplied from JS — wasm has no clock.)
- Completeness check aggregates entries across all payload manifests.
- `fast=True` validates only `Payload-Oxum` and requires it to be present.
- Default checksums: `sha256`, `sha512`.
- `fetch.txt` URLs validated: must have scheme+netloc, or use `file:` scheme.
  Paths must live under `data/`. Held files are not exempt from completeness
  (matches bagit-python); supply a `fetch(url)` method on the source if you
  want validation to materialize them itself.
- `data/` payload required: a bag with no payload files anywhere (on disk
  or via `fetch.txt`) is rejected.

Beyond bagit-python:
- Optional `source.fetch(url)` hook lets the validator download held files
  during validation, rather than failing as bagit-python does.

Not yet implemented:
- Parallel hashing (`processes>1`); we hash sequentially.

## Layout

```
src/
  spec.rs        0.97 constants
  bagit_txt.rs   bagit.txt parse/serialize
  bag_info.rs    bag-info.txt
  manifest.rs    manifest-*.txt and tagmanifest-*.txt
  fetch.rs       fetch.txt
  hash.rs        md5 / sha256 / sha512 streaming hashers
  path.rs        path normalization + manifest-path encoding
  io.rs          BagSource / BagSink async traits, in-memory fake
  validate.rs    validation pipeline
  create.rs      bag-creation pipeline
  wasm.rs        wasm-bindgen bridge (JS object ↔ Rust traits)
  lib.rs
```
