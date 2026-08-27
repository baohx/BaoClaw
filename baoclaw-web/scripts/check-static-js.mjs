#!/usr/bin/env node
/**
 * Static JS syntax checker for baoclaw-web.
 *
 * The web server serves hand-written, non-TypeScript static files from
 * `public/` (app.js, themes.js, ...). `tsc` cannot type-check these, and a
 * duplicate-identifier (or any other parse) error silently kills them at
 * runtime — which is exactly how the duplicate `const isLight` regression
 * slipped through in `8ba45df`.
 *
 * This script parses every `*.js` file under `public/` with Node's own
 * parser and fails on any syntax error. `npm run check:js` wires it up; it
 * also runs as part of `npm run check` and `npm run check:all`.
 *
 * Usage:
 *   node scripts/check-static-js.mjs [--dir public]
 */

import { readdir, readFile } from "node:fs/promises";
import { resolve, join, extname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const root = resolve(__dirname, "..");

// --args resolution----------------------------------------------------------
function parseArgs(argv) {
  const args = { dir: "public" };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--dir" && argv[i + 1]) args.dir = argv[i + 1];
  }
  return args;
}

// --recursive scan------------------------------------------------------------
async function collectJs(dir, out = []) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return out; // missing dir -> nothing to check, not an error
  }
  for (const e of entries) {
    const full = join(dir, e.name);
    if (e.isDirectory()) {
      // Skip node_modules if it ever lands under public/.
      if (e.name === "node_modules") continue;
      await collectJs(full, out);
    } else if (e.isFile() && extname(e.name) === ".js") {
      out.push(full);
    }
  }
  return out;
}

// --main------------------------------------------------------------------------
const { dir } = parseArgs(process.argv.slice(2));
const target = resolve(root, dir);

const files = await collectJs(target);

if (files.length === 0) {
  console.log(
    `check-static-js: no .js files found under ${dir}/ (nothing to check).`,
  );
  process.exit(0);
}

let failed = 0;
for (const file of files) {
  const src = await readFile(file, "utf8");
  try {
    // Parse-only: validates syntax without executing anything.
    new Function(src); // eslint-disable-line no-new-func
    console.log(`  ok    ${file.replace(root + "/", "")}`);
  } catch (err) {
    failed++;
    console.error(`  FAIL  ${file.replace(root + "/", "")}`);
    console.error(`        ${err.message}`);
  }
}

console.log(
  `\ncheck-static-js: ${files.length - failed}/${files.length} files passed.`,
);
if (failed > 0) {
  console.error(`check-static-js: ${failed} file(s) failed syntax check.`);
  process.exit(1);
}
