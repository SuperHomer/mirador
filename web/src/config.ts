export const GITHUB_URL = "https://github.com/SuperHomer/mirador";

// GitHub resolves /releases/latest itself, so this page never needs syncing
// after a release. It used to link a version-pinned .dmg, which meant a
// workflow rewrote this file on every release — and that workflow could not
// deploy, because Pages only accepts deployments from main while a release
// event runs on the tag. Pointing at the release page also lets visitors
// pick their own platform now that there is more than one asset.
export const RELEASES_URL = `${GITHUB_URL}/releases/latest`;

export const SHOW_APP_MOCKUP = true;
export const SHOW_SHORTCUTS = true;
