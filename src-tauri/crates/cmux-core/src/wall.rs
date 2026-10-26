//! The order a tab's wall shows its agents in, worked out here so the
//! frontend only draws it.
//!
//! Two orders, for two jobs. **Team order** places the tiles and fills the
//! Grid's pages: the team's own order when the tab runs a team, else pane
//! order — and it never changes with status, so a tile stays where it was
//! when its agent starts or stops working. **Urgency** picks which agent
//! Focus shows when none is pinned: whoever needs you, else whoever is
//! working, else the idle, then the rest, each group in team order.

use cmux_protocol::{AgentInfo, AgentStatus, TeamRun, WorkspaceSnapshot};

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

/// Fills every tab's `wall_urgency` and `wall_pages` from the snapshot's
/// agents, which come in tab and pane order.
pub fn fill(snapshot: &mut WorkspaceSnapshot, per_tab: u8) {
    for tab in &mut snapshot.tabs {
        let agents: Vec<&AgentInfo> = snapshot.agents.iter().filter(|a| a.tab_id == tab.id).collect();
        let order = team_order(&agents, tab.team.as_ref());
        tab.wall_urgency = urgency(&agents, &order);
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
        let mut other = agent("x", Some(AgentStatus::NeedsYou));
        other.tab_id = "elsewhere".into();
        snapshot.agents = vec![agent("a", Some(AgentStatus::Working)), agent("b", Some(AgentStatus::Idle)), other];
        fill(&mut snapshot, 4);
        assert_eq!(snapshot.tabs[0].wall_pages, vec![ids(&["b", "a"])]);
        assert_eq!(snapshot.tabs[0].wall_urgency, ids(&["a", "b"]));
    }
}
