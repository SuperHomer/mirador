//! What account each harness is signed in with, read from the files it
//! keeps: how it is billed (a plan or an API key), the plan, and the
//! plan's limits where the harness writes them down.
//!
//! Claude Code keeps its account in `.claude.json` (`$CLAUDE_CONFIG_DIR`,
//! else the home directory): an `oauthAccount` is a plan's sign-in, with
//! its `organizationType` and `emailAddress`. An `apiKeyHelper` in
//! `settings.json` makes it an API key. A key in `ANTHROPIC_API_KEY` is
//! used only if the person said yes to it: Claude Code asks once, and
//! records the answer in `.claude.json`'s `customApiKeyResponses`, each
//! list holding a suffix of a key approved or rejected. Only how many
//! entries each list has is read, never an entry. Claude Code writes its
//! limits nowhere on disk, so it has none here.
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
//! What each used over the plans' windows (five hours, seven days) is
//! added up from the same files: Claude Code's transcripts
//! (`projects/<project>/<session>.jsonl` and its subagents', which bill
//! too) by each reply's `usage` and `timestamp`, and Codex's rollouts by
//! each `token_count`'s `info.last_token_usage`. A file last written
//! before the longer window is not opened. A [`UsageCache`] keeps what
//! each file's lines counted and reads only what was appended since, so
//! reading every minute costs a directory listing and the new lines; the
//! windows are summed from those at each read.
//!
//! Only the shape of a login is read, never a credential: the key and the
//! tokens are skipped while parsing, and the environment is only asked
//! whether a key is set. Nothing here is a documented format: every field
//! is optional, and a line that does not parse is skipped.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use cmux_protocol::{AccountUsage, Harness, LimitWindow, Login, TokenTotals};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codex_trail::newest_rollout;
use crate::pricing::Usage;
use crate::transcript::{parse_timestamp, subagent_dir, usage_of, Cache, Feed};

/// How much of a rollout's end is read for its limits: every turn writes
/// them, so the last one is near the end, and rollouts grow to megabytes.
const ROLLOUT_TAIL: u64 = 256 * 1024;

const FIVE_HOURS_MS: u64 = 5 * 3600 * 1000;
const SEVEN_DAYS_MS: u64 = 7 * 24 * 3600 * 1000;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClaudeJson {
    oauth_account: Option<OauthAccount>,
    custom_api_key_responses: Option<KeyResponses>,
}

/// Claude Code's answers about a key in the environment: counted, never
/// read — `IgnoredAny` drops each entry unread.
#[derive(Deserialize, Default)]
#[serde(default)]
struct KeyResponses {
    approved: Vec<IgnoredAny>,
    rejected: Vec<IgnoredAny>,
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

/// Claude Code's account, from its `.claude.json`, and its settings and
/// transcripts under `claude_dir`, `api_key_in_env` being whether
/// `ANTHROPIC_API_KEY` is set and `now_ms` the time the windows end.
/// The plan's windows come from the status line's snapshot at `limits`.
pub fn claude_usage(claude_json: &Path, claude_dir: &Path, api_key_in_env: bool, now_ms: u64, limits: &Path) -> AccountUsage {
    UsageCache::default().claude(claude_json, claude_dir, api_key_in_env, now_ms, limits)
}

/// Codex's account, from `auth.json` under `codex_home` and the limits in
/// its newest rollout, `api_key_in_env` being whether `OPENAI_API_KEY` is
/// set and `now_ms` the time the windows end.
pub fn codex_usage(codex_home: &Path, api_key_in_env: bool, now_ms: u64) -> AccountUsage {
    UsageCache::default().codex(codex_home, api_key_in_env, now_ms)
}

/// What every recent transcript and rollout counted, kept between reads:
/// a file is read again only from where the last read stopped, or from
/// its start once it shrank.
#[derive(Default)]
pub struct UsageCache {
    claude: Cache<Replies>,
    codex: Cache<Calls>,
}

impl UsageCache {
    /// Every harness's account, from where each keeps it on this machine.
    pub fn accounts(&mut self) -> Vec<AccountUsage> {
        let home = crate::config::home_dir().map(PathBuf::from).unwrap_or_default();
        let claude_config = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
        let claude_json = claude_config.as_deref().unwrap_or(&home).join(".claude.json");
        let claude_dir = claude_config.unwrap_or_else(|| home.join(".claude"));
        // The key Claude Code sees is its pane's: the interactive shell's
        // environment, which a Dock-launched app does not inherit, or the
        // app's own, which its panes do. On Windows the shell's is never
        // known (no `-ilc` lookup), so the app's own decides.
        let api_key_in_env = crate::pty::shell::interactive_anthropic_key() == Some(true)
            || std::env::var_os("ANTHROPIC_API_KEY").is_some_and(|v| !v.is_empty());
        let codex_home = std::env::var_os("CODEX_HOME").map_or_else(|| home.join(".codex"), PathBuf::from);
        let openai_key_in_env = std::env::var_os("OPENAI_API_KEY").is_some_and(|v| !v.is_empty());
        let now_ms = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        vec![
            self.claude(&claude_json, &claude_dir, api_key_in_env, now_ms, &claude_limits_path()),
            self.codex(&codex_home, openai_key_in_env, now_ms),
        ]
    }

    /// [`claude_usage`], reading only what changed since the last call.
    pub fn claude(
        &mut self,
        claude_json: &Path,
        claude_dir: &Path,
        api_key_in_env: bool,
        now_ms: u64,
        limits: &Path,
    ) -> AccountUsage {
        let json = read_json::<ClaudeJson>(claude_json);
        let account = json.oauth_account;
        let helper = read_json::<ClaudeSettings>(&claude_dir.join("settings.json")).api_key_helper.is_some();
        let answers = json.custom_api_key_responses.unwrap_or_default();
        let signed_in = if account.is_some() { Login::Subscription } else { Login::None };
        let mut note = None;
        let login = match (helper, api_key_in_env, !answers.approved.is_empty(), !answers.rejected.is_empty()) {
            // A helper's key is used with no question asked.
            (true, ..) => Login::ApiKey,
            (false, false, ..) => signed_in,
            (false, true, true, false) => Login::ApiKey,
            (false, true, false, true) => {
                note = Some("API key in env, declined".to_string());
                signed_in
            }
            // One key approved and another rejected: which one is in the
            // environment is not something read here.
            (false, true, true, true) if account.is_some() => Login::PlanOrApiKey,
            (false, true, true, true) => Login::ApiKey,
            // Never asked: Claude Code will offer the key, and most say
            // yes. A guess at the next answer.
            (false, true, false, false) => Login::ApiKey,
        };
        let (plan, email) = account.map_or((None, None), |a| (a.organization_type, a.email_address));
        // On a plan the list price is not what is paid: no cost, not a cost
        // that reads as a bill.
        let mut totals = Totals::new(now_ms, login == Login::ApiKey);
        let transcripts = claude_transcripts(&claude_dir.join("projects"), now_ms);
        let mut refused: Option<Rejection> = None;
        for transcript in &transcripts {
            let Some(replies) = self.claude.read(transcript) else { continue };
            for c in &replies.counted {
                totals.add(c.at_ms, c.usage, c.model.as_deref());
            }
            if let Some(r) = replies.rejected.filter(|r| refused.is_none_or(|o| r.at_ms > o.at_ms)) {
                refused = Some(r);
            }
        }
        // A file last written before the window counts nothing any more.
        self.claude.retain(|p| transcripts.iter().any(|t| t == p));
        // The windows are the plan's: an API key has none.
        let on_plan = matches!(login, Login::Subscription | Login::PlanOrApiKey);
        let (limits, as_of) =
            if on_plan { plan_limits(read_claude_limits(limits), refused, now_ms) } else { (Vec::new(), None) };
        let limits_note = (on_plan && limits.is_empty()).then(|| no_limits_note(claude_dir));
        AccountUsage {
            harness: Harness::Claude,
            login,
            // A plan that is not what usage is billed to would mislead.
            plan: plan.filter(|_| matches!(login, Login::Subscription | Login::PlanOrApiKey)),
            email,
            note,
            limits,
            as_of,
            limits_note,
            in_use: in_use(login, &totals),
            five_hours: totals.five_hours,
            seven_days: totals.seven_days,
        }
    }

    /// [`codex_usage`], reading only what changed since the last call.
    pub fn codex(&mut self, codex_home: &Path, api_key_in_env: bool, now_ms: u64) -> AccountUsage {
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
        let last = newest_rollout(&codex_home.join("sessions")).and_then(|r| last_rate_limits(&r));
        let (rate_limits, as_of) = last.unwrap_or((Value::Null, None));
        let mut limits: Vec<LimitWindow> =
            ["primary", "secondary"].iter().filter_map(|w| limit_window(&rate_limits[w])).collect();
        // The most used first: the one a glance at the account should see.
        limits.sort_by(|a, b| b.used_percent.total_cmp(&a.used_percent));
        // `pricing` has no OpenAI rates, so never a cost.
        let mut totals = Totals::new(now_ms, false);
        let mut rollouts = Vec::new();
        recent_files(&codex_home.join("sessions"), now_ms, 3, &mut rollouts);
        for rollout in &rollouts {
            for c in self.codex.read(rollout).map_or(&[][..], |r| &r.counted) {
                totals.add(c.at_ms, c.usage, None);
            }
        }
        self.codex.retain(|p| rollouts.iter().any(|r| r == p));
        AccountUsage {
            harness: Harness::Codex,
            login,
            plan: rate_limits["plan_type"].as_str().map(str::to_string),
            email: None,
            note: None,
            limits,
            as_of,
            limits_note: None,
            in_use: in_use(login, &totals),
            five_hours: totals.five_hours,
            seven_days: totals.seven_days,
        }
    }
}

/// Claude's plan windows: the status line's snapshot, and a refused
/// request where the snapshot says nothing newer about its window; when
/// they were read, the newest of the two used. Most used first.
fn plan_limits(snapshot: Option<ClaudeLimits>, refused: Option<Rejection>, now_ms: u64) -> (Vec<LimitWindow>, Option<u64>) {
    let now_s = now_ms / 1000;
    let (mut limits, mut as_of) = snapshot.map_or((Vec::new(), None), |s| (s.windows, Some(s.saved_at)));
    for w in &mut limits {
        // A window that has reset since the snapshot is empty now: its old
        // percentage, shown as current, would say the plan is still that
        // full until the next status line.
        if w.resets_at.is_some_and(|at| at <= now_s) {
            w.used_percent = 0.0;
        }
    }
    if let Some(r) = refused.filter(|r| r.resets_at > now_s) {
        let known = limits.iter().any(|w| w.window_minutes == r.window_minutes);
        if !known || as_of.is_none_or(|t| r.at_ms > t) {
            limits.retain(|w| w.window_minutes != r.window_minutes);
            limits.push(LimitWindow {
                used_percent: 100.0,
                window_minutes: r.window_minutes,
                name: window_name(r.window_minutes),
                resets_at: Some(r.resets_at),
                limit_reached: true,
            });
            as_of = Some(as_of.map_or(r.at_ms, |t| t.max(r.at_ms)));
        }
    }
    limits.sort_by(|a, b| b.used_percent.total_cmp(&a.used_percent));
    (limits, as_of)
}

/// Why a Claude plan shows no limits, and what gets them: the status line
/// wrapper, or, once it is there, Claude Code's next reply.
fn no_limits_note(claude_dir: &Path) -> String {
    if cfg!(windows) {
        "Claude Code's plan limits are not read on Windows.".into()
    } else if crate::claude_integration::status_line_installed(&claude_dir.join("settings.json")) {
        "Limits appear after Claude Code's next reply.".into()
    } else {
        "Run mira hooks setup (or Set up in the banner) to read your plan's limits from Claude Code's status line.".into()
    }
}

/// Whether an account is worth showing at all: signed in, or with tokens
/// in either window — a key since removed still used them.
fn in_use(login: Login, totals: &Totals) -> bool {
    let tokens = |t: &TokenTotals| t.input + t.cache_write + t.cache_read + t.output;
    login != Login::None || tokens(&totals.five_hours) + tokens(&totals.seven_days) > 0
}

/// Every session transcript under `projects`, and every one of their
/// subagents', last written within seven days of `now_ms`.
fn claude_transcripts(projects: &Path, now_ms: u64) -> Vec<PathBuf> {
    let mut sessions = Vec::new();
    for project in std::fs::read_dir(projects).into_iter().flatten().flatten() {
        recent_files(&project.path(), now_ms, 0, &mut sessions);
    }
    let mut subagents = Vec::new();
    for session in &sessions {
        recent_files(&subagent_dir(session), now_ms, 0, &mut subagents);
    }
    sessions.extend(subagents);
    sessions
}

/// The `.jsonl` files in `dir`, and in its directories `depth` levels
/// down, last written within seven days of `now_ms`.
fn recent_files(dir: &Path, now_ms: u64, depth: u32, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 {
                recent_files(&path, now_ms, depth - 1, found);
            }
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
            if modified.is_some_and(|m| m.as_millis() as u64 + SEVEN_DAYS_MS >= now_ms) {
                found.push(path);
            }
        }
    }
}

/// Both windows' totals, added up one usage at a time.
struct Totals {
    now_ms: u64,
    five_hours: TokenTotals,
    seven_days: TokenTotals,
}

impl Totals {
    fn new(now_ms: u64, priced: bool) -> Self {
        let empty = TokenTotals { cost: priced.then_some(0.0), ..Default::default() };
        Self { now_ms, five_hours: empty.clone(), seven_days: empty }
    }

    /// `usage` at `at_ms`, costing what `model`'s rates say when known.
    fn add(&mut self, at_ms: u64, usage: Usage, model: Option<&str>) {
        let age = self.now_ms.saturating_sub(at_ms);
        let cost = model.and_then(crate::pricing::rates).map_or(0.0, |rates| crate::pricing::cost(rates, usage));
        for (window, totals) in [(FIVE_HOURS_MS, &mut self.five_hours), (SEVEN_DAYS_MS, &mut self.seven_days)] {
            if age <= window {
                totals.input += usage.input;
                totals.cache_write += usage.cache_write;
                totals.cache_read += usage.cache_read;
                totals.output += usage.output;
                if let Some(total) = &mut totals.cost {
                    *total += cost;
                }
            }
        }
    }
}

/// A line's share of the totals: what it used, and when, and on which
/// model, which prices it.
struct Counted {
    at_ms: u64,
    model: Option<String>,
    usage: Usage,
}

/// A Claude Code transcript's replies. One reply is written as several
/// entries in a row, each repeating its usage, so as in
/// `transcript::Stats` an entry with the last one's message id replaces
/// it rather than adding to it — across reads too, since the last id is
/// kept with the replies.
#[derive(Default)]
struct Replies {
    counted: Vec<Counted>,
    last_id: String,
    /// The newest request the plan refused, if any.
    rejected: Option<Rejection>,
}

/// A request Claude's plan refused, as Claude Code logs it with its error:
/// the window that is full, and when it resets (Unix seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rejection {
    at_ms: u64,
    window_minutes: u64,
    resets_at: u64,
}

/// The refusal an entry carries: `apiErrorParams.rate_limit_info`, or a
/// `quotaLimits` beside it, with `status` "rejected", `rateLimitType`
/// five_hour or seven_day, and `resetsAt`. Not seen in a transcript here;
/// every field optional, as everywhere in this file.
fn rejection(entry: &Value, at_ms: u64) -> Option<Rejection> {
    let params = &entry["apiErrorParams"];
    let info = [&params["rate_limit_info"], &params["quotaLimits"], &entry["quotaLimits"]]
        .into_iter()
        .find(|v| v.is_object())?;
    if info["status"] != "rejected" {
        return None;
    }
    let window_minutes = match info["rateLimitType"].as_str()? {
        "five_hour" => 300,
        "seven_day" => 10080,
        _ => return None,
    };
    Some(Rejection { at_ms, window_minutes, resets_at: epoch_secs(&info["resetsAt"])? })
}

/// A time as Unix seconds: given in seconds, in millis, or as RFC 3339.
fn epoch_secs(v: &Value) -> Option<u64> {
    v.as_u64()
        .map(|n| if n > 1_000_000_000_000 { n / 1000 } else { n })
        .or_else(|| v.as_str().and_then(parse_timestamp).map(|ms| ms / 1000))
}

impl Feed for Replies {
    fn feed(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return };
        let message = &entry["message"];
        // Entries Claude Code adds for itself, left out as Stats does.
        let meta = entry["isMeta"].as_bool() == Some(true);
        if meta || entry["type"] != "assistant" {
            return;
        }
        let Some(at_ms) = entry["timestamp"].as_str().and_then(parse_timestamp) else { return };
        if let Some(r) = rejection(&entry, at_ms).filter(|r| self.rejected.is_none_or(|o| r.at_ms >= o.at_ms)) {
            self.rejected = Some(r);
        }
        if !message["usage"].is_object() {
            return;
        }
        let id = message["id"].as_str().unwrap_or("").to_string();
        if !id.is_empty() && id == self.last_id {
            self.counted.pop();
        }
        let model = message["model"].as_str().map(str::to_string);
        self.counted.push(Counted { at_ms, model, usage: usage_of(&message["usage"]) });
        self.last_id = id;
    }
}

/// A Codex rollout's model calls. Its `input_tokens` include the cached
/// ones, which are split out to be counted as cache reads.
#[derive(Default)]
struct Calls {
    counted: Vec<Counted>,
}

impl Feed for Calls {
    fn feed(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return };
        let p = &entry["payload"];
        let last = &p["info"]["last_token_usage"];
        if entry["type"] != "event_msg" || p["type"] != "token_count" || !last.is_object() {
            return;
        }
        let Some(at_ms) = entry["timestamp"].as_str().and_then(parse_timestamp) else { return };
        let n = |key: &str| last[key].as_u64().unwrap_or(0);
        let usage = Usage {
            input: n("input_tokens").saturating_sub(n("cached_input_tokens")),
            cache_write: n("cache_write_input_tokens"),
            cache_read: n("cached_input_tokens"),
            output: n("output_tokens"),
        };
        self.counted.push(Counted { at_ms, model: None, usage });
    }
}

/// The `rate_limits` of the last `token_count` in the end of `rollout`
/// that has any (one sent with none, a turn's first count, says nothing),
/// and that line's time.
fn last_rate_limits(rollout: &Path) -> Option<(Value, Option<u64>)> {
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
            .then(|| (p["rate_limits"].clone(), entry["timestamp"].as_str().and_then(parse_timestamp)))
    })
}

fn limit_window(w: &Value) -> Option<LimitWindow> {
    let window_minutes = w["window_minutes"].as_u64()?;
    Some(LimitWindow {
        used_percent: w["used_percent"].as_f64()?,
        window_minutes,
        name: window_name(window_minutes),
        resets_at: w["resets_at"].as_u64(),
        limit_reached: false,
    })
}

/// `ms` (Unix millis) as a time of day on this machine's clock: `14:52`.
/// Windows has no `localtime_r` here, so it says the time in UTC.
pub fn local_clock(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    #[cfg(unix)]
    {
        let t = secs as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: both pointers are to locals that outlive the call.
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        }
    }
    let day = secs.rem_euclid(86_400);
    format!("{:02}:{:02} UTC", day / 3600, day % 3600 / 60)
}

/// A limit window by its length, as the plans name theirs: `5h`,
/// `weekly`, `30 days`, else its minutes.
pub fn window_name(minutes: u64) -> String {
    match minutes {
        300 => "5h".to_string(),
        10080 => "weekly".to_string(),
        43200 => "30 days".to_string(),
        m => format!("{m}m"),
    }
}

/// Claude Code's plan windows, as its status line last saw them.
///
/// Claude Code writes its limits nowhere on disk, but since 2.1.80 it
/// pipes them to the `statusLine` command, as `rate_limits.five_hour` and
/// `rate_limits.seven_day`, each `{used_percentage, resets_at}` — on a
/// Pro, Max or Ultra plan, after the first response, either window
/// possibly absent. `mira claude-statusline`, put in front of the user's
/// own status line, keeps the last of them in this snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeLimits {
    pub windows: Vec<LimitWindow>,
    /// Unix millis of the status line that carried them.
    pub saved_at: u64,
}

/// Where the snapshot is kept.
pub fn claude_limits_path() -> PathBuf {
    crate::session::data_dir().join("claude-limits.json")
}

/// The plan windows in a status line's JSON, five hours first; none for
/// input that does not parse or carries no `rate_limits`. `resets_at` is
/// read as Unix seconds, or as an RFC 3339 time.
pub fn statusline_windows(input: &[u8]) -> Vec<LimitWindow> {
    let Ok(json) = serde_json::from_slice::<Value>(input) else { return Vec::new() };
    let limits = &json["rate_limits"];
    [("five_hour", 300), ("seven_day", 10080)]
        .into_iter()
        .filter_map(|(key, window_minutes)| {
            let w = &limits[key];
            Some(LimitWindow {
                used_percent: w["used_percentage"].as_f64()?,
                window_minutes,
                name: window_name(window_minutes),
                resets_at: epoch_secs(&w["resets_at"]),
                limit_reached: false,
            })
        })
        .collect()
}

/// Keeps the windows in `input` as the snapshot at `path`, when it has
/// any; otherwise the snapshot there stays as it was. Every Claude Code
/// session's status line writes here, and the last write wins: they all
/// see the same account, so the newest is the truest. Written to a file
/// of this process's own and renamed over the snapshot, so a reader never
/// sees half of one and two sessions writing at once never share a file.
pub fn save_claude_limits(path: &Path, input: &[u8], now_ms: u64) -> std::io::Result<bool> {
    let windows = statusline_windows(input);
    if windows.is_empty() {
        return Ok(false);
    }
    let bytes = serde_json::to_vec(&ClaudeLimits { windows, saved_at: now_ms })?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// The snapshot at `path`, if there is one that parses.
pub fn read_claude_limits(path: &Path) -> Option<ClaudeLimits> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A snapshot path with nothing there: the tests never read the real one.
    fn no_snapshot() -> PathBuf {
        std::env::temp_dir().join(format!("mira-usage-{}-none/claude-limits.json", std::process::id()))
    }

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

    /// When the tests' windows end: 2026-11-02T12:00:00Z.
    fn now() -> u64 {
        parse_timestamp("2026-11-02T12:00:00.000Z").unwrap()
    }

    /// `write`, last modified at `ms`: the windows go by the file's time,
    /// not the clock the test runs at.
    fn write_at(dir: &Path, rel: &str, text: &str, ms: u64) -> PathBuf {
        let path = write(dir, rel, text);
        let at = UNIX_EPOCH + std::time::Duration::from_millis(ms);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
        path
    }

    const DAY_MS: u64 = 24 * 3600 * 1000;

    fn reply(at: &str, id: &str, model: &str, input: u64, output: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": at,
            "message": { "id": id, "model": model, "usage": { "input_tokens": input, "output_tokens": output } },
        })
        .to_string()
    }

    /// A session an hour ago (one reply written as two entries) and two
    /// days ago (a model with no price), an entry Claude Code added for
    /// itself (`isMeta`), a reply too old for either window,
    /// a subagent's reply, and a file last written eight days ago.
    fn claude_projects(dir: &Path) {
        let session = [
            reply("2026-11-02T11:00:00.000Z", "msg_a", "claude-opus-5-5", 100, 1),
            reply("2026-11-02T11:00:01.000Z", "msg_a", "claude-opus-5-5", 100, 10),
            reply("2026-10-31T12:00:00.000Z", "msg_b", "claude-unknown-9", 1000, 0),
            reply("2026-10-25T12:00:00.000Z", "msg_c", "claude-opus-5-5", 9999, 0),
            r#"{"type":"user","timestamp":"2026-11-02T11:00:02.000Z","message":{"content":"hi"}}"#.into(),
            r#"{"type":"assistant","isMeta":true,"timestamp":"2026-11-02T11:00:03.000Z","message":{"id":"msg_m","model":"claude-opus-5-5","usage":{"input_tokens":3333}}}"#.into(),
        ]
        .join("\n");
        write_at(dir, "projects/-repo/s1.jsonl", &session, now());
        let subagent = reply("2026-11-02T11:30:00.000Z", "msg_d", "claude-opus-5-5", 5, 0);
        // Every line ends: a line still being written is not counted yet.
        write_at(dir, "projects/-repo/s1/subagents/agent-x.jsonl", &format!("{subagent}\n"), now());
        let old = reply("2026-11-02T11:00:00.000Z", "msg_e", "claude-opus-5-5", 7777, 0);
        write_at(dir, "projects/-repo/old.jsonl", &old, now() - 8 * DAY_MS);
    }

    #[test]
    fn claude_totals_count_each_reply_once_by_its_own_time() {
        let dir = temp_dir("claude-totals");
        claude_projects(&dir);
        let usage = claude_usage(&write(&dir, ".claude.json", OAUTH), &dir, false, now(), &no_snapshot());
        let (five, seven) = (&usage.five_hours, &usage.seven_days);
        // The repeated reply at its last entry's usage, and the subagent's.
        assert_eq!((five.input, five.output), (105, 10));
        // Plus the reply two days ago; not the one eight days ago, nor the
        // old file's.
        assert_eq!((seven.input, seven.output), (1105, 10));
        // On a plan, no dollars.
        assert_eq!((five.cost, seven.cost), (None, None));
    }

    #[test]
    fn claude_totals_on_an_api_key_cost_the_priced_replies() {
        let dir = temp_dir("claude-cost");
        claude_projects(&dir);
        let usage = claude_usage(&write(&dir, ".claude.json", OAUTH), &dir, true, now(), &no_snapshot());
        let opus = crate::pricing::rates("claude-opus-5-5").unwrap();
        let priced = crate::pricing::cost(opus, Usage { input: 105, output: 10, ..Default::default() });
        // The unknown model's tokens are counted, with no cost.
        assert_eq!(usage.seven_days.input, 1105);
        for cost in [usage.five_hours.cost, usage.seven_days.cost] {
            assert!((cost.unwrap() - priced).abs() < 1e-12, "{cost:?}");
        }
    }

    #[test]
    fn codex_totals_add_up_each_model_call() {
        let dir = temp_dir("codex-totals");
        let count = |at: &str, input: u64, cached: u64, output: u64| {
            serde_json::json!({
                "timestamp": at,
                "type": "event_msg",
                "payload": { "type": "token_count", "info": { "last_token_usage": {
                    "input_tokens": input, "cached_input_tokens": cached, "output_tokens": output,
                } } },
            })
            .to_string()
        };
        let rollout = [
            count("2026-11-02T11:00:00.000Z", 1000, 600, 50),
            r#"{"timestamp":"2026-11-02T11:00:01.000Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#.into(),
            count("2026-11-02T06:00:00.000Z", 200, 0, 20),
            count("2026-10-25T12:00:00.000Z", 9999, 0, 0),
        ]
        .join("\n");
        write_at(&dir, "sessions/2026/11/02/rollout-2026-11-02T06-00-00-a.jsonl", &rollout, now());
        let old = count("2026-11-02T11:00:00.000Z", 7777, 0, 0);
        write_at(&dir, "sessions/2026/10/20/rollout-2026-10-20T06-00-00-b.jsonl", &old, now() - 8 * DAY_MS);
        let usage = codex_usage(&dir, false, now());
        let five = &usage.five_hours;
        assert_eq!((five.input, five.cache_read, five.output, five.cost), (400, 600, 50, None));
        let seven = &usage.seven_days;
        assert_eq!((seven.input, seven.cache_read, seven.output, seven.cost), (600, 600, 70, None));
    }

    const OAUTH: &str = r#"{"oauthAccount":{"billingType":"stripe_subscription","organizationType":"claude_max",
        "userRateLimitTier":null,"emailAddress":"dev@example.com"}}"#;

    /// One session's five-hour input, read through `cache`.
    fn input_5h(cache: &mut UsageCache, dir: &Path, now_ms: u64) -> (u64, u64) {
        let usage = cache.claude(&dir.join(".claude.json"), dir, false, now_ms, &no_snapshot());
        (usage.five_hours.input, usage.seven_days.input)
    }

    #[test]
    fn a_second_read_counts_only_what_was_appended() {
        let dir = temp_dir("cache-append");
        let path = dir.join("projects/-repo/s1.jsonl");
        let first = reply("2026-11-02T11:00:00.000Z", "msg_a", "claude-opus-5-5", 100, 0);
        write_at(&dir, "projects/-repo/s1.jsonl", &format!("{first}\n"), now());
        let mut cache = UsageCache::default();
        assert_eq!(input_5h(&mut cache, &dir, now()), (100, 100));
        let read = cache.claude.consumed(&path).unwrap();

        // A new reply, and another entry of it repeating its usage: the
        // repeat replaces it, though the two came in different reads.
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", reply("2026-11-02T11:30:00.000Z", "msg_b", "claude-opus-5-5", 20, 0)).unwrap();
        assert_eq!(input_5h(&mut cache, &dir, now()), (120, 120));
        writeln!(file, "{}", reply("2026-11-02T11:30:01.000Z", "msg_b", "claude-opus-5-5", 25, 0)).unwrap();
        assert_eq!(input_5h(&mut cache, &dir, now()), (125, 125));
        // Read on from where it stopped, not from the start.
        assert!(cache.claude.consumed(&path).unwrap() > read);
    }

    #[test]
    fn a_rewritten_transcript_is_counted_again() {
        let dir = temp_dir("cache-shrunk");
        let two = [
            reply("2026-11-02T11:00:00.000Z", "msg_a", "claude-opus-5-5", 100, 0),
            reply("2026-11-02T11:10:00.000Z", "msg_b", "claude-opus-5-5", 200, 0),
        ]
        .join("\n");
        write_at(&dir, "projects/-repo/s1.jsonl", &format!("{two}\n"), now());
        let mut cache = UsageCache::default();
        assert_eq!(input_5h(&mut cache, &dir, now()), (300, 300));
        let one = reply("2026-11-02T11:20:00.000Z", "msg_c", "claude-opus-5-5", 7, 0);
        write_at(&dir, "projects/-repo/s1.jsonl", &format!("{one}\n"), now());
        assert_eq!(input_5h(&mut cache, &dir, now()), (7, 7));
    }

    #[test]
    fn a_reply_leaves_the_windows_as_time_passes_without_a_new_read() {
        let dir = temp_dir("cache-ages");
        let lines = [
            reply("2026-10-26T15:00:00.000Z", "msg_a", "claude-opus-5-5", 1000, 0),
            reply("2026-11-02T11:00:00.000Z", "msg_b", "claude-opus-5-5", 10, 0),
        ]
        .join("\n");
        let path = write_at(&dir, "projects/-repo/s1.jsonl", &format!("{lines}\n"), now());
        let mut cache = UsageCache::default();
        assert_eq!(input_5h(&mut cache, &dir, now()), (10, 1010));
        let read = cache.claude.consumed(&path);
        // Six hours on: the newer reply is out of five hours, the older
        // one out of seven days, and nothing more was read.
        assert_eq!(input_5h(&mut cache, &dir, now() + 6 * 3600 * 1000), (0, 10));
        assert_eq!(cache.claude.consumed(&path), read);
        // Eight days on, the file itself is too old and is forgotten.
        assert_eq!(input_5h(&mut cache, &dir, now() + 8 * DAY_MS), (0, 0));
        assert_eq!(cache.claude.consumed(&path), None);
    }

    #[test]
    fn a_claude_sign_in_is_a_subscription_with_its_plan() {
        let dir = temp_dir("claude-sub");
        let usage = claude_usage(&write(&dir, ".claude.json", OAUTH), &dir, false, now(), &no_snapshot());
        assert_eq!(usage.login, Login::Subscription);
        assert_eq!(usage.plan.as_deref(), Some("claude_max"));
        assert_eq!(usage.email.as_deref(), Some("dev@example.com"));
        assert!(usage.limits.is_empty());
    }

    /// The sign-in, with Claude Code's answers about an environment key.
    fn oauth_with(approved: &str, rejected: &str) -> String {
        format!(
            r#"{{"oauthAccount":{{"organizationType":"claude_max"}},"customApiKeyResponses":{{"approved":[{approved}],"rejected":[{rejected}]}}}}"#
        )
    }

    #[test]
    fn an_approved_env_key_wins_over_a_claude_sign_in() {
        let dir = temp_dir("claude-key");
        let json = write(&dir, ".claude.json", &oauth_with(r#""abcdefghij0123456789""#, ""));
        let usage = claude_usage(&json, &dir, true, now(), &no_snapshot());
        assert_eq!((usage.login, usage.plan, usage.note), (Login::ApiKey, None, None));
        // Without the key in the environment, the approval is moot.
        assert_eq!(claude_usage(&json, &dir, false, now(), &no_snapshot()).login, Login::Subscription);
    }

    #[test]
    fn a_declined_env_key_leaves_the_plan_with_a_note() {
        let dir = temp_dir("claude-declined");
        let json = write(&dir, ".claude.json", &oauth_with("", r#""abcdefghij0123456789""#));
        let usage = claude_usage(&json, &dir, true, now(), &no_snapshot());
        assert_eq!(usage.login, Login::Subscription);
        assert_eq!(usage.plan.as_deref(), Some("claude_max"));
        assert_eq!(usage.note.as_deref(), Some("API key in env, declined"));
        // No cost: the plan is what is billed.
        assert_eq!(usage.five_hours.cost, None);
        // Declined with no sign-in: nothing is billed at all.
        let alone = write(&dir, "alone.json", r#"{"customApiKeyResponses":{"approved":[],"rejected":["x"]}}"#);
        assert_eq!(claude_usage(&alone, &dir, true, now(), &no_snapshot()).login, Login::None);
    }

    #[test]
    fn a_key_both_approved_and_rejected_is_plan_or_key() {
        let dir = temp_dir("claude-both");
        let json = write(&dir, ".claude.json", &oauth_with(r#""a""#, r#""b""#));
        let usage = claude_usage(&json, &dir, true, now(), &no_snapshot());
        assert_eq!((usage.login, usage.plan.as_deref()), (Login::PlanOrApiKey, Some("claude_max")));
        assert_eq!(usage.five_hours.cost, None);
    }

    #[test]
    fn a_key_never_asked_about_is_taken_as_a_key() {
        let dir = temp_dir("claude-unasked");
        let json = write(&dir, ".claude.json", OAUTH);
        assert_eq!(claude_usage(&json, &dir, true, now(), &no_snapshot()).login, Login::ApiKey);
        let empty = write(&dir, "empty.json", &oauth_with("", ""));
        assert_eq!(claude_usage(&empty, &dir, true, now(), &no_snapshot()).login, Login::ApiKey);
    }

    #[test]
    fn a_key_helper_is_a_key_with_no_question_asked() {
        let dir = temp_dir("claude-helper");
        let json = write(&dir, ".claude.json", &oauth_with("", r#""b""#));
        write(&dir, "settings.json", r#"{"apiKeyHelper":"~/bin/key.sh"}"#);
        assert_eq!(claude_usage(&json, &dir, false, now(), &no_snapshot()).login, Login::ApiKey);
        assert_eq!(claude_usage(&json, &dir, true, now(), &no_snapshot()).login, Login::ApiKey);
    }

    #[test]
    fn claude_signed_out_or_missing_is_no_login() {
        let dir = temp_dir("claude-none");
        let out = claude_usage(&write(&dir, ".claude.json", r#"{"hasCompletedOnboarding":true}"#), &dir, false, now(), &no_snapshot());
        assert_eq!((out.login, out.plan, out.email), (Login::None, None, None));
        assert_eq!(claude_usage(&dir.join("missing.json"), &dir, false, now(), &no_snapshot()).login, Login::None);
        assert_eq!(claude_usage(&write(&dir, "bad.json", "{not json"), &dir, false, now(), &no_snapshot()).login, Login::None);
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
        let usage = codex_usage(&dir, false, now());
        assert_eq!(usage.login, Login::Subscription);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        // The sign-in Codex stored wins over a key in the environment.
        assert_eq!(codex_usage(&dir, true, now()).login, Login::Subscription);
        let five_hours = LimitWindow { used_percent: 42.5, window_minutes: 300, name: "5h".into(), resets_at: Some(1795590902), limit_reached: false };
        assert_eq!(usage.limits, vec![five_hours]);
    }

    #[test]
    fn an_account_is_in_use_when_signed_in_or_used() {
        let dir = temp_dir("in-use");
        let none = dir.join("missing.json");
        // Signed out, nothing used: a harness never used here.
        assert!(!claude_usage(&none, &dir, false, now(), &no_snapshot()).in_use);
        // Signed in, nothing used yet.
        assert!(claude_usage(&write(&dir, ".claude.json", OAUTH), &dir, false, now(), &no_snapshot()).in_use);
        // Signed out, but used lately: a key since removed.
        let line = reply("2026-11-02T11:00:00.000Z", "msg_a", "claude-opus-5-5", 10, 0);
        write_at(&dir, "projects/-repo/s1.jsonl", &format!("{line}\n"), now());
        assert!(claude_usage(&none, &dir, false, now(), &no_snapshot()).in_use);
    }

    #[test]
    fn the_most_used_window_comes_first() {
        let dir = temp_dir("codex-worst");
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":12.0,"window_minutes":300},"secondary":{"used_percent":64.0,"window_minutes":10080}}}}"#;
        write(&dir, "sessions/2026/11/02/rollout-2026-11-02T09-00-00-a.jsonl", &format!("{line}\n"));
        let names: Vec<String> = codex_usage(&dir, false, now()).limits.into_iter().map(|w| w.name).collect();
        assert_eq!(names, ["weekly", "5h"]);
    }

    const STATUSLINE: &str = r#"{"model":{"id":"claude-opus-5-5"},"rate_limits":{
        "five_hour":{"used_percentage":23.5,"resets_at":1795590902},
        "seven_day":{"used_percentage":71,"resets_at":"2026-11-05T08:00:00.000Z"}}}"#;

    #[test]
    fn a_status_line_gives_both_windows() {
        let windows = statusline_windows(STATUSLINE.as_bytes());
        let seven_day_reset = parse_timestamp("2026-11-05T08:00:00.000Z").unwrap() / 1000;
        assert_eq!(
            windows,
            [
                LimitWindow { used_percent: 23.5, window_minutes: 300, name: "5h".into(), resets_at: Some(1795590902), limit_reached: false },
                LimitWindow { used_percent: 71.0, window_minutes: 10080, name: "weekly".into(), resets_at: Some(seven_day_reset), limit_reached: false },
            ]
        );
    }

    #[test]
    fn a_window_may_be_absent() {
        let only_week = r#"{"rate_limits":{"seven_day":{"used_percentage":12}}}"#;
        let windows = statusline_windows(only_week.as_bytes());
        assert_eq!(windows.len(), 1);
        assert_eq!((windows[0].window_minutes, windows[0].resets_at), (10080, None));
    }

    #[test]
    fn a_status_line_without_limits_keeps_the_snapshot() {
        let dir = temp_dir("statusline-keep");
        let path = dir.join("claude-limits.json");
        assert!(save_claude_limits(&path, STATUSLINE.as_bytes(), 1).unwrap());
        let saved = read_claude_limits(&path).unwrap();
        assert_eq!((saved.windows.len(), saved.saved_at), (2, 1));
        // An API-key session, or before the first response: no rate_limits.
        assert!(!save_claude_limits(&path, br#"{"model":{"id":"claude-opus-5-5"}}"#, 2).unwrap());
        // Not JSON at all.
        assert!(!save_claude_limits(&path, b"{not json", 3).unwrap());
        assert_eq!(read_claude_limits(&path), Some(saved));
        // Nothing left beside it.
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["claude-limits.json"]);
    }

    /// A refusal the way the fallback reads one: an assistant entry at
    /// `at`, its `apiErrorParams.rate_limit_info` rejected for `window`.
    fn refusal(at: &str, window: &str, resets_at: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": at,
            "isApiErrorMessage": true,
            "apiErrorParams": { "rate_limit_info": { "status": "rejected", "rateLimitType": window, "resetsAt": resets_at } },
            "message": { "id": "err", "content": [] },
        })
        .to_string()
    }

    /// Signed in on a plan, with `snapshot` (status line JSON) saved at
    /// `saved_at` and `transcript` lines: Claude's account then.
    fn plan_with(name: &str, snapshot: Option<(&str, u64)>, transcript: &[String]) -> AccountUsage {
        let dir = temp_dir(name);
        let limits = dir.join("data/claude-limits.json");
        if let Some((json, saved_at)) = snapshot {
            assert!(save_claude_limits(&limits, json.as_bytes(), saved_at).unwrap());
        }
        if !transcript.is_empty() {
            write_at(&dir, "projects/-repo/s1.jsonl", &format!("{}\n", transcript.join("\n")), now());
        }
        claude_usage(&write(&dir, ".claude.json", OAUTH), &dir, false, now(), &limits)
    }

    const HOUR_MS: u64 = 3600 * 1000;

    #[test]
    fn the_status_lines_snapshot_gives_claudes_limits() {
        let usage = plan_with("limits-snapshot", Some((STATUSLINE, now() - HOUR_MS)), &[]);
        let shown: Vec<(&str, f64, bool)> = usage.limits.iter().map(|w| (w.name.as_str(), w.used_percent, w.limit_reached)).collect();
        assert_eq!(shown, [("weekly", 71.0, false), ("5h", 23.5, false)], "most used first");
        assert_eq!((usage.as_of, usage.limits_note), (Some(now() - HOUR_MS), None));
    }

    #[test]
    fn a_window_that_has_reset_since_reads_empty() {
        let passed = format!(r#"{{"rate_limits":{{"five_hour":{{"used_percentage":88,"resets_at":{}}}}}}}"#, now() / 1000 - 60);
        let usage = plan_with("limits-reset", Some((&passed, now() - 2 * HOUR_MS)), &[]);
        assert_eq!((usage.limits[0].used_percent, usage.limits[0].resets_at), (0.0, Some(now() / 1000 - 60)));
    }

    #[test]
    fn an_api_key_has_no_plan_limits() {
        let dir = temp_dir("limits-key");
        let limits = dir.join("claude-limits.json");
        save_claude_limits(&limits, STATUSLINE.as_bytes(), now()).unwrap();
        write(&dir, "settings.json", r#"{"apiKeyHelper":"~/bin/key.sh"}"#);
        let usage = claude_usage(&write(&dir, ".claude.json", OAUTH), &dir, false, now(), &limits);
        assert_eq!((usage.login, usage.limits.len(), usage.as_of, usage.limits_note), (Login::ApiKey, 0, None, None));
    }

    #[test]
    fn a_refused_request_shows_its_window_full() {
        let resets = now() / 1000 + 3600;
        let usage = plan_with("limits-refused", None, &[refusal("2026-11-02T11:00:00.000Z", "five_hour", resets)]);
        let w = &usage.limits[0];
        assert_eq!((w.window_minutes, w.used_percent, w.limit_reached, w.resets_at), (300, 100.0, true, Some(resets)));
        assert_eq!(usage.as_of, Some(now() - HOUR_MS));
        // One whose reset has passed says nothing any more.
        let stale = plan_with("limits-refused-old", None, &[refusal("2026-11-02T06:00:00.000Z", "five_hour", now() / 1000 - 1)]);
        assert!(stale.limits.is_empty());
    }

    #[test]
    fn the_newer_of_snapshot_and_refusal_wins() {
        let refused = [refusal("2026-11-02T11:00:00.000Z", "five_hour", now() / 1000 + 3600)];
        // The status line spoke after the refusal: its 5h stands.
        let newer = plan_with("limits-newer-snapshot", Some((STATUSLINE, now() - HOUR_MS / 2)), &refused);
        let five = newer.limits.iter().find(|w| w.window_minutes == 300).unwrap();
        assert_eq!((five.used_percent, five.limit_reached), (23.5, false));
        // The refusal came after: the 5h is full, the weekly still the snapshot's.
        let older = plan_with("limits-older-snapshot", Some((STATUSLINE, now() - 2 * HOUR_MS)), &refused);
        let shown: Vec<(u64, f64, bool)> = older.limits.iter().map(|w| (w.window_minutes, w.used_percent, w.limit_reached)).collect();
        assert_eq!(shown, [(300, 100.0, true), (10080, 71.0, false)]);
        assert_eq!(older.as_of, Some(now() - HOUR_MS));
    }

    #[cfg(unix)]
    #[test]
    fn a_plan_with_no_limits_says_how_to_get_them() {
        let none = plan_with("limits-note", None, &[]);
        assert_eq!(
            none.limits_note.as_deref(),
            Some("Run mira hooks setup (or Set up in the banner) to read your plan's limits from Claude Code's status line.")
        );
        let dir = temp_dir("limits-note-ours");
        let ours = format!(r#"{{"statusLine":{{"type":"command","command":"\"{}\" claude-statusline"}}}}"#, dir.join("mira").display());
        write(&dir, "settings.json", &ours);
        let usage = claude_usage(&write(&dir, ".claude.json", OAUTH), &dir, false, now(), &no_snapshot());
        assert_eq!(usage.limits_note.as_deref(), Some("Limits appear after Claude Code's next reply."));
    }

    #[test]
    fn codex_limits_are_as_of_their_rollout_line() {
        let dir = temp_dir("codex-as-of");
        let line = r#"{"timestamp":"2026-11-02T11:00:00.000Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":5,"window_minutes":300}}}}"#;
        write(&dir, "sessions/2026/11/02/rollout-2026-11-02T09-00-00-a.jsonl", &format!("{line}\n"));
        assert_eq!(codex_usage(&dir, false, now()).as_of, Some(now() - HOUR_MS));
    }

    #[test]
    fn a_clock_reads_hours_and_minutes() {
        let clock = local_clock(now());
        let hm = clock.trim_end_matches(" UTC");
        assert!(hm.len() == 5 && hm.as_bytes()[2] == b':' && hm.replace(':', "").bytes().all(|b| b.is_ascii_digit()), "{clock}");
    }

    #[test]
    fn a_window_is_named_by_its_length() {
        assert_eq!([300, 10080, 43200, 60].map(window_name), ["5h", "weekly", "30 days", "60m"]);
    }

    #[test]
    fn codex_on_a_key_is_an_api_key() {
        let dir = temp_dir("codex-key");
        write(&dir, "auth.json", r#"{"auth_mode":"apikey"}"#);
        assert_eq!(codex_usage(&dir, false, now()).login, Login::ApiKey);
        // A stored key is a key whatever the mode says.
        write(&dir, "auth.json", r#"{"OPENAI_API_KEY":"sk-test"}"#);
        let usage = codex_usage(&dir, false, now());
        assert_eq!((usage.login, usage.limits), (Login::ApiKey, vec![]));
    }

    #[test]
    fn codex_uses_a_key_from_the_environment_only_with_no_login_stored() {
        let dir = temp_dir("codex-env");
        assert_eq!(codex_usage(&dir, true, now()).login, Login::ApiKey);
        write(&dir, "auth.json", r#"{"auth_mode":null,"OPENAI_API_KEY":null,"tokens":null}"#);
        assert_eq!(codex_usage(&dir, true, now()).login, Login::ApiKey);
        // An older Codex's sign-in: tokens, no mode.
        write(&dir, "auth.json", r#"{"tokens":{"id_token":"x"}}"#);
        assert_eq!(codex_usage(&dir, true, now()).login, Login::Subscription);
    }

    #[test]
    fn codex_with_nothing_on_disk_is_no_login() {
        let dir = temp_dir("codex-none");
        let usage = codex_usage(&dir.join("missing"), false, now());
        assert_eq!((usage.login, usage.plan, usage.limits), (Login::None, None, vec![]));
    }
}
