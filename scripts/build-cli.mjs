#!/usr/bin/env node
// Builds the release `mira` CLI and stages it as a Tauri sidecar so it ships
// inside the app bundle (Mirador.app/Contents/MacOS/mira, or next to
// mirador.exe on Windows). Tauri requires the target triple suffix on
// sidecar binaries. Node rather than sh: Windows has no shell to rely on.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = join(root, "src-tauri", "Cargo.toml");

const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .split("\n")
  .find((line) => line.startsWith("host:"));
if (!host) {
  console.error("build-cli: could not read the host target triple from rustc");
  process.exit(1);
}
const triple = host.slice("host:".length).trim();

execFileSync(
  "cargo",
  ["build", "--release", "-p", "cmux-cli", "--manifest-path", manifest],
  { stdio: "inherit" },
);

const exe = process.platform === "win32" ? ".exe" : "";
const binaries = join(root, "src-tauri", "binaries");
mkdirSync(binaries, { recursive: true });
const sidecar = join(binaries, `mira-${triple}${exe}`);
copyFileSync(join(root, "src-tauri", "target", "release", `mira${exe}`), sidecar);
console.log(`staged CLI sidecar: ${sidecar}`);
