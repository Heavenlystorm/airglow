
// Parse every browser module the way a browser does, and check that a named import exists in the module it comes
// from. "node --check" parses as a script and accepts a few things Chromium rejects (a stray template tag, for one),
// which shipped a blank window once: this is the gate for that.
//
//   node --experimental-vm-modules tools/check-js.mjs

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dirs = [join(root, "web", "js"), join(root, "web", "runtime")];
const files = process.argv.length > 2
  ? process.argv.slice(2).map((f) => resolve(f))
  : dirs.flatMap((dir) => readdirSync(dir).filter((n) => n.endsWith(".js")).map((n) => join(dir, n)));

const problems = [];
const sources = new Map(files.map((f) => [f, readFileSync(f, "utf8")]));

for (const [file, source] of sources) {
  try {
    new vm.SourceTextModule(source, { identifier: file });
  } catch (e) {
    problems.push(relative(root, file) + ": " + e.message);
  }
}

const DECLARED = /\bexport\s+(?:async\s+)?(?:function|const|let|var|class)\s+([A-Za-z_$][\w$]*)/g;
const LISTED = /\bexport\s*\{([^}]*)\}/g;
const IMPORT = /\bimport\s+([^;]*?)\s*from\s*["']([^"']+)["']/g;
const DYNAMIC = /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g;

function exportsOf(file) {
  const src = sources.get(file) ?? readFileSync(file, "utf8");
  const names = new Set();
  for (const m of src.matchAll(DECLARED)) names.add(m[1]);
  for (const m of src.matchAll(LISTED)) {
    for (const part of m[1].split(",")) {
      const bits = part.trim().split(/\s+as\s+/);
      if (bits.length && bits[bits.length - 1].trim()) names.add(bits[bits.length - 1].trim());
    }
  }
  return names;
}

for (const [file, source] of sources) {
  for (const m of source.matchAll(IMPORT)) {
    const spec = m[2];
    if (!spec.startsWith(".")) continue;
    const target = resolve(dirname(file), spec);
    if (!existsSync(target)) {
      problems.push(relative(root, file) + ": no such module " + spec);
      continue;
    }
    const clause = m[1].trim();
    if (!clause.startsWith("{")) continue;
    const have = exportsOf(target);
    for (const part of clause.replace(/[{}]/g, "").split(",")) {
      const name = part.trim().split(/\s+as\s+/)[0].trim();
      if (name && !have.has(name)) problems.push(relative(root, file) + ": " + spec + " has no export named " + name);
    }
  }
  for (const m of source.matchAll(DYNAMIC)) {
    if (!m[1].startsWith(".")) continue;
    if (!existsSync(resolve(dirname(file), m[1]))) problems.push(relative(root, file) + ": no such module " + m[1]);
  }
}

if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log("checked " + files.length + " browser modules");
