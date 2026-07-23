#!/usr/bin/env sh
# Builds the release `mira` CLI and stages it as a Tauri sidecar so it ships
# inside the app bundle (Mirador.app/Contents/MacOS/mira). Tauri requires the
# target triple suffix on sidecar binaries.
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"

cargo build --release -p cmux-cli --manifest-path "$ROOT/src-tauri/Cargo.toml"

mkdir -p "$ROOT/src-tauri/binaries"
cp "$ROOT/src-tauri/target/release/mira" "$ROOT/src-tauri/binaries/mira-$TRIPLE"
echo "staged CLI sidecar: src-tauri/binaries/mira-$TRIPLE"
