// Cross-validation: build a bag with bagr-wasm, then validate it with
// bagit-python (and vice versa). Catches any subtle on-disk-format drift
// from the LoC reference implementation that the static conformance suite
// might not exercise.

import { promises as fs, createReadStream, mkdtempSync } from "node:fs";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { tmpdir } from "node:os";
import { join, sep, relative, dirname } from "node:path";
import { BagBuilder, Validator } from "../../pkg-node/bagr_wasm.js";

const run = promisify(execFile);

async function walk(dir) {
  const out = [];
  for (const e of await fs.readdir(dir, { withFileTypes: true })) {
    const full = join(dir, e.name);
    if (e.isDirectory()) out.push(...(await walk(full)));
    else if (e.isFile()) out.push(full);
  }
  return out;
}

function fsSource(root) {
  return {
    async list() {
      const files = await walk(root);
      return files.map((f) => relative(root, f).split(sep).join("/"));
    },
    async size(p) {
      return (await fs.stat(join(root, p))).size;
    },
    async open(p) {
      const it = createReadStream(join(root, p))[Symbol.asyncIterator]();
      return {
        async next() {
          const { value, done } = await it.next();
          if (done) return { done: true };
          return {
            value: new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
            done: false,
          };
        },
      };
    },
  };
}

function fsSink(root) {
  return {
    async create(p) {
      const full = join(root, p);
      await fs.mkdir(dirname(full), { recursive: true });
      const handle = await fs.open(full, "w");
      return {
        async write(chunk) {
          await handle.write(chunk);
        },
        async close() {
          await handle.close();
        },
      };
    },
  };
}

function tmp() {
  return mkdtempSync(join(tmpdir(), "bagr-cross-"));
}

const BAGIT_CLI = process.env.BAGIT_CLI || "/usr/local/bin/bagit";

async function bagit_py_validate(dir) {
  // `bagit --validate <dir>` returns 0 on success, non-zero on failure.
  try {
    await run(BAGIT_CLI, ["--validate", dir]);
    return { ok: true };
  } catch (e) {
    return { ok: false, stderr: e.stderr?.toString() || String(e) };
  }
}

async function bagit_py_make(dir, args = []) {
  await run(BAGIT_CLI, [...args, dir]);
  // bagit-python writes `BagIt-Version: 1.0` by default. The on-disk format
  // is otherwise identical to 0.97 for everything we test, so rewrite the
  // declaration to exercise the 0.97 read path in bagr-wasm.
  await fs.writeFile(
    join(dir, "bagit.txt"),
    "BagIt-Version: 0.97\nTag-File-Character-Encoding: UTF-8\n",
  );
  // We also need to refresh any tag manifest entries that hashed bagit.txt.
  for (const e of await fs.readdir(dir)) {
    if (!e.startsWith("tagmanifest-")) continue;
    const alg = e.slice("tagmanifest-".length, -".txt".length);
    const lines = (await fs.readFile(join(dir, e), "utf8")).split("\n");
    const out = [];
    for (const line of lines) {
      if (!line.trim()) continue;
      const [_oldHash, ...pathParts] = line.split(/\s+/);
      const path = pathParts.join(" ");
      if (path === "bagit.txt") {
        const bytes = await fs.readFile(join(dir, "bagit.txt"));
        const { createHash } = await import("node:crypto");
        const algName = alg.replace(/-/g, "").toLowerCase();
        const map = { md5: "md5", sha1: "sha1", sha224: "sha224", sha256: "sha256", sha512: "sha512" };
        const h = createHash(map[algName]).update(bytes).digest("hex");
        out.push(`${h}  ${path}`);
      } else {
        out.push(line);
      }
    }
    await fs.writeFile(join(dir, e), out.join("\n") + "\n");
  }
}

function assert(cond, msg) {
  if (!cond) {
    console.error("FAIL:", msg);
    process.exit(1);
  }
  console.log("  ok:", msg);
}

// ---------- 1) bagr-wasm builds → bagit-python validates ----------

console.log("=== bagr-wasm → bagit-python ===\n");

for (const algs of [["md5"], ["sha256"], ["sha512"], ["md5", "sha256"], ["sha256", "sha512"]]) {
  const src = tmp();
  await fs.writeFile(join(src, "a.txt"), "hello\n");
  await fs.mkdir(join(src, "sub"));
  await fs.writeFile(join(src, "sub", "b.bin"), Buffer.from([0, 1, 2, 3, 4, 5]));

  const dst = tmp();
  await new BagBuilder().build(fsSource(src), fsSink(dst), {
    algorithms: algs,
    bagging_date: "2026-05-26",
    bag_info: { "Source-Organization": "cross-validate" },
  });

  const r = await bagit_py_validate(dst);
  assert(r.ok, `bag built with [${algs.join(",")}] accepted by bagit-python`);
  if (!r.ok) console.error("    stderr:", r.stderr);
}

// ---------- 2) bagit-python builds → bagr-wasm validates ----------

console.log("\n=== bagit-python → bagr-wasm ===\n");

const validator = new Validator();
for (const args of [
  ["--md5"],
  ["--sha1"],
  ["--sha256"],
  ["--sha512"],
  ["--md5", "--sha256"],
  ["--sha1", "--sha256", "--sha512"],
]) {
  const src = tmp();
  await fs.writeFile(join(src, "a.txt"), "round-trip\n");
  await fs.mkdir(join(src, "sub"));
  await fs.writeFile(join(src, "sub", "b.bin"), Buffer.from([9, 8, 7, 6, 5]));

  await bagit_py_make(src, args);
  await validator.validate(fsSource(src), {});
  console.log(`  ok: bag built with bagit ${args.join(" ")} accepted by bagr-wasm`);
}

// ---------- 3) Round-trip: bagr-wasm builds → bagit-python validates →
//             bagit-python adds a file via re-save? Just symmetry check. ----

console.log("\n=== Symmetric corruption detection ===\n");

const src = tmp();
await fs.writeFile(join(src, "doc.txt"), "to be corrupted");
const dst = tmp();
await new BagBuilder().build(fsSource(src), fsSink(dst), {
  algorithms: ["sha256"],
  bagging_date: "2026-05-26",
  bag_info: {},
});

// Now flip a byte in data/doc.txt and verify BOTH validators reject it.
await fs.writeFile(join(dst, "data", "doc.txt"), "to be CORRUPTED");

const pyRes = await bagit_py_validate(dst);
assert(!pyRes.ok, "corrupted bag rejected by bagit-python");
let wasmRej = false;
try { await validator.validate(fsSource(dst), {}); } catch { wasmRej = true; }
assert(wasmRej, "corrupted bag rejected by bagr-wasm");

console.log("\nAll cross-validation checks passed.");
