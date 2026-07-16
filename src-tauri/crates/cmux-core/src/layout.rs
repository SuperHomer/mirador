//! Pure operations on a tab's split tree. The tree (`cmux_protocol::Node`)
//! is the single source of truth for pane layout; the frontend only renders
//! snapshots of it.

use cmux_protocol::{Direction, Node, SplitDir};

/// Splits the leaf holding `target`. If the leaf's parent already splits in
/// `dir`, the new pane is inserted next to it (sharing its space); otherwise
/// the leaf is wrapped in a new two-child split.
pub fn split(node: &mut Node, target: &str, dir: SplitDir, new_pane: &str) -> bool {
    match node {
        Node::Leaf { pane_id } if pane_id == target => {
            let old = node.clone();
            *node = Node::Split {
                dir,
                ratios: vec![0.5, 0.5],
                children: vec![
                    old,
                    Node::Leaf {
                        pane_id: new_pane.to_string(),
                    },
                ],
            };
            true
        }
        Node::Leaf { .. } => false,
        Node::Split {
            dir: d,
            ratios,
            children,
        } => {
            for (i, child) in children.iter_mut().enumerate() {
                let is_target = matches!(child, Node::Leaf { pane_id } if pane_id == target);
                if is_target {
                    if *d == dir {
                        let half = ratios[i] / 2.0;
                        ratios[i] = half;
                        ratios.insert(i + 1, half);
                        children.insert(
                            i + 1,
                            Node::Leaf {
                                pane_id: new_pane.to_string(),
                            },
                        );
                    } else {
                        let old = child.clone();
                        *child = Node::Split {
                            dir,
                            ratios: vec![0.5, 0.5],
                            children: vec![
                                old,
                                Node::Leaf {
                                    pane_id: new_pane.to_string(),
                                },
                            ],
                        };
                    }
                    return true;
                }
            }
            children.iter_mut().any(|c| split(c, target, dir, new_pane))
        }
    }
}

pub enum RemoveOutcome {
    NotFound,
    Removed,
    /// The tree is now empty (the removed leaf was the root).
    BecameEmpty,
}

/// Removes the leaf holding `target`. Single-child splits collapse into
/// their remaining child; the freed ratio is redistributed proportionally.
pub fn remove(node: &mut Node, target: &str) -> RemoveOutcome {
    match node {
        Node::Leaf { pane_id } => {
            if pane_id == target {
                RemoveOutcome::BecameEmpty
            } else {
                RemoveOutcome::NotFound
            }
        }
        Node::Split {
            ratios, children, ..
        } => {
            if let Some(i) = children
                .iter()
                .position(|c| matches!(c, Node::Leaf { pane_id } if pane_id == target))
            {
                children.remove(i);
                ratios.remove(i);
                normalize(ratios);
                collapse_if_single(node);
                return RemoveOutcome::Removed;
            }
            for child in children.iter_mut() {
                if matches!(remove(child, target), RemoveOutcome::Removed) {
                    collapse_if_single(node);
                    return RemoveOutcome::Removed;
                }
            }
            RemoveOutcome::NotFound
        }
    }
}

fn collapse_if_single(node: &mut Node) {
    if let Node::Split { children, .. } = node {
        if children.len() == 1 {
            *node = children.pop().unwrap();
        }
    }
}

fn normalize(ratios: &mut [f32]) {
    let sum: f32 = ratios.iter().sum();
    if sum > 0.0 {
        for r in ratios.iter_mut() {
            *r /= sum;
        }
    }
}

/// Replaces the ratios of the split reached by walking `path` (child indices
/// from the root). Ratios are re-normalized defensively.
pub fn set_ratios(node: &mut Node, path: &[usize], mut new_ratios: Vec<f32>) -> bool {
    let mut cur = node;
    for &idx in path {
        match cur {
            Node::Split { children, .. } if idx < children.len() => {
                cur = &mut children[idx];
            }
            _ => return false,
        }
    }
    match cur {
        Node::Split {
            ratios, children, ..
        } if new_ratios.len() == children.len() => {
            normalize(&mut new_ratios);
            *ratios = new_ratios;
            true
        }
        _ => false,
    }
}

pub fn pane_ids(node: &Node) -> Vec<String> {
    let mut out = Vec::new();
    collect_panes(node, &mut out);
    out
}

fn collect_panes(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Leaf { pane_id } => out.push(pane_id.clone()),
        Node::Split { children, .. } => children.iter().for_each(|c| collect_panes(c, out)),
    }
}

pub fn contains(node: &Node, target: &str) -> bool {
    match node {
        Node::Leaf { pane_id } => pane_id == target,
        Node::Split { children, .. } => children.iter().any(|c| contains(c, target)),
    }
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn collect_rects(node: &Node, rect: Rect, out: &mut Vec<(String, Rect)>) {
    match node {
        Node::Leaf { pane_id } => out.push((pane_id.clone(), rect)),
        Node::Split {
            dir,
            ratios,
            children,
        } => {
            let mut offset = 0.0;
            for (child, ratio) in children.iter().zip(ratios) {
                let child_rect = match dir {
                    SplitDir::Row => Rect {
                        x: rect.x + offset * rect.w,
                        y: rect.y,
                        w: ratio * rect.w,
                        h: rect.h,
                    },
                    SplitDir::Column => Rect {
                        x: rect.x,
                        y: rect.y + offset * rect.h,
                        w: rect.w,
                        h: ratio * rect.h,
                    },
                };
                collect_rects(child, child_rect, out);
                offset += ratio;
            }
        }
    }
}

/// Finds the pane adjacent to `from` in `direction`, judged geometrically on
/// the normalized layout: nearest facing edge, tie-broken by the largest
/// perpendicular overlap with the current pane.
pub fn neighbor(root: &Node, from: &str, direction: Direction) -> Option<String> {
    let mut rects = Vec::new();
    collect_rects(
        root,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        &mut rects,
    );
    let cur = rects.iter().find(|(id, _)| id == from)?.1;
    const EPS: f32 = 1e-4;

    let mut best: Option<(&String, f32, f32)> = None; // (id, edge_distance, overlap)
    for (id, r) in &rects {
        if id == from {
            continue;
        }
        let (facing, dist) = match direction {
            Direction::Left => (r.x + r.w <= cur.x + EPS, cur.x - (r.x + r.w)),
            Direction::Right => (r.x >= cur.x + cur.w - EPS, r.x - (cur.x + cur.w)),
            Direction::Up => (r.y + r.h <= cur.y + EPS, cur.y - (r.y + r.h)),
            Direction::Down => (r.y >= cur.y + cur.h - EPS, r.y - (cur.y + cur.h)),
        };
        if !facing {
            continue;
        }
        let overlap = match direction {
            Direction::Left | Direction::Right => {
                (cur.y + cur.h).min(r.y + r.h) - cur.y.max(r.y)
            }
            Direction::Up | Direction::Down => (cur.x + cur.w).min(r.x + r.w) - cur.x.max(r.x),
        };
        if overlap <= 0.0 {
            continue;
        }
        let better = match &best {
            None => true,
            Some((_, bd, bo)) => dist < bd - EPS || (dist < bd + EPS && overlap > *bo),
        };
        if better {
            best = Some((id, dist, overlap));
        }
    }
    best.map(|(id, _, _)| id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str) -> Node {
        Node::Leaf {
            pane_id: id.to_string(),
        }
    }

    #[test]
    fn split_root_leaf_wraps() {
        let mut root = leaf("a");
        assert!(split(&mut root, "a", SplitDir::Row, "b"));
        assert_eq!(pane_ids(&root), vec!["a", "b"]);
        match &root {
            Node::Split { dir, ratios, .. } => {
                assert_eq!(*dir, SplitDir::Row);
                assert_eq!(ratios, &vec![0.5, 0.5]);
            }
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn split_same_dir_inserts_sibling() {
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        split(&mut root, "a", SplitDir::Row, "c");
        // a splits again in the same direction: flat 3-child row, a halved.
        match &root {
            Node::Split {
                ratios, children, ..
            } => {
                assert_eq!(children.len(), 3);
                assert_eq!(pane_ids(&root), vec!["a", "c", "b"]);
                assert!((ratios[0] - 0.25).abs() < 1e-6);
                assert!((ratios[1] - 0.25).abs() < 1e-6);
                assert!((ratios[2] - 0.5).abs() < 1e-6);
            }
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn split_cross_dir_nests() {
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        split(&mut root, "b", SplitDir::Column, "c");
        assert_eq!(pane_ids(&root), vec!["a", "b", "c"]);
        match &root {
            Node::Split { children, .. } => match &children[1] {
                Node::Split { dir, .. } => assert_eq!(*dir, SplitDir::Column),
                _ => panic!("expected nested split"),
            },
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn remove_promotes_single_sibling() {
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        assert!(matches!(remove(&mut root, "b"), RemoveOutcome::Removed));
        assert_eq!(root, leaf("a"));
    }

    #[test]
    fn remove_nested_collapses_and_renormalizes() {
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        split(&mut root, "b", SplitDir::Column, "c");
        assert!(matches!(remove(&mut root, "c"), RemoveOutcome::Removed));
        // b's column split collapsed back into the row.
        match &root {
            Node::Split {
                ratios, children, ..
            } => {
                assert_eq!(children.len(), 2);
                assert_eq!(pane_ids(&root), vec!["a", "b"]);
                assert!((ratios.iter().sum::<f32>() - 1.0).abs() < 1e-6);
            }
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn remove_root_becomes_empty() {
        let mut root = leaf("a");
        assert!(matches!(remove(&mut root, "a"), RemoveOutcome::BecameEmpty));
    }

    #[test]
    fn remove_redistributes_ratios() {
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        split(&mut root, "a", SplitDir::Row, "c"); // a=0.25 c=0.25 b=0.5
        assert!(matches!(remove(&mut root, "b"), RemoveOutcome::Removed));
        match &root {
            Node::Split { ratios, .. } => {
                assert!((ratios.iter().sum::<f32>() - 1.0).abs() < 1e-6);
                assert!((ratios[0] - 0.5).abs() < 1e-6);
            }
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn set_ratios_by_path() {
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        split(&mut root, "b", SplitDir::Column, "c");
        // Resize the nested column split (child index 1 of the root).
        assert!(set_ratios(&mut root, &[1], vec![0.7, 0.3]));
        match &root {
            Node::Split { children, .. } => match &children[1] {
                Node::Split { ratios, .. } => {
                    assert!((ratios[0] - 0.7).abs() < 1e-6);
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
        // Wrong arity is rejected.
        assert!(!set_ratios(&mut root, &[1], vec![1.0]));
    }

    #[test]
    fn neighbor_navigation() {
        // [a | [b over c]]
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        split(&mut root, "b", SplitDir::Column, "c");

        assert_eq!(neighbor(&root, "a", Direction::Right), Some("b".into()));
        assert_eq!(neighbor(&root, "b", Direction::Left), Some("a".into()));
        assert_eq!(neighbor(&root, "b", Direction::Down), Some("c".into()));
        assert_eq!(neighbor(&root, "c", Direction::Up), Some("b".into()));
        assert_eq!(neighbor(&root, "c", Direction::Left), Some("a".into()));
        assert_eq!(neighbor(&root, "a", Direction::Left), None);
        assert_eq!(neighbor(&root, "a", Direction::Up), None);
    }

    #[test]
    fn neighbor_prefers_larger_overlap() {
        // Row of [a | column of [b, c]] where a is full height:
        // moving right from a should pick b or c; from c moving left → a.
        // Then: [column of [a, d] | b]: from b moving left picks the one
        // with larger overlap.
        let mut root = leaf("a");
        split(&mut root, "a", SplitDir::Row, "b");
        split(&mut root, "a", SplitDir::Column, "d");
        // shrink a: d gets most of the left column
        set_ratios(&mut root, &[0], vec![0.2, 0.8]).then_some(()).unwrap();
        assert_eq!(neighbor(&root, "b", Direction::Left), Some("d".into()));
    }
}
