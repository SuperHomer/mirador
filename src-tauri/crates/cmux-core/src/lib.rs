/// This product's GitHub repository: where its release notes and the
/// `mira` a remote computer fetches come from. Pinned rather than derived
/// from the updater endpoint: a config-driven URL would be a way to point
/// the app at someone else's notes, or someone else's binary.
pub const REPO: &str = "SuperHomer/mirador";

pub mod agents;
pub mod board;
pub mod browser_url;
pub mod builtin;
pub mod checkpoints;
pub mod claude_integration;
pub mod codex_trail;
pub mod config;
pub mod cwd;
pub mod diff;
pub mod git;
pub mod handoff;
pub mod graph;
pub mod hold;
pub mod ipc;
pub mod layout;
pub mod ports;
pub mod session;
pub mod ssh;
pub mod notes;
pub mod osc;
pub mod pane_origin;
pub mod pricing;
pub mod proc;
pub mod pty;
pub mod role_files;
pub mod role_limits;
pub mod roles;
pub mod state;
pub mod tasks;
pub mod teams;
pub mod trail;
pub mod transcript;
pub mod transport;
pub mod usage;
pub mod wall;
pub mod worktree;
