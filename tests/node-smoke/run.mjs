// Smoke test that drives the wasm bridge from real JS. Builds a bag in
// memory, validates it, mutates it, and checks the validator surfaces the
// corruption. Run with `node tests/node-smoke/run.mjs` after
// `cargo build --target wasm32-unknown-unknown --release && wasm-bindgen ...`.

import { Validator, BagBuilder, bagit_version } from "../../pkg-node/bagr_wasm.js";

console.log("BagIt version:", bagit_version());

// ---- Host implementations: in-memory source/sink ----

function makeSource(files /* Map<string, Uint8Array> */) {
  return {
    async list() {
      return Array.from(files.keys());
    },
    async size(path) {
      const b = files.get(path);
      if (!b) throw new Error("missing: " + path);
      return b.length;
    },
    async open(path) {
      const b = files.get(path);
      if (!b) throw new Error("missing: " + path);
      let done = false;
      return {
        async next() {
          if (done) return { done: true };
          done = true;
          return { value: b, done: false };
        },
      };
    },
  };
}

function makeSink() {
  const files = new Map();
  return {
    files,
    async create(path) {
      const chunks = [];
      return {
        async write(chunk) {
          chunks.push(Uint8Array.from(chunk));
        },
        async close() {
          const total = chunks.reduce((n, c) => n + c.length, 0);
          const out = new Uint8Array(total);
          let off = 0;
          for (const c of chunks) {
            out.set(c, off);
            off += c.length;
          }
          files.set(path, out);
        },
      };
    },
  };
}

function assert(cond, msg) {
  if (!cond) {
    console.error("FAIL:", msg);
    process.exit(1);
  }
  console.log("  ok:", msg);
}

const enc = new TextEncoder();

// ---- 1) Build a bag ----

const payload = new Map();
payload.set("hello.txt", enc.encode("hello\n"));
payload.set("nested/world.bin", new Uint8Array([0, 1, 2, 3, 4, 5]));

const sink = makeSink();
const builder = new BagBuilder();
await builder.build(makeSource(payload), sink, {
  algorithms: ["md5", "sha256"],
  bagging_date: "2026-05-26",
  bag_info: { "Source-Organization": "bagr-wasm smoke" },
});

console.log("\nBag files:");
for (const k of sink.files.keys()) console.log("  -", k);

assert(sink.files.has("bagit.txt"), "bagit.txt written");
assert(sink.files.has("manifest-md5.txt"), "manifest-md5.txt written");
assert(sink.files.has("manifest-sha256.txt"), "manifest-sha256.txt written");
assert(sink.files.has("tagmanifest-md5.txt"), "tagmanifest-md5.txt written");
assert(sink.files.has("data/hello.txt"), "payload preserved under data/");

const bagitTxt = new TextDecoder().decode(sink.files.get("bagit.txt"));
console.log("\nbagit.txt:\n" + bagitTxt);
assert(bagitTxt.includes("BagIt-Version: 0.97"), "declares 0.97");
assert(!bagitTxt.startsWith("﻿"), "no UTF-8 BOM");

const md5Manifest = new TextDecoder().decode(sink.files.get("manifest-md5.txt"));
console.log("manifest-md5.txt:\n" + md5Manifest);
assert(/\b[0-9a-f]{32}  data\/hello\.txt\n/.test(md5Manifest), "md5 line uses two-space separator");

// ---- 2) Validate the freshly built bag ----

const validator = new Validator();
const report = await validator.validate(makeSource(sink.files), {});
console.log("\nValidation report:", report);
assert(report.payload_files === 2, "2 payload files counted");
assert(report.payload_octets === enc.encode("hello\n").length + 6, "payload octets correct");
assert(report.payload_manifests.length === 2, "both payload manifests detected");

// ---- 3) Fast validate using the Payload-Oxum shortcut ----

const fastReport = await validator.validate(makeSource(sink.files), { fast: true });
console.log("Fast report:", fastReport);

// ---- 4) Corrupt a payload byte and ensure detection ----

const corrupted = new Map(sink.files);
corrupted.set("data/hello.txt", enc.encode("HELLO\n"));
let caught = null;
try {
  await validator.validate(makeSource(corrupted), {});
} catch (e) {
  caught = String(e);
}
console.log("\nCorruption error:", caught);
assert(caught && /checksum mismatch/i.test(caught), "checksum mismatch surfaced to JS");

// ---- 5) Fast mode catches size changes via Payload-Oxum ----

const wrongSize = new Map(sink.files);
wrongSize.set("data/hello.txt", enc.encode("extra-bytes\n"));
let oxumErr = null;
try {
  await validator.validate(makeSource(wrongSize), { fast: true });
} catch (e) {
  oxumErr = String(e);
}
console.log("Oxum error:", oxumErr);
assert(oxumErr && /oxum/i.test(oxumErr), "fast mode catches size mismatch");

console.log("\nAll smoke checks passed.");
