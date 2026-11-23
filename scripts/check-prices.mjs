#!/usr/bin/env node
// Compares the Claude prices in cmux-core's pricing.rs with Anthropic's
// pricing page, before a release: a price that moved, or a model the page
// lists and the table does not, fails it. Run with `npm run check:prices`.
//
// It reads the page's Markdown form and the Rust table as text, so either
// changing shape makes it fail loudly ("found no rows") rather than pass.

import { readFileSync } from "node:fs";

const PAGE = "https://platform.claude.com/docs/en/about-claude/pricing.md";
const RUST = new URL("../src-tauri/crates/cmux-core/src/pricing.rs", import.meta.url);

// The table in pricing.rs: (id, input, output, Some(cache read) | None).
const source = readFileSync(RUST, "utf8");
const ours = new Map();
for (const m of source.matchAll(/\("(claude-[a-z0-9-]+)",\s*([\d.]+),\s*([\d.]+),\s*(?:Some\(([\d.]+)\)|None)\)/g)) {
  const input = Number(m[2]);
  ours.set(m[1], { input, output: Number(m[3]), cacheRead: m[4] ? Number(m[4]) : input * 0.1 });
}
if (ours.size === 0) fail("found no rows in pricing.rs's TABLE");

const res = await fetch(PAGE);
if (!res.ok) fail(`${PAGE}: HTTP ${res.status}`);
const page = await res.text();

// The first table on the page is "Model pricing": model, base input,
// 5m cache write, 1h cache write, cache hits, output.
const dollars = (cell) => {
  const m = cell.match(/\$([\d.]+)/);
  return m ? Number(m[1]) : NaN;
};
const theirs = new Map();
let inTable = false;
for (const line of page.split("\n")) {
  const cells = line.split("|").slice(1, -1).map((c) => c.trim());
  if (cells[0] === "Model" && cells[1]?.startsWith("Base input")) {
    inTable = true;
    continue;
  }
  if (!inTable) continue;
  if (!line.startsWith("|")) break;
  if (cells[0].startsWith("---")) continue;
  const name = cells[0];
  // Retired models are not worth a row; Haiku 5.5's over-100K prices are
  // not modelled (see pricing.rs).
  if (/retired|over 100,000/i.test(name)) continue;
  const m = name.match(/^Claude (\w+) (\d+(?:\.\d+)?)/);
  if (!m) continue;
  const id = `claude-${m[1].toLowerCase()}-${m[2].replace(".", "-")}`;
  theirs.set(id, { input: dollars(cells[1]), output: dollars(cells[5]), cacheRead: dollars(cells[4]) });
}
if (theirs.size === 0) fail(`found no rows in the model pricing table of ${PAGE}`);

const close = (a, b) => Math.abs(a - b) < 1e-9;
const problems = [];
for (const [id, p] of theirs) {
  const o = ours.get(id);
  if (!o) {
    problems.push(`${id}: on the page ($${p.input} in / $${p.output} out / $${p.cacheRead} cache read), not in pricing.rs`);
    continue;
  }
  for (const key of ["input", "output", "cacheRead"]) {
    if (!close(o[key], p[key])) problems.push(`${id}: ${key} is $${o[key]} in pricing.rs, $${p[key]} on the page`);
  }
}
for (const id of ours.keys()) {
  if (!theirs.has(id)) problems.push(`${id}: in pricing.rs, no longer on the page (retired?)`);
}

if (problems.length) {
  console.error(`Prices differ from ${PAGE}:\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
const checked = source.match(/PRICES_CHECKED: &str = "([\d-]+)"/)?.[1];
console.log(`${ours.size} models match ${PAGE}.`);
console.log(`PRICES_CHECKED is ${checked}; set it to today's date if it is older.`);

function fail(message) {
  console.error(`check-prices: ${message}`);
  process.exit(2);
}
