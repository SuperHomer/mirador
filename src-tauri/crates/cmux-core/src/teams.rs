//! Teams: roles started together in one tab, and kept track of there.
//!
//! A team is `teams/<name>.md` beside `roles/` and `ways/` — the
//! repository's `.mirador/teams`, then the user's — with its members in the
//! frontmatter and its charter as the body:
//!
//! ```markdown
//! ---
//! description: Plan, build, review
//! way: our-way
//! members:
//!   - role: planner
//!     lead: true
//!   - role: dev
//!     count: 2
//!   - role: reviewer
//! ---
//! The planner splits the work; devs build; nothing merges unreviewed.
//! ```
//!
//! Every member gets its role's prompt, then the charter, the team's way of
//! working, and who its teammates are — by name, which is stable, and with
//! the command that lists their panes, which are not.

use std::path::Path;

use cmux_protocol::{
    AgentRole, AgentTeam, AgentWay, LayerWeight, Node, PromptLayer, PromptLayerKind, RoleCatalog, RoleSource, SeatWeight,
    SplitDir, TeamFields, TeamMember, TeamWeight,
};
use serde::Deserialize;

/// More agents than this in one tab is not a team, it is a mistake.
pub const MAX_MEMBERS: u32 = 12;

#[derive(Deserialize, Default)]
#[serde(default)]
struct TeamFront {
    description: Option<String>,
    way: Option<String>,
    layout: Option<String>,
    members: Vec<TeamMember>,
    #[serde(flatten)]
    other: std::collections::BTreeMap<String, serde_yaml_ng::Value>,
}

/// A team file: the team, and the frontmatter keys it did not know.
pub fn parse_team(name: &str, path: &Path, source: RoleSource, text: &str) -> Result<(AgentTeam, Vec<String>), String> {
    let (front, body) = crate::roles::split_frontmatter(text)?;
    let front: TeamFront = match front {
        Some(yaml) if !yaml.trim().is_empty() => {
            serde_yaml_ng::from_str(yaml).map_err(|e| format!("frontmatter: {e}"))?
        }
        _ => TeamFront::default(),
    };
    let trim = |v: Option<String>| v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let members = front
        .members
        .into_iter()
        .map(|m| TeamMember {
            role: m.role.trim().to_string(),
            name: trim(m.name),
            ..m
        })
        .collect();
    let body = body.trim();
    Ok((
        AgentTeam {
            name: name.to_string(),
            description: trim(front.description).map(|d| d.split_whitespace().collect::<Vec<_>>().join(" ")),
            way: trim(front.way),
            members,
            layout: trim(front.layout),
            prompt: (!body.is_empty()).then(|| body.to_string()),
            source,
            path: path.to_string_lossy().into_owned(),
            problem: None,
        },
        front.other.into_keys().collect(),
    ))
}

/// `text`, a team file, with the team form's `fields`: description, way,
/// layout and members set in its frontmatter (a key removed when empty),
/// the charter as its body. A member says only what differs from the
/// default — no `count: 1`, no `lead: false`, no `name` but its own — so
/// the file reads as if written by hand. Every key Mirador does not know
/// stays, where it was, a member's own included: it keeps those of the
/// member it was (same role and name, else same place and role). The frontmatter is rewritten, so comments in it
/// are lost.
pub fn set_team_fields(text: &str, fields: &TeamFields) -> Result<String, String> {
    use crate::roles::{set_key, text_value};
    use serde_yaml_ng::{Mapping, Value};
    let (mut map, _) = crate::roles::front_and_body(text)?;
    set_key(&mut map, "description", text_value(&fields.description));
    set_key(&mut map, "way", text_value(&fields.way));
    set_key(&mut map, "layout", text_value(&fields.layout));
    // The members as the file has them, for the keys another tool may have
    // put on one: each member written keeps those of the one it was.
    let originals: Vec<Mapping> = match map.get("members") {
        Some(Value::Sequence(list)) => list.iter().filter_map(|m| m.as_mapping().cloned()).collect(),
        _ => Vec::new(),
    };
    let field = |m: &Mapping, key: &str| m.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let members: Vec<Value> = fields
        .members
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let role = m.role.trim();
            let name = m.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
            // The same role and name, else the same place if its role
            // matches; a member added is a new one.
            let was = originals
                .iter()
                .find(|o| field(o, "role").as_deref() == Some(role) && field(o, "name").as_deref() == name)
                .or_else(|| originals.get(i).filter(|o| field(o, "role").as_deref() == Some(role)));
            let mut entry = was.cloned().unwrap_or_default();
            set_key(&mut entry, "role", Some(role.into()));
            set_key(&mut entry, "name", name.map(Into::into));
            set_key(&mut entry, "count", (m.count != 1).then(|| m.count.into()));
            set_key(&mut entry, "lead", m.lead.then_some(true.into()));
            Value::Mapping(entry)
        })
        .collect();
    set_key(&mut map, "members", (!members.is_empty()).then_some(Value::Sequence(members)));
    crate::roles::write_file(&map, fields.charter.as_deref())
}

/// One seat of a team about to start: what the member is called, which
/// role it is, and whether it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub name: String,
    pub role: String,
    pub lead: bool,
}

/// The team's members one by one: `dev` twice is `dev-1` and `dev-2`, a
/// `name` replaces the role's. The lead is the member marked so (its first
/// seat, if it has several), else the first.
pub fn seats(team: &AgentTeam) -> Vec<Seat> {
    let mut out = Vec::new();
    for m in &team.members {
        let base = m.name.clone().unwrap_or_else(|| m.role.clone());
        for i in 1..=m.count {
            out.push(Seat {
                name: if m.count > 1 { format!("{base}-{i}") } else { base.clone() },
                role: m.role.clone(),
                // A lead counted twice is one lead and a teammate.
                lead: m.lead && i == 1,
            });
        }
    }
    if !out.iter().any(|s| s.lead) {
        if let Some(first) = out.first_mut() {
            first.lead = true;
        }
    }
    out
}

/// A name for a member added to a running team: `wanted` if free and not
/// already numbered; else the next number — `dev-3` beside `dev-1` and
/// `dev-2`, `dev-2` beside `dev`.
pub fn new_seat_name(wanted: &str, taken: &[&str]) -> String {
    let numbered: Vec<u32> = taken
        .iter()
        .filter_map(|t| t.strip_prefix(wanted)?.strip_prefix('-')?.parse().ok())
        .collect();
    if !taken.contains(&wanted) && numbered.is_empty() {
        return wanted.to_string();
    }
    let next = numbered.into_iter().max().unwrap_or(1) + 1;
    (next..)
        .map(|i| format!("{wanted}-{i}"))
        .find(|n| !taken.contains(&n.as_str()))
        .unwrap()
}

/// Where a member is, from whether its pane is still in the tab and
/// whether that pane still runs an agent.
pub fn seat_state(in_tab: bool, is_agent: bool) -> cmux_protocol::SeatState {
    match (in_tab, is_agent) {
        (false, _) => cmux_protocol::SeatState::Closed,
        (true, true) => cmux_protocol::SeatState::Running,
        (true, false) => cmux_protocol::SeatState::Exited,
    }
}

/// Why `team` cannot start, if it cannot.
pub fn problem_of(team: &AgentTeam, roles: &[AgentRole], ways: &[AgentWay]) -> Option<String> {
    if team.members.is_empty() {
        return Some("a team needs `members`".into());
    }
    let total: u32 = team.members.iter().map(|m| m.count).sum();
    if team.members.iter().any(|m| m.count == 0) {
        return Some("a member's `count` must be at least 1".into());
    }
    if total > MAX_MEMBERS {
        return Some(format!("{total} members; a team has at most {MAX_MEMBERS}"));
    }
    if team.members.iter().filter(|m| m.lead).count() > 1 {
        return Some("more than one member is marked `lead`".into());
    }
    if let Some(layout) = team.layout.as_deref().filter(|l| !matches!(*l, "grid" | "columns" | "rows")) {
        return Some(format!("layout `{layout}` is not grid, columns or rows"));
    }
    if let Some(way) = team.way.as_ref().filter(|way| !ways.iter().any(|w| &&w.name == way)) {
        return Some(format!("way of working `{way}` is not in any ways/ directory"));
    }
    for m in &team.members {
        let Some(role) = roles.iter().find(|r| r.name == m.role) else {
            return Some(format!("no role `{}`", m.role));
        };
        if let Some(problem) = &role.problem {
            return Some(format!("role `{}`: {problem}", m.role));
        }
        if let Some(name) = m.name.as_deref().filter(|n| !crate::roles::is_name(n)) {
            return Some(format!("`{name}` cannot name a member"));
        }
    }
    let seats = seats(team);
    for (i, seat) in seats.iter().enumerate() {
        if seats[..i].iter().any(|s| s.name == seat.name) {
            return Some(format!("two members are called `{}`: give one a `name`", seat.name));
        }
    }
    None
}

/// What a member is told about its team, after its role's own prompt: the
/// charter, the team's way of working, and its teammates. `mira` is the
/// path to the CLI, which the agent's shell may not have on its PATH.
pub fn member_prompt(team: &AgentTeam, seat: &Seat, all: &[Seat], way: Option<&str>, mira: &str) -> String {
    let way = way.map(|w| AgentWay { prompt: w.to_string(), ..Default::default() });
    member_layers(team, seat, all, way.as_ref(), mira).into_iter().map(|l| l.text).collect()
}

/// The source of the layer Mirador writes itself.
pub const WRITTEN_BY_MIRADOR: &str = "written by Mirador";

/// [`member_prompt`] as the layers it is made of, each as it is told —
/// they follow each other as they are: who the member is and its
/// teammates, the charter, then the team's way of working.
pub fn member_layers(team: &AgentTeam, seat: &Seat, all: &[Seat], way: Option<&AgentWay>, mira: &str) -> Vec<PromptLayer> {
    let mut out = format!("# Your team: {}\n\n", team.name);
    let role_of = if seat.name == seat.role { String::new() } else { format!(", a {}", seat.role) };
    let leads = if seat.lead { " You lead it: the team's task comes to you." } else { "" };
    out.push_str(&format!("You are `{}`{role_of}, in team `{}`.{leads}\n", seat.name, team.name));
    let others: Vec<&Seat> = all.iter().filter(|s| s.name != seat.name).collect();
    if !others.is_empty() {
        out.push_str("\nYour teammates, each an agent in a pane of this tab:\n\n");
        for s in others {
            let lead = if s.lead { ", leads the team" } else { "" };
            out.push_str(&format!("- `{}` ({}{lead})\n", s.name, s.role));
        }
        out.push_str(&format!(
            "\nTo hand a teammate work or tell it something, \
             `{mira} agent send <name> \"<message>\"` types the message in as its next \
             prompt, saying it is from you; add `--wait` to block until it has finished \
             (or `{mira} agent wait <name>` later). `{mira} team list` shows each one's \
             pane and what it is doing, and `{mira} read-screen --pane <pane>` its screen.\n"
        ));
    }
    let mut layers = vec![PromptLayer { kind: PromptLayerKind::Team, source: WRITTEN_BY_MIRADOR.into(), text: out }];
    if let Some(charter) = &team.prompt {
        layers.push(PromptLayer {
            kind: PromptLayerKind::Charter,
            source: team.path.clone(),
            text: format!("\n## The team's charter\n\n{charter}\n"),
        });
    }
    if let Some(way) = way {
        layers.push(PromptLayer {
            kind: PromptLayerKind::TeamWay,
            source: way.path.clone(),
            text: format!("\n## The team's way of working\n\n{}\n", way.prompt.trim()),
        });
    }
    layers
}

/// A member's own layers, from the catalog: [`member_layers`] with the
/// team's way of working unless its role already follows that way.
pub fn seat_layers(catalog: &RoleCatalog, team: &AgentTeam, seat: &Seat, all: &[Seat], mira: &str) -> Vec<PromptLayer> {
    member_layers(team, seat, all, team_way(catalog, team, &seat.role), mira)
}

/// The team's way of working, for a member of `role` — none when the role
/// already follows the same one, which its prompt then has already.
pub fn team_way<'a>(catalog: &'a RoleCatalog, team: &AgentTeam, role: &str) -> Option<&'a AgentWay> {
    let way = team.way.as_ref()?;
    let role_way = catalog.roles.iter().find(|r| r.name == role).and_then(|r| r.way.as_ref());
    if role_way == Some(way) {
        return None;
    }
    catalog.ways.iter().find(|w| &w.name == way)
}

/// What `team` tells its members, in estimated tokens: each seat's layers
/// — its role's, then its own in the team, the very ones it starts with —
/// the least a turn costs to re-send them, read from the prompt cache, and
/// what the first turn costs to write them to it.
/// What else a turn sends is not counted (see [`TeamWeight`]). A seat whose
/// model has no known price has tokens and no floor, and is counted as
/// unpriced. Fails as starting the team would: on a team that cannot
/// start, whose weight would be short or none, and on a role it cannot
/// find.
pub fn team_weight(catalog: &RoleCatalog, team: &AgentTeam, mira: &str) -> Result<TeamWeight, String> {
    if let Some(problem) = &team.problem {
        return Err(format!("team `{}`: {problem}", team.name));
    }
    let all = seats(team);
    let mut out = Vec::new();
    for seat in &all {
        let (role, mut layers) = crate::roles::role_layers(catalog, &seat.role)?;
        layers.extend(seat_layers(catalog, team, seat, &all, mira));
        let layers: Vec<LayerWeight> = layers
            .into_iter()
            .map(|l| LayerWeight { kind: l.kind, source: l.source, tokens: crate::pricing::estimate_tokens(&l.text) })
            .collect();
        let tokens = layers.iter().map(|l| l.tokens).sum();
        let rates = role.model.as_deref().and_then(crate::pricing::rates);
        let floor = rates.map(|rates| {
            crate::pricing::cost(rates, crate::pricing::Usage { cache_read: tokens, ..Default::default() })
        });
        let start = rates.map(|rates| {
            crate::pricing::cost(rates, crate::pricing::Usage { cache_write: tokens, ..Default::default() })
        });
        out.push(SeatWeight {
            name: seat.name.clone(),
            role: seat.role.clone(),
            harness: role.harness,
            model: role.model,
            effort: role.effort,
            layers,
            tokens,
            cache_read_per_million: rates.map(|r| r.cache_read),
            per_turn_floor_usd: floor,
            start_usd: start,
        });
    }
    Ok(TeamWeight {
        team: team.name.clone(),
        tokens: out.iter().map(|s| s.tokens).sum(),
        per_turn_floor_usd: out.iter().filter_map(|s| s.per_turn_floor_usd).sum(),
        start_usd: out.iter().filter_map(|s| s.start_usd).sum(),
        unpriced: out.iter().filter(|s| s.per_turn_floor_usd.is_none()).count() as u32,
        seats: out,
        prices_checked: crate::pricing::PRICES_CHECKED.to_string(),
    })
}

/// The tab a team fills: its panes, `layout` "columns" side by side, "rows"
/// one above the other, or "grid" (the default) in as many columns as it
/// takes to keep them square-ish — each with an equal share.
pub fn layout(panes: &[String], layout: Option<&str>) -> Node {
    let leaf = |p: &String| Node::Leaf { pane_id: p.clone() };
    let line = |dir: SplitDir, nodes: Vec<Node>| -> Node {
        if nodes.len() == 1 {
            return nodes.into_iter().next().unwrap();
        }
        let n = nodes.len();
        Node::Split { dir, ratios: vec![1.0 / n as f32; n], children: nodes }
    };
    match layout {
        Some("columns") => line(SplitDir::Row, panes.iter().map(leaf).collect()),
        Some("rows") => line(SplitDir::Column, panes.iter().map(leaf).collect()),
        _ => {
            let cols = (panes.len() as f64).sqrt().ceil().max(1.0) as usize;
            let columns: Vec<Node> = (0..cols)
                .map(|c| {
                    // Fill column by column, earlier ones taking the extra
                    // pane: the lead, first, gets the tallest share.
                    let per = panes.len() / cols + usize::from(c < panes.len() % cols);
                    let start: usize = (0..c).map(|k| panes.len() / cols + usize::from(k < panes.len() % cols)).sum();
                    line(SplitDir::Column, panes[start..start + per].iter().map(leaf).collect())
                })
                .filter(|n| !matches!(n, Node::Split { children, .. } if children.is_empty()))
                .collect();
            line(SplitDir::Row, columns)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team(text: &str) -> AgentTeam {
        parse_team("squad", Path::new("/r/.mirador/teams/squad.md"), RoleSource::Repo, text).unwrap().0
    }

    fn role(name: &str) -> AgentRole {
        AgentRole { name: name.into(), ..Default::default() }
    }

    const SQUAD: &str = "---\ndescription: Plan,\n  build\nway: our-way\nmembers:\n  - role: planner\n  - role: dev\n    count: 2\n    lead: true\n  - role: reviewer\n    name: gate\n---\nShip small.\n";

    #[test]
    fn parses_a_team_file() {
        let t = team(SQUAD);
        assert_eq!(t.description.as_deref(), Some("Plan, build"));
        assert_eq!(t.way.as_deref(), Some("our-way"));
        assert_eq!(t.prompt.as_deref(), Some("Ship small."));
        assert_eq!(t.members.len(), 3);
        assert_eq!((t.members[1].count, t.members[1].lead), (2, true));
        assert_eq!(t.members[0].count, 1);
        let (_, ignored) = parse_team("x", Path::new("x.md"), RoleSource::Global, "---\nmembers: []\ncolor: red\n---\n").unwrap();
        assert_eq!(ignored, ["color"]);
        assert!(parse_team("x", Path::new("x.md"), RoleSource::Global, "---\nmembers: planner\n---\n").is_err());
    }

    #[test]
    fn names_each_seat() {
        let names: Vec<(String, bool)> = seats(&team(SQUAD)).into_iter().map(|s| (s.name, s.lead)).collect();
        assert_eq!(
            names,
            [("planner".into(), false), ("dev-1".into(), true), ("dev-2".into(), false), ("gate".into(), false)]
        );
        // No lead marked: the first member leads.
        let t = team("---\nmembers: [{role: a}, {role: b}]\n---\n");
        assert!(seats(&t)[0].lead && !seats(&t)[1].lead);
    }

    #[test]
    fn says_why_a_team_cannot_start() {
        let roles = [role("planner"), role("dev"), role("reviewer")];
        let ways = [AgentWay { name: "our-way".into(), ..Default::default() }];
        assert_eq!(problem_of(&team(SQUAD), &roles, &ways), None);
        let p = |text: &str| problem_of(&team(text), &roles, &ways).unwrap_or_default();
        assert!(p("---\n---\n").contains("needs `members`"));
        assert!(p("---\nmembers: [{role: ghost}]\n---\n").contains("no role `ghost`"));
        assert!(p("---\nmembers: [{role: dev, count: 13}]\n---\n").contains("at most 12"));
        assert!(p("---\nmembers: [{role: dev, count: 0}]\n---\n").contains("at least 1"));
        assert!(p("---\nmembers: [{role: dev, lead: true}, {role: planner, lead: true}]\n---\n").contains("more than one"));
        assert!(p("---\nmembers: [{role: dev}, {role: dev}]\n---\n").contains("two members are called `dev`"));
        assert!(p("---\nmembers: [{role: dev, name: 'a b'}]\n---\n").contains("cannot name"));
        assert!(p("---\nway: nope\nmembers: [{role: dev}]\n---\n").contains("`nope`"));
        assert!(p("---\nlayout: circle\nmembers: [{role: dev}]\n---\n").contains("circle"));
        let broken = [AgentRole { problem: Some("way missing".into()), ..role("dev") }];
        assert!(problem_of(&team("---\nmembers: [{role: dev}]\n---\n"), &broken, &ways).unwrap().contains("way missing"));
    }

    #[test]
    fn tells_a_member_about_its_team() {
        let t = team(SQUAD);
        let all = seats(&t);
        let p = member_prompt(&t, &all[3], &all, Some("Tests first."), "/app/mira");
        assert!(p.starts_with("# Your team: squad\n\nYou are `gate`, a reviewer, in team `squad`.\n"), "{p}");
        assert!(p.contains("- `planner` (planner)\n- `dev-1` (dev, leads the team)\n- `dev-2` (dev)\n"));
        assert!(!p.contains("- `gate`"));
        assert!(p.contains("`/app/mira team list`"));
        assert!(p.contains("`/app/mira agent send <name> \"<message>\"`"));
        assert!(p.contains("## The team's charter\n\nShip small.\n"));
        assert!(p.ends_with("## The team's way of working\n\nTests first.\n"));
        let lead = member_prompt(&t, &all[1], &all, None, "mira");
        assert!(lead.contains("You lead it"));
        assert!(!lead.contains("way of working"));
    }

    /// What the launch gave a member before its prompt came in layers:
    /// the role's resolved prompt, then the roster, joined as
    /// `prepare_launch` joined them.
    fn as_launched(catalog: &RoleCatalog, t: &AgentTeam, seat: &Seat, all: &[Seat]) -> Option<String> {
        let (_, role_prompt) = crate::roles::resolve(catalog, &seat.role).unwrap();
        let way = team_way(catalog, t, &seat.role).map(|w| w.prompt.as_str());
        let roster = member_prompt(t, seat, all, way, "mira");
        let prompt = [role_prompt.as_deref(), Some(roster.as_str())]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        (!prompt.is_empty()).then_some(prompt)
    }

    fn told(catalog: &RoleCatalog, t: &AgentTeam, seat: &Seat, all: &[Seat]) -> Vec<PromptLayer> {
        let (_, mut layers) = crate::roles::role_layers(catalog, &seat.role).unwrap();
        layers.extend(seat_layers(catalog, t, seat, all, "mira"));
        layers
    }

    #[test]
    fn a_members_layers_join_into_what_it_is_told() {
        let way = |name: &str, prompt: &str| AgentWay {
            name: name.into(),
            prompt: prompt.into(),
            path: format!("/r/.mirador/ways/{name}.md"),
            ..Default::default()
        };
        let catalog = RoleCatalog {
            roles: vec![
                AgentRole {
                    prompt: Some("  You review.\n".into()),
                    way: Some("careful".into()),
                    path: Some("/r/.mirador/roles/reviewer.md".into()),
                    ..role("reviewer")
                },
                role("dev"),
                role("planner"),
            ],
            ways: vec![way("careful", "Read twice.\n"), way("our-way", "Tests first.")],
            ..Default::default()
        };
        let t = team(SQUAD);
        let all = seats(&t);

        // gate: its role's prompt and way, the roster, the charter, the team's way.
        let layers = told(&catalog, &t, &all[3], &all);
        let kinds: Vec<_> = layers.iter().map(|l| l.kind).collect();
        use PromptLayerKind::*;
        assert_eq!(kinds, [Role, RoleWay, Team, Charter, TeamWay]);
        let sources: Vec<_> = layers.iter().map(|l| l.source.as_str()).collect();
        assert_eq!(
            sources,
            [
                "/r/.mirador/roles/reviewer.md",
                "/r/.mirador/ways/careful.md",
                WRITTEN_BY_MIRADOR,
                "/r/.mirador/teams/squad.md",
                "/r/.mirador/ways/our-way.md"
            ]
        );
        assert_eq!(crate::roles::join_layers(&layers), as_launched(&catalog, &t, &all[3], &all));

        // A team with no charter or way, a role with no prompt or way: the
        // roster alone.
        let bare = team("---\nmembers: [{role: dev}, {role: planner}]\n---\n");
        let all = seats(&bare);
        let layers = told(&catalog, &bare, &all[0], &all);
        assert_eq!(layers.iter().map(|l| l.kind).collect::<Vec<_>>(), [Team]);
        let joined = crate::roles::join_layers(&layers);
        assert_eq!(joined, as_launched(&catalog, &bare, &all[0], &all));
        assert!(joined.unwrap().starts_with("# Your team: squad\n\nYou are `dev`, in team `squad`."));
    }

    #[test]
    fn a_teams_weight_is_what_each_seat_is_told() {
        let way = |name: &str, prompt: &str| AgentWay {
            name: name.into(),
            prompt: prompt.into(),
            path: format!("/r/.mirador/ways/{name}.md"),
            ..Default::default()
        };
        let catalog = RoleCatalog {
            roles: vec![
                // Follows the team's own way: told it once, as its role's.
                AgentRole {
                    prompt: Some("Plan it.".repeat(100)),
                    model: Some("opus".into()),
                    way: Some("our-way".into()),
                    ..role("planner")
                },
                AgentRole { prompt: Some("Build it.".into()), model: Some("claude-sonnet-5-5[1m]".into()), ..role("dev") },
                AgentRole {
                    harness: cmux_protocol::Harness::Codex,
                    model: Some("gpt-6-luna".into()),
                    ..role("reviewer")
                },
            ],
            ways: vec![way("our-way", &"Tests first. ".repeat(50))],
            ..Default::default()
        };
        let t = team(SQUAD);
        let w = team_weight(&catalog, &t, "mira").unwrap();

        // dev twice is two seats, each counted.
        let names: Vec<_> = w.seats.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["planner", "dev-1", "dev-2", "gate"]);
        for s in &w.seats {
            assert_eq!(s.tokens, s.layers.iter().map(|l| l.tokens).sum::<u64>(), "{}", s.name);
            assert!(s.layers.iter().all(|l| l.tokens > 0), "{}", s.name);
        }
        assert_eq!(w.tokens, w.seats.iter().map(|s| s.tokens).sum::<u64>());

        // The planner's way is the team's: there once, as its role's way,
        // never again as the team's.
        use PromptLayerKind::*;
        let planner = &w.seats[0];
        assert_eq!(planner.layers.iter().map(|l| l.kind).collect::<Vec<_>>(), [Role, RoleWay, Team, Charter]);
        let dev = &w.seats[1];
        assert_eq!(dev.layers.iter().map(|l| l.kind).collect::<Vec<_>>(), [Role, Team, Charter, TeamWay]);

        // The floor is the tokens at the model's cache-read rate.
        let floor = |s: &SeatWeight, rate: f64| s.tokens as f64 * rate / 1_000_000.0;
        assert!((planner.per_turn_floor_usd.unwrap() - floor(planner, 0.20)).abs() < 1e-12);
        assert!((dev.per_turn_floor_usd.unwrap() - floor(dev, 0.20)).abs() < 1e-12);
        assert_eq!(planner.model.as_deref(), Some("opus"));
        // ...and that rate is given, to show how the floor is worked out.
        assert_eq!(planner.cache_read_per_million, Some(0.20));

        // Codex: tokens, no price, and counted as such.
        let gate = &w.seats[3];
        assert_eq!((gate.harness, gate.per_turn_floor_usd), (cmux_protocol::Harness::Codex, None));
        assert_eq!((gate.cache_read_per_million, gate.start_usd), (None, None));
        assert!(gate.tokens > 0);
        assert_eq!(w.unpriced, 1);
        let priced: f64 = w.seats[..3].iter().map(|s| s.per_turn_floor_usd.unwrap()).sum();
        assert!((w.per_turn_floor_usd - priced).abs() < 1e-12);

        // Starting writes the tokens to the cache, at the cache-write rate
        // (Opus 5.5: 1.25 × $4); the team's start leaves Codex out too.
        let start = |s: &SeatWeight| s.start_usd.unwrap();
        assert!((start(planner) - planner.tokens as f64 * 5.0 / 1_000_000.0).abs() < 1e-12);
        let started: f64 = w.seats[..3].iter().map(start).sum();
        assert!((w.start_usd - started).abs() < 1e-12);
        // With a model's default rates, writing is 12.5× reading.
        let mut haiku = catalog.clone();
        haiku.roles[1].model = Some("claude-haiku-4-5".into());
        let dev = &team_weight(&haiku, &t, "mira").unwrap().seats[1];
        assert!((start(dev) - dev.tokens as f64 * 1.25 / 1_000_000.0).abs() < 1e-12);
        assert!((start(dev) / dev.per_turn_floor_usd.unwrap() - 12.5).abs() < 1e-9);
        assert_eq!(w.prices_checked, crate::pricing::PRICES_CHECKED);

        // A role with no model runs the harness's default, whose price is
        // not known either.
        let mut plain = catalog.clone();
        plain.roles[1].model = None;
        let w = team_weight(&plain, &t, "mira").unwrap();
        assert_eq!((w.seats[1].per_turn_floor_usd, w.seats[1].start_usd, w.unpriced), (None, None, 3));

        // A role the team names but nobody wrote: as starting would.
        let mut missing = catalog.clone();
        missing.roles.retain(|r| r.name != "dev");
        assert!(team_weight(&missing, &t, "mira").unwrap_err().contains("no role `dev`"));
    }

    #[test]
    fn a_team_that_cannot_start_is_not_weighed() {
        let catalog = RoleCatalog {
            roles: vec![role("dev"), role("planner")],
            ways: vec![AgentWay { name: "our-way".into(), ..Default::default() }],
            ..Default::default()
        };
        // Its problem as the catalog finds it (`roles::catalog_in`).
        let refused = |text: &str| {
            let mut t = team(text);
            t.problem = problem_of(&t, &catalog.roles, &catalog.ways);
            team_weight(&catalog, &t, "mira").unwrap_err()
        };
        assert_eq!(
            refused("---
members: [{role: dev, count: 0}, {role: planner}]
---
"),
            "team `squad`: a member's `count` must be at least 1"
        );
        assert_eq!(
            refused("---
way: nowhere
members: [{role: dev}]
---
"),
            "team `squad`: way of working `nowhere` is not in any ways/ directory"
        );
        assert_eq!(refused("---
members: []
---
"), "team `squad`: a team needs `members`");
    }

    fn fields_of(t: &AgentTeam) -> TeamFields {
        TeamFields {
            description: t.description.clone(),
            way: t.way.clone(),
            layout: t.layout.clone(),
            members: t.members.clone(),
            charter: t.prompt.clone(),
        }
    }

    #[test]
    fn saves_the_forms_fields_into_a_team_file() {
        // A key Mirador does not know, between its own, and a comment.
        let text = "---\ndescription: Plan, build\ncolor: red # mine\nway: our-way\nmembers:\n  - role: planner\n  - role: dev\n    count: 2\n    lead: true\n---\nShip small.\n";
        let t = team(text);
        let same = set_team_fields(text, &fields_of(&t)).unwrap();
        assert_eq!(team(&same), t);
        // Its key and value survive, in their place; the comment does not.
        assert!(same.starts_with("---\ndescription: Plan, build\ncolor: red\nway: our-way\nmembers:\n"), "{same}");
        assert!(same.ends_with("---\nShip small.\n"));

        // Defaults and blanks are left out.
        let lean = TeamFields {
            description: Some("  ".into()),
            members: vec![TeamMember { role: "dev".into(), name: Some("".into()), count: 1, lead: false }],
            ..Default::default()
        };
        let out = set_team_fields(text, &lean).unwrap();
        assert_eq!(out, "---\ncolor: red\nmembers:\n- role: dev\n---\n");
        let t = team(&out);
        assert_eq!((t.description, t.way, t.prompt), (None, None, None));

        // A file with no frontmatter gets one.
        let fresh = set_team_fields("", &TeamFields { members: vec![TeamMember { role: "dev".into(), name: Some("ana".into()), count: 3, lead: true }], ..Default::default() }).unwrap();
        assert_eq!(fresh, "---\nmembers:\n- role: dev\n  name: ana\n  count: 3\n  lead: true\n---\n");
    }

    #[test]
    fn a_members_own_keys_stay_with_it() {
        let text = "---\nmembers:\n  - role: planner\n    color: blue\n  - role: dev\n    name: ana\n    seat: 3\n---\n";
        let t = team(text);
        let same = set_team_fields(text, &fields_of(&t)).unwrap();
        assert_eq!(same, "---\nmembers:\n- role: planner\n  color: blue\n- role: dev\n  name: ana\n  seat: 3\n---\n");
        // ana counted twice and moved first: her key goes with her.
        let mut f = fields_of(&t);
        f.members.swap(0, 1);
        f.members[0].count = 2;
        let out = set_team_fields(text, &f).unwrap();
        assert_eq!(out, "---\nmembers:\n- role: dev\n  name: ana\n  seat: 3\n  count: 2\n- role: planner\n  color: blue\n---\n");
        // The planner removed, and a reviewer added: its keys go, the new
        // one has none.
        let mut f = fields_of(&t);
        f.members[0] = TeamMember { role: "reviewer".into(), name: None, count: 1, lead: false };
        let out = set_team_fields(text, &f).unwrap();
        assert_eq!(out, "---\nmembers:\n- role: reviewer\n- role: dev\n  name: ana\n  seat: 3\n---\n");
    }

    #[test]
    fn a_built_in_team_survives_a_save_unchanged() {
        for (path, name, text) in crate::builtin::of("teams") {
            let parse = |text: &str| parse_team(&name, Path::new(&path), RoleSource::Builtin, text).unwrap();
            let (t, ignored) = parse(text);
            let saved = set_team_fields(text, &fields_of(&t)).unwrap();
            assert_eq!(parse(&saved), (t, ignored), "{name}");
        }
    }

    #[test]
    fn names_an_added_member_and_says_where_each_is() {
        assert_eq!(new_seat_name("reviewer", &["dev-1", "dev-2"]), "reviewer");
        assert_eq!(new_seat_name("dev", &["dev", "dev-2"]), "dev-3");
        assert_eq!(new_seat_name("dev", &["dev"]), "dev-2");
        assert_eq!(new_seat_name("reviewer", &["reviewer-1", "reviewer-2", "charts"]), "reviewer-3");
        assert_eq!(new_seat_name("dev", &["dev-ops"]), "dev");
        use cmux_protocol::SeatState::*;
        assert_eq!([seat_state(true, true), seat_state(true, false), seat_state(false, true)], [Running, Exited, Closed]);
    }

    #[test]
    fn lays_the_members_out() {
        let panes: Vec<String> = (1..=5).map(|i| format!("p{i}")).collect();
        let leaves = |n: &Node| crate::layout::pane_ids(n);
        // Five in a grid: three columns of 2, 2 and 1.
        match layout(&panes, None) {
            Node::Split { dir: SplitDir::Row, children, ratios } => {
                assert_eq!(children.len(), 3);
                assert!((ratios[0] - 1.0 / 3.0).abs() < 1e-6);
                assert_eq!(leaves(&children[0]), ["p1", "p2"]);
                assert_eq!(leaves(&children[2]), ["p5"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(layout(&panes, Some("columns")), Node::Split { dir: SplitDir::Row, ref children, .. } if children.len() == 5));
        assert!(matches!(layout(&panes, Some("rows")), Node::Split { dir: SplitDir::Column, ref children, .. } if children.len() == 5));
        assert_eq!(layout(&panes[..1], None), Node::Leaf { pane_id: "p1".into() });
        assert_eq!(leaves(&layout(&panes, None)), panes);
    }
}
