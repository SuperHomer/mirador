//! The order a tab's wall shows its agents in, worked out here so the
//! frontend only draws it.
//!
//! Two orders, for two jobs. **Team order** places the tiles and fills the
//! Grid's pages: the team's own order when the tab runs a team, else pane
//! order — and it never changes with status, so a tile stays where it was
//! when its agent starts or stops working. **Urgency** picks which agent
//! Focus shows when none is pinned: whoever needs you, else whoever is
//! working, else the idle, then the rest, each group in team order.
//!
//! Beside the agents the wall has the tab's **Tools**: its other panes
//! that do work — terminals, browsers, diffs, graphs — in pane order. The
//! Grid's pages hold the agents first, then the tools;
//! urgency is the agents' alone, so Focus never picks a tool by itself.

use std::collections::HashMap;

use cmux_protocol::{AgentInfo, AgentStatus, TeamRun, ToolKind, WallTool, WorkspaceSnapshot};

use crate::state::{pane_title, PaneMeta};

/// `agents` (one tab's, in pane order) as pane ids in team order: the
/// team's members in the team's order, then any agent not in the team in
/// pane order. Without a team, pane order.
pub fn team_order(agents: &[&AgentInfo], team: Option<&TeamRun>) -> Vec<String> {
    let seat = |pane: &str| team.and_then(|t| t.members.iter().position(|m| m.pane == pane));
    let mut order: Vec<(usize, &str)> = agents
        .iter()
        .map(|a| (seat(&a.pane_id).unwrap_or(usize::MAX), a.pane_id.as_str()))
        .collect();
    // Stable: agents outside the team keep their pane order after it.
    order.sort_by_key(|(seat, _)| *seat);
    order.into_iter().map(|(_, pane)| pane.to_string()).collect()
}

/// `order` (pane ids in team order) by how much each agent wants you:
/// needs you, working, idle, then no known status; ties keep team order.
pub fn urgency(agents: &[&AgentInfo], order: &[String]) -> Vec<String> {
    let rank = |pane: &str| match agents.iter().find(|a| a.pane_id == pane).and_then(|a| a.status) {
        Some(AgentStatus::NeedsYou) => 0,
        Some(AgentStatus::Working) => 1,
        Some(AgentStatus::Idle) => 2,
        None => 3,
    };
    let mut by_urgency = order.to_vec();
    by_urgency.sort_by_key(|pane| rank(pane));
    by_urgency
}

/// `order` cut into the Grid's pages of `per_tab` agents; no agents, no
/// pages.
pub fn pages(order: &[String], per_tab: usize) -> Vec<Vec<String>> {
    order.chunks(per_tab.max(1)).map(<[String]>::to_vec).collect()
}

/// What kind of tool a pane is, if it is one. The panes that edit the
/// app's own files or show its release notes are not: they are about
/// Mirador, not the work.
pub fn tool_kind(meta: Option<&PaneMeta>) -> Option<ToolKind> {
    let Some(m) = meta else {
        return Some(ToolKind::Terminal);
    };
    if m.agent_editor || m.settings || m.whats_new.is_some() {
        None
    } else if m.browser_url.is_some() {
        Some(ToolKind::Browser)
    } else if m.diff_repo.is_some() {
        Some(ToolKind::Diff)
    } else if m.graph_repo.is_some() {
        Some(ToolKind::Graph)
    } else {
        Some(ToolKind::Terminal)
    }
}

/// `panes` (one tab's, in pane order) that are tools, agents among them:
/// which panes are agents is known only once the app adds them, and
/// `fill` drops those.
pub fn tools(panes: &[String], meta: &HashMap<String, PaneMeta>) -> Vec<WallTool> {
    panes
        .iter()
        .filter_map(|pane| {
            let m = meta.get(pane);
            Some(WallTool { pane_id: pane.clone(), kind: tool_kind(m)?, title: pane_title(m) })
        })
        .collect()
}

/// Fills every tab's `wall_urgency` and `wall_pages` from the snapshot's
/// agents, which come in tab and pane order, and takes them out of its
/// `wall_tools`. The pages hold the agents, then the tools.
pub fn fill(snapshot: &mut WorkspaceSnapshot, per_tab: u8) {
    for tab in &mut snapshot.tabs {
        let agents: Vec<&AgentInfo> = snapshot.agents.iter().filter(|a| a.tab_id == tab.id).collect();
        tab.wall_tools.retain(|t| !agents.iter().any(|a| a.pane_id == t.pane_id));
        let mut order = team_order(&agents, tab.team.as_ref());
        tab.wall_urgency = urgency(&agents, &order);
        order.extend(tab.wall_tools.iter().map(|t| t.pane_id.clone()));
        tab.wall_pages = pages(&order, per_tab as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_protocol::{Harness, TeamSeat};

    fn agent(pane: &str, status: Option<AgentStatus>) -> AgentInfo {
        AgentInfo {
            pane_id: pane.into(),
            tab_id: "t".into(),
            harness: Harness::Claude,
            role: None,
            member: None,
            has_activity: false,
            name: None,
            model: None,
            effort: None,
            status,
            message: None,
            asking: None,
            topic: None,
            since_ms: None,
            cwd: None,
            branch: None,
        }
    }

    fn team(panes: &[&str]) -> TeamRun {
        TeamRun {
            name: "crew".into(),
            members: panes
                .iter()
                .map(|p| TeamSeat { name: format!("m-{p}"), pane: p.to_string(), ..Default::default() })
                .collect(),
        }
    }

    fn ids(panes: &[&str]) -> Vec<String> {
        panes.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn without_a_team_the_order_is_pane_order() {
        let (a, b, c) = (agent("a", None), agent("b", None), agent("c", None));
        assert_eq!(team_order(&[&a, &b, &c], None), ids(&["a", "b", "c"]));
    }

    #[test]
    fn a_team_orders_its_members_and_others_follow_in_pane_order() {
        let (a, b, c, d) = (agent("a", None), agent("b", None), agent("c", None), agent("d", None));
        // The team is c then a; b and d started by hand, after it.
        let crew = team(&["c", "gone", "a"]);
        assert_eq!(team_order(&[&a, &b, &c, &d], Some(&crew)), ids(&["c", "a", "b", "d"]));
    }

    #[test]
    fn urgency_puts_needs_you_first_and_breaks_ties_by_team_order() {
        let idle = agent("idle", Some(AgentStatus::Idle));
        let w1 = agent("w1", Some(AgentStatus::Working));
        let unknown = agent("unknown", None);
        let asks = agent("asks", Some(AgentStatus::NeedsYou));
        let w2 = agent("w2", Some(AgentStatus::Working));
        let agents = [&idle, &w1, &unknown, &asks, &w2];
        let order = ids(&["idle", "w2", "unknown", "w1", "asks"]);
        assert_eq!(urgency(&agents, &order), ids(&["asks", "w2", "w1", "idle", "unknown"]));
        // Team order itself never moves with status.
        assert_eq!(order, ids(&["idle", "w2", "unknown", "w1", "asks"]));
    }

    #[test]
    fn pages_hold_per_tab_agents_each() {
        let n = |count: usize| (0..count).map(|i| i.to_string()).collect::<Vec<_>>();
        let sizes = |count: usize, per_tab: usize| pages(&n(count), per_tab).iter().map(Vec::len).collect::<Vec<_>>();
        assert!(pages(&[], 4).is_empty());
        assert_eq!(sizes(3, 4), [3]);
        assert_eq!(sizes(4, 4), [4]);
        assert_eq!(sizes(5, 4), [4, 1]);
        assert_eq!(sizes(9, 4), [4, 4, 1]);
        assert_eq!(sizes(10, 4), [4, 4, 2]);
        assert_eq!(sizes(5, 6), [5]);
        assert_eq!(sizes(9, 6), [6, 3]);
        assert_eq!(sizes(10, 6), [6, 4]);
        assert_eq!(sizes(9, 9), [9]);
        assert_eq!(sizes(10, 9), [9, 1]);
        assert!(pages(&[], 9).is_empty());
        // In order: the first page is the first agents.
        assert_eq!(pages(&n(5), 4)[1], ids(&["4"]));
    }

    #[test]
    fn fill_lays_out_each_tab_from_its_own_agents() {
        let mut snapshot = crate::state::Workspace::default().snapshot(&std::collections::HashMap::new());
        snapshot.tabs[0].id = "t".into();
        snapshot.tabs[0].team = Some(team(&["b"]));
        // Its own pane, a shell, is not one of these agents.
        snapshot.tabs[0].wall_tools.clear();
        let mut other = agent("x", Some(AgentStatus::NeedsYou));
        other.tab_id = "elsewhere".into();
        snapshot.agents = vec![agent("a", Some(AgentStatus::Working)), agent("b", Some(AgentStatus::Idle)), other];
        fill(&mut snapshot, 4);
        assert_eq!(snapshot.tabs[0].wall_pages, vec![ids(&["b", "a"])]);
        assert_eq!(snapshot.tabs[0].wall_urgency, ids(&["a", "b"]));
    }

    fn kinds(tools: &[WallTool]) -> Vec<(&str, ToolKind)> {
        tools.iter().map(|t| (t.pane_id.as_str(), t.kind)).collect()
    }

    #[test]
    fn tools_come_in_pane_order_with_their_kind() {
        let meta = HashMap::from([
            ("web".to_string(), PaneMeta { browser_url: Some("https://example.com".into()), ..Default::default() }),
            ("diff".to_string(), PaneMeta { diff_repo: Some("/r".into()), diff_spec: Some("worktree".into()), ..Default::default() }),
            ("graph".to_string(), PaneMeta { graph_repo: Some("/r".into()), ..Default::default() }),
            ("ssh".to_string(), PaneMeta { remote_host: Some("box".into()), ..Default::default() }),
            ("cmd".to_string(), PaneMeta { command: Some("npm test".into()), ..Default::default() }),
        ]);
        let panes = ids(&["graph", "shell", "web", "ssh", "diff", "cmd"]);
        assert_eq!(
            kinds(&tools(&panes, &meta)),
            [
                ("graph", ToolKind::Graph),
                ("shell", ToolKind::Terminal),
                ("web", ToolKind::Browser),
                ("ssh", ToolKind::Terminal),
                ("diff", ToolKind::Diff),
                ("cmd", ToolKind::Terminal),
            ]
        );
        assert_eq!(tools(&panes, &meta)[2].title, "https://example.com");
    }

    #[test]
    fn the_apps_own_panes_are_not_tools() {
        let meta = HashMap::from([
            ("editor".to_string(), PaneMeta { agent_editor: true, ..Default::default() }),
            ("settings".to_string(), PaneMeta { settings: true, ..Default::default() }),
            ("news".to_string(), PaneMeta { whats_new: Some("0.1.35".into()), ..Default::default() }),
        ]);
        let panes = ids(&["editor", "shell", "settings", "news"]);
        assert_eq!(kinds(&tools(&panes, &meta)), [("shell", ToolKind::Terminal)]);
    }

    #[test]
    fn fill_never_makes_an_agent_a_tool() {
        let mut ws = crate::state::Workspace::default();
        let first = ws.focused_pane();
        let second = ws.split_pane(&first, cmux_protocol::SplitDir::Row).unwrap();
        let mut snapshot = ws.snapshot(&HashMap::new());
        let tab = snapshot.tabs[0].id.clone();
        assert_eq!(kinds(&snapshot.tabs[0].wall_tools), [(first.as_str(), ToolKind::Terminal), (second.as_str(), ToolKind::Terminal)]);
        let mut claude = agent(&first, None);
        claude.tab_id = tab;
        snapshot.agents = vec![claude];
        fill(&mut snapshot, 4);
        assert_eq!(kinds(&snapshot.tabs[0].wall_tools), [(second.as_str(), ToolKind::Terminal)]);
        // A tab whose only pane is an agent has no tools.
        let mut alone = agent(&second, None);
        alone.tab_id = snapshot.tabs[0].id.clone();
        snapshot.agents.push(alone);
        fill(&mut snapshot, 4);
        assert!(snapshot.tabs[0].wall_tools.is_empty());
    }

    /// A tab of `panes`, the first `agents` of them agents, with `meta`.
    fn filled(panes: usize, agents: usize, meta: &HashMap<String, PaneMeta>, per_tab: u8) -> WorkspaceSnapshot {
        let mut ws = crate::state::Workspace::default();
        let mut last = ws.focused_pane();
        let mut ids = vec![last.clone()];
        for _ in 1..panes {
            last = ws.split_pane(&last, cmux_protocol::SplitDir::Row).unwrap();
            ids.push(last.clone());
        }
        let meta: HashMap<String, PaneMeta> =
            meta.iter().map(|(k, m)| (ids[k.parse::<usize>().unwrap()].clone(), m.clone())).collect();
        let mut snapshot = ws.snapshot(&meta);
        let tab = snapshot.tabs[0].id.clone();
        snapshot.agents = ids[..agents]
            .iter()
            .map(|p| AgentInfo { tab_id: tab.clone(), ..agent(p, Some(AgentStatus::Idle)) })
            .collect();
        fill(&mut snapshot, per_tab);
        snapshot
    }

    #[test]
    fn terminal_tools_follow_the_agents_on_the_pages() {
        let snapshot = filled(5, 3, &HashMap::new(), 4);
        let tab = &snapshot.tabs[0];
        let agents: Vec<String> = snapshot.agents.iter().map(|a| a.pane_id.clone()).collect();
        let tools: Vec<String> = tab.wall_tools.iter().map(|t| t.pane_id.clone()).collect();
        // Three agents and a tool on the first page, the other tool after.
        let mut first = agents.clone();
        first.push(tools[0].clone());
        assert_eq!(tab.wall_pages, vec![first, vec![tools[1].clone()]]);
        // Focus follows the agents only.
        assert_eq!(tab.wall_urgency.len(), 3);
        assert!(tab.wall_urgency.iter().all(|p| agents.contains(p)));
    }

    #[test]
    fn a_tab_of_tools_alone_has_pages() {
        let snapshot = filled(2, 0, &HashMap::new(), 4);
        let tab = &snapshot.tabs[0];
        assert_eq!(tab.wall_pages.len(), 1);
        assert_eq!(tab.wall_pages[0].len(), 2);
        assert!(tab.wall_urgency.is_empty());
    }

    #[test]
    fn every_kind_of_tool_follows_the_agents_in_pane_order() {
        let meta = HashMap::from([
            ("1".to_string(), PaneMeta { browser_url: Some("https://example.com".into()), ..Default::default() }),
            ("2".to_string(), PaneMeta { graph_repo: Some("/r".into()), ..Default::default() }),
            ("3".to_string(), PaneMeta { diff_repo: Some("/r".into()), diff_spec: Some("worktree".into()), ..Default::default() }),
        ]);
        let snapshot = filled(5, 1, &meta, 6);
        let tab = &snapshot.tabs[0];
        assert_eq!(tab.wall_tools.len(), 4);
        let of = |kind| tab.wall_tools.iter().find(|t| t.kind == kind).unwrap().pane_id.clone();
        assert_eq!(
            tab.wall_pages,
            vec![vec![
                snapshot.agents[0].pane_id.clone(),
                of(ToolKind::Browser),
                of(ToolKind::Graph),
                of(ToolKind::Diff),
                of(ToolKind::Terminal),
            ]]
        );
    }
}
