//! What account each harness is signed in with, read from the files it
//! keeps: how it is billed (a plan or an API key), the plan, and the
//! plan's limits where the harness writes them down.
//!
//! Claude Code keeps its account in `.claude.json` (`$CLAUDE_CONFIG_DIR`,
//! else the home directory): an `oauthAccount` is a plan's sign-in, with
//! its `organizationType` and `emailAddress`. A key in
//! `ANTHROPIC_API_KEY`, or an `apiKeyHelper` in `settings.json`, is used
//! over that sign-in, so either makes it an API key. Claude Code writes
//! its limits nowhere on disk, so it has none here.
//!
//! Codex keeps its login in `$CODEX_HOME/auth.json` (`~/.codex`):
//! `auth_mode` is `chatgpt` for a plan, `apikey` for a key, and a key
//! stored in `OPENAI_API_KEY` is one too; `tokens` without a mode is an
//! older Codex's ChatGPT sign-in. `OPENAI_API_KEY` in the environment is
//! used only when that file holds no login at all — the opposite of
//! Claude Code, whose key wins. Its limits are in its rollouts:
//! each `event_msg` `token_count` carries the `rate_limits` the server
//! last sent, a `primary` and a `secondary` window and the `plan_type`.
//! The newest rollout's last such line is the latest word.
//!
//! Only the shape of a login is read, never a credential: the key and the
//! tokens are skipped while parsing, and the environment is only asked
//! whether a key is set. Nothing here is a documented format: every field
//! is optional, and a line that does not parse is skipped.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use cmux_protocol::{AccountUsage, Harness, LimitWindow, Login};
use serde::de::IgnoredAny;
use serde::Deserialize;
use serde_json::Value;

use crate::codex_trail::newest_rollout;

/// How much of a rollout's end is read for its limits: every turn writes
/// them, so the last one is near the end, and rollouts grow to megabytes.
const ROLLOUT_TAIL: u64 = 256 * 1024;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClaudeJson {
    oauth_account: Option<OauthAccount>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OauthAccount {
    /// `claude_max`, `claude_pro`, `claude_team`…: the plan by the name
    /// people buy it under. `billingType` (`stripe_subscription`) says how
    /// it is paid, not which plan, and `userRateLimitTier` is often null.
    organization_type: Option<String>,
    email_address: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClaudeSettings {
    api_key_helper: Option<IgnoredAny>,
}

#[derive(Deserialize, Default)]
struct CodexAuth {
    auth_mode: Option<String>,
    /// Only whether it is there: `IgnoredAny` drops the key unread.
    #[serde(rename = "OPENAI_API_KEY")]
    api_key: Option<IgnoredAny>,
    tokens: Option<IgnoredAny>,
}

/// A JSON file as `T`, or its default when it is missing or does not parse.
fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Claude Code's account, from its `.claude.json` and `settings.json`,
/// `api_key_in_env` being whether `ANTHROPIC_API_KEY` is set.
pub fn claude_usage(claude_json: &Path, settings: &Path, api_key_in_env: bool) -> AccountUsage {
    let account = read_json::<ClaudeJson>(claude_json).oauth_account;
    let helper = read_json::<ClaudeSettings>(settings).api_key_helper.is_some();
    let login = if api_key_in_env || helper {
        Login::ApiKey
    } else if account.is_some() {
        Login::Subscription
    } else {
        Login::None
    };
    let (plan, email) = account.map_or((None, None), |a| (a.organization_type, a.email_address));
    AccountUsage {
        harness: Harness::Claude,
        login,
        // A plan that is not what usage is billed to would mislead.
        plan: plan.filter(|_| login == Login::Subscription),
        email,
        limits: Vec::new(),
    }
}

/// Codex's account, from `auth.json` under `codex_home` and the limits in
/// its newest rollout, `api_key_in_env` being whether `OPENAI_API_KEY` is
/// set.
pub fn codex_usage(codex_home: &Path, api_key_in_env: bool) -> AccountUsage {
    let auth = read_json::<CodexAuth>(&codex_home.join("auth.json"));
    // Codex prefers the login it stored over a key in the environment.
    let login = match auth.auth_mode.as_deref() {
        _ if auth.api_key.is_some() => Login::ApiKey,
        Some("apikey") => Login::ApiKey,
        Some("chatgpt") => Login::Subscription,
        None if auth.tokens.is_some() => Login::Subscription,
        _ if api_key_in_env => Login::ApiKey,
        _ => Login::None,
    };
    let rate_limits = newest_rollout(&codex_home.join("sessions")).and_then(|r| last_rate_limits(&r));
    let rate_limits = rate_limits.unwrap_or(Value::Null);
    let limits = ["primary", "secondary"].iter().filter_map(|w| limit_window(&rate_limits[w])).collect();
    AccountUsage {
        harness: Harness::Codex,
        login,
        plan: rate_limits["plan_type"].as_str().map(str::to_string),
        email: None,
        limits,
    }
}

/// The `rate_limits` of the last `token_count` in the end of `rollout`
/// that has any: one sent with none (a turn's first count) says nothing.
fn last_rate_limits(rollout: &Path) -> Option<Value> {
    let mut file = std::fs::File::open(rollout).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(ROLLOUT_TAIL);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    // Read from the middle, the first line is a piece of one.
    let whole = if start > 0 { bytes.iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| i + 1) } else { 0 };
    bytes[whole..].rsplit(|&b| b == b'\n').find_map(|line| {
        let entry = serde_json::from_slice::<Value>(line).ok()?;
        let p = &entry["payload"];
        (entry["type"] == "event_msg" && p["type"] == "token_count" && p["rate_limits"].is_object())
            .then(|| p["rate_limits"].clone())
    })
}

fn limit_window(w: &Value) -> Option<LimitWindow> {
    Some(LimitWindow {
        used_percent: w["used_percent"].as_f64()?,
        window_minutes: w["window_minutes"].as_u64()?,
        resets_at: w["resets_at"].as_u64(),
    })
}

/// Every harness's account, from where each keeps it on this machine.
pub fn accounts() -> Vec<AccountUsage> {
    let home = crate::config::home_dir().map(PathBuf::from).unwrap_or_default();
    let claude_config = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    let claude_json = claude_config.as_deref().unwrap_or(&home).join(".claude.json");
    let settings = claude_config.unwrap_or_else(|| home.join(".claude")).join("settings.json");
    let api_key_in_env = std::env::var_os("ANTHROPIC_API_KEY").is_some_and(|v| !v.is_empty());
    let codex_home = std::env::var_os("CODEX_HOME").map_or_else(|| home.join(".codex"), PathBuf::from);
    let openai_key_in_env = std::env::var_os("OPENAI_API_KEY").is_some_and(|v| !v.is_empty());
    vec![claude_usage(&claude_json, &settings, api_key_in_env), codex_usage(&codex_home, openai_key_in_env)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-usage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    const OAUTH: &str = r#"{"oauthAccount":{"billingType":"stripe_subscription","organizationType":"claude_max",
        "userRateLimitTier":null,"emailAddress":"dev@example.com"}}"#;

    #[test]
    fn a_claude_sign_in_is_a_subscription_with_its_plan() {
        let dir = temp_dir("claude-sub");
        let usage = claude_usage(&write(&dir, ".claude.json", OAUTH), &dir.join("settings.json"), false);
        assert_eq!(usage.login, Login::Subscription);
        assert_eq!(usage.plan.as_deref(), Some("claude_max"));
        assert_eq!(usage.email.as_deref(), Some("dev@example.com"));
        assert!(usage.limits.is_empty());
    }

    #[test]
    fn an_api_key_wins_over_a_claude_sign_in() {
        let dir = temp_dir("claude-key");
        let json = write(&dir, ".claude.json", OAUTH);
        let none = dir.join("settings.json");
        let from_env = claude_usage(&json, &none, true);
        assert_eq!(from_env.login, Login::ApiKey);
        assert_eq!(from_env.plan, None);
        let helper = write(&dir, "settings.json", r#"{"apiKeyHelper":"~/bin/key.sh"}"#);
        assert_eq!(claude_usage(&json, &helper, false).login, Login::ApiKey);
    }

    #[test]
    fn claude_signed_out_or_missing_is_no_login() {
        let dir = temp_dir("claude-none");
        let settings = dir.join("settings.json");
        let out = claude_usage(&write(&dir, ".claude.json", r#"{"hasCompletedOnboarding":true}"#), &settings, false);
        assert_eq!((out.login, out.plan, out.email), (Login::None, None, None));
        assert_eq!(claude_usage(&dir.join("missing.json"), &settings, false).login, Login::None);
        assert_eq!(claude_usage(&write(&dir, "bad.json", "{not json"), &settings, false).login, Login::None);
    }

    #[test]
    fn codex_on_chatgpt_reads_its_limits_from_the_newest_rollout() {
        let dir = temp_dir("codex-sub");
        write(&dir, "auth.json", r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"x"}}"#);
        let limits = |used: f64| {
            format!(
                r#"{{"type":"event_msg","payload":{{"type":"token_count","rate_limits":{{"primary":{{"used_percent":{used},"window_minutes":300,"resets_at":1795590902}},"secondary":null,"plan_type":"plus"}}}}}}"#
            )
        };
        let old = limits(99.0);
        write(&dir, "sessions/2026/10/31/rollout-2026-10-31T09-00-00-a.jsonl", &format!("{old}\n"));
        // Longer than the tail read, so it starts in the middle of a line.
        let rollout = [
            "x".repeat(ROLLOUT_TAIL as usize),
            limits(10.0),
            limits(42.5),
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#.into(),
            "{half a line".into(),
        ]
        .join("\n");
        write(&dir, "sessions/2026/11/02/rollout-2026-11-02T09-00-00-b.jsonl", &rollout);
        let usage = codex_usage(&dir, false);
        assert_eq!(usage.login, Login::Subscription);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        // The sign-in Codex stored wins over a key in the environment.
        assert_eq!(codex_usage(&dir, true).login, Login::Subscription);
        assert_eq!(usage.limits, vec![LimitWindow { used_percent: 42.5, window_minutes: 300, resets_at: Some(1795590902) }]);
    }

    #[test]
    fn codex_on_a_key_is_an_api_key() {
        let dir = temp_dir("codex-key");
        write(&dir, "auth.json", r#"{"auth_mode":"apikey"}"#);
        assert_eq!(codex_usage(&dir, false).login, Login::ApiKey);
        // A stored key is a key whatever the mode says.
        write(&dir, "auth.json", r#"{"OPENAI_API_KEY":"sk-test"}"#);
        let usage = codex_usage(&dir, false);
        assert_eq!((usage.login, usage.limits), (Login::ApiKey, vec![]));
    }

    #[test]
    fn codex_uses_a_key_from_the_environment_only_with_no_login_stored() {
        let dir = temp_dir("codex-env");
        assert_eq!(codex_usage(&dir, true).login, Login::ApiKey);
        write(&dir, "auth.json", r#"{"auth_mode":null,"OPENAI_API_KEY":null,"tokens":null}"#);
        assert_eq!(codex_usage(&dir, true).login, Login::ApiKey);
        // An older Codex's sign-in: tokens, no mode.
        write(&dir, "auth.json", r#"{"tokens":{"id_token":"x"}}"#);
        assert_eq!(codex_usage(&dir, true).login, Login::Subscription);
    }

    #[test]
    fn codex_with_nothing_on_disk_is_no_login() {
        let dir = temp_dir("codex-none");
        let usage = codex_usage(&dir.join("missing"), false);
        assert_eq!((usage.login, usage.plan, usage.limits), (Login::None, None, vec![]));
    }
}
