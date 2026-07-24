#!/usr/bin/env node
// Rewrite web/src/config.ts to point the download button at a released version.
//
// Usage:
//   node web/scripts/sync-download.mjs           # uses root package.json "version"
//   node web/scripts/sync-download.mjs v0.1.2     # explicit tag or version
//
// Kept as the single source of the rewrite logic so CI and humans behave identically.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url)); // web/scripts
const configPath = resolve(here, "../src/config.ts");
const rootPkgPath = resolve(here, "../../package.json");

const version = (
  process.argv[2] ?? JSON.parse(readFileSync(rootPkgPath, "utf8")).version
).replace(/^v/, "");

if (!/^\d+\.\d+\.\d+/.test(version)) {
  console.error(`Refusing to sync: "${version}" is not a semver version.`);
  process.exit(1);
}

const REPO = "SuperHomer/mirador";
const asset = `Mirador_${version}_aarch64.dmg`;
const url = `https://github.com/${REPO}/releases/download/v${version}/${asset}`;

let src = readFileSync(configPath, "utf8");
src = src.replace(
  /export const DOWNLOAD_URL =\s*[\s\S]*?;/,
  `export const DOWNLOAD_URL =\n  "${url}";`,
);
src = src.replace(
  /export const DOWNLOAD_VERSION = ".*?";/,
  `export const DOWNLOAD_VERSION = "v${version}";`,
);
writeFileSync(configPath, src);
console.log(`Landing page synced to v${version}`);
