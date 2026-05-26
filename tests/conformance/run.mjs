// Run bagr-wasm against the Library of Congress BagIt conformance suite
// (https://github.com/LibraryOfCongress/bagit-conformance-suite), v0.97
// valid/ and invalid/ directories. The suite must be cloned to
// /tmp/bagit-conformance-suite (or override with $CONFORMANCE_DIR).

import { promises as fs, createReadStream } from "node:fs";
import { join, relative, sep } from "node:path";
import { Validator } from "../../pkg-node/bagr_wasm.js";

const root =
  process.env.CONFORMANCE_DIR || "/tmp/bagit-conformance-suite/v0.97";

async function walk(dir) {
  const out = [];
  for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await walk(full)));
    else if (entry.isFile()) out.push(full);
  }
  return out;
}

function makeFsSource(bagDir) {
  return {
    async list() {
      const files = await walk(bagDir);
      return files.map((f) => relative(bagDir, f).split(sep).join("/"));
    },
    async size(path) {
      const st = await fs.stat(join(bagDir, path));
      return st.size;
    },
    async open(path) {
      const stream = createReadStream(join(bagDir, path), {
        highWaterMark: 64 * 1024,
      });
      const it = stream[Symbol.asyncIterator]();
      return {
        async next() {
          const { value, done } = await it.next();
          if (done) return { done: true };
          // Node streams yield Buffer; wasm bridge wants Uint8Array.
          return { value: new Uint8Array(value.buffer, value.byteOffset, value.byteLength), done: false };
        },
      };
    },
  };
}

async function runOne(bagDir, expectValid) {
  const validator = new Validator();
  try {
    await validator.validate(makeFsSource(bagDir), {});
    return { ok: expectValid, msg: "valid" };
  } catch (e) {
    return { ok: !expectValid, msg: String(e) };
  }
}

async function runSet(subdir, expectValid) {
  const dir = join(root, subdir);
  const entries = await fs.readdir(dir, { withFileTypes: true });
  const bags = entries
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .sort();

  let pass = 0;
  let fail = 0;
  const failures = [];
  for (const name of bags) {
    const r = await runOne(join(dir, name), expectValid);
    const marker = r.ok ? "PASS" : "FAIL";
    console.log(`  ${marker}  ${subdir}/${name}`);
    if (!r.ok) {
      console.log(`         got: ${r.msg.split("\n")[0]}`);
      failures.push({ name: `${subdir}/${name}`, msg: r.msg });
      fail++;
    } else {
      pass++;
    }
  }
  return { pass, fail, failures };
}

console.log(`Conformance suite: ${root}\n`);
console.log(`-- VALID bags (should all pass) --`);
const v = await runSet("valid", true);
console.log(`\n-- INVALID bags (should all fail) --`);
const i = await runSet("invalid", false);

const total = v.pass + v.fail + i.pass + i.fail;
const okTotal = v.pass + i.pass;
console.log(
  `\nSummary: ${okTotal}/${total} expectations met (valid ${v.pass}/${v.pass + v.fail}, invalid ${i.pass}/${i.pass + i.fail}).`,
);
if (okTotal !== total) process.exit(1);
