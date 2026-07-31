export const GITHUB_URL = "https://github.com/SuperHomer/mirador";

// GitHub resolves /releases/latest itself, so this page never needs syncing
// after a release. It used to link a version-pinned .dmg, which meant a
// workflow rewrote this file on every release — and that workflow could not
// deploy, because Pages only accepts deployments from main while a release
// event runs on the tag. Pointing at the release page also lets visitors
// pick their own platform now that there is more than one asset.
export const RELEASES_URL = `${GITHUB_URL}/releases/latest`;

// Direct downloads. These work forever because the release assets are
// uploaded under version-free names (see .github/workflows/build-*.yml) —
// /releases/latest/download/<name> then resolves to the newest release on
// its own. A version-stamped filename would put us back to rewriting this
// file on every release.
const LATEST_ASSET = `${GITHUB_URL}/releases/latest/download`;
export const DOWNLOAD_MACOS = `${LATEST_ASSET}/Mirador-macOS-arm64.dmg`;
export const DOWNLOAD_WINDOWS = `${LATEST_ASSET}/Mirador-Windows-x64-setup.exe`;

export const SHOW_APP_MOCKUP = true;
export const SHOW_SHORTCUTS = true;
