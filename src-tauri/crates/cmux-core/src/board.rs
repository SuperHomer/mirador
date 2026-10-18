//! Where panes sit on the board: every tab is a frame, every pane a card in
//! it. The board is a second layout of the same panes, kept apart from the
//! split tree: moving a card never changes a split, and a split never moves
//! a card already there. Cards are placed relative to their frame's origin,
//! so a frame moves as a group; the frame's size follows its cards.
//!
//! Pure functions over a tab's tree and cards. `Workspace` calls them
//! wherever panes and tabs come and go, in either view, so every pane has a
//! card whether or not the board was ever shown.

use std::collections::HashMap;

use cmux_protocol::{BoardPoint, BoardRect, Camera, Node, SplitDir};

use crate::layout;

/// The size a tab's splits are laid out at when they first become cards.
pub const TAB_WIDTH: f64 = 1200.0;
pub const TAB_HEIGHT: f64 = 800.0;
/// Space between neighbouring cards.
pub const CARD_GAP: f64 = 12.0;
/// Room inside a frame around its cards...
pub const FRAME_PADDING: f64 = 16.0;
/// ...and above them, for the frame's label.
pub const FRAME_HEADER: f64 = 36.0;
/// Space between frames placed side by side.
pub const FRAME_GAP: f64 = 80.0;
pub const MIN_CARD_WIDTH: f64 = 160.0;
pub const MIN_CARD_HEIGHT: f64 = 100.0;
pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 4.0;
/// Further out than anyone pans: a coordinate past it is a bug or garbage,
/// and would make every size derived from it meaningless.
const MAX_COORD: f64 = 10_000_000.0;

/// A tab's cards by pane id, relative to its frame's origin.
pub type Cards = HashMap<String, BoardRect>;

/// Cards for panes that have none yet: the tab's splits scaled to
/// `TAB_WIDTH` × `TAB_HEIGHT`, so the board starts out looking like the tab.
/// A tab split so finely that a pane would come out smaller than a card
/// can be is laid out larger instead, keeping its proportions: shrinking
/// only the small cards would make them overlap their neighbours.
pub fn default_cards(root: &Node) -> Cards {
    let rects = layout::leaf_rects(root);
    let need = |min: f64, share: f32, size: f64| (min + CARD_GAP) / (share as f64 * size);
    let width = rects
        .iter()
        .map(|(_, r)| need(MIN_CARD_WIDTH, r.w, TAB_WIDTH))
        .fold(1.0, f64::max)
        * TAB_WIDTH;
    let height = rects
        .iter()
        .map(|(_, r)| need(MIN_CARD_HEIGHT, r.h, TAB_HEIGHT))
        .fold(1.0, f64::max)
        * TAB_HEIGHT;
    rects
        .into_iter()
        .map(|(pane, r)| {
            let card = BoardRect {
                x: FRAME_PADDING + r.x as f64 * width + CARD_GAP / 2.0,
                y: FRAME_HEADER + r.y as f64 * height + CARD_GAP / 2.0,
                w: r.w as f64 * width - CARD_GAP,
                h: r.h as f64 * height - CARD_GAP,
            };
            (pane, card)
        })
        .collect()
}

/// The frame around `cards`, on the board: from its origin to just past
/// its furthest card.
pub fn frame_rect(origin: BoardPoint, cards: &Cards) -> BoardRect {
    let right = cards
        .values()
        .map(|c| c.x + c.w)
        .fold(FRAME_PADDING, f64::max);
    let bottom = cards
        .values()
        .map(|c| c.y + c.h)
        .fold(FRAME_HEADER, f64::max);
    BoardRect {
        x: origin.x,
        y: origin.y,
        w: right + FRAME_PADDING,
        h: bottom + FRAME_PADDING,
    }
}

/// Where a new frame goes: right of the rightmost one.
pub fn next_frame_origin(frames: impl IntoIterator<Item = BoardRect>) -> BoardPoint {
    let right = frames.into_iter().map(|f| f.x + f.w).reduce(f64::max);
    match right {
        Some(right) => BoardPoint {
            x: right + FRAME_GAP,
            y: 0.0,
        },
        None => BoardPoint::default(),
    }
}

/// The card for a pane split from `source`: the same size, beside it in the
/// split's direction (below it for a column split), or the nearest free
/// place further along, so no card is covered.
pub fn card_beside(cards: &Cards, source: &str, dir: SplitDir) -> Option<BoardRect> {
    let src = *cards.get(source)?;
    let right = |step: f64| BoardRect {
        x: src.x + step * (src.w + CARD_GAP),
        ..src
    };
    let below = |step: f64| BoardRect {
        y: src.y + step * (src.h + CARD_GAP),
        ..src
    };
    // Terminates: past the furthest card in either direction, nothing is
    // left to overlap.
    let mut step = 1.0;
    loop {
        let candidates = match dir {
            SplitDir::Row => [right(step), below(step)],
            SplitDir::Column => [below(step), right(step)],
        };
        if let Some(free) = candidates.into_iter().find(|c| is_free(cards, c)) {
            return Some(free);
        }
        step += 1.0;
    }
}

fn is_free(cards: &Cards, rect: &BoardRect) -> bool {
    !cards.values().any(|c| overlaps(c, rect))
}

fn overlaps(a: &BoardRect, b: &BoardRect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Makes `cards` hold exactly the panes of `root`: drops the cards of panes
/// that left, and gives a card to any pane without one — the tab's split
/// layout when it has none at all (a tab from a session saved before the
/// board), otherwise its default size, right of the others.
pub fn settle(root: &Node, cards: &mut Cards) {
    let panes = layout::pane_ids(root);
    cards.retain(|pane, _| panes.contains(pane));
    if cards.is_empty() {
        *cards = default_cards(root);
        return;
    }
    let defaults = default_cards(root);
    for pane in panes {
        if cards.contains_key(&pane) {
            continue;
        }
        let size = defaults[&pane];
        let right = cards
            .values()
            .map(|c| c.x + c.w)
            .fold(FRAME_PADDING, f64::max);
        let card = BoardRect {
            x: right + CARD_GAP,
            y: FRAME_HEADER + CARD_GAP / 2.0,
            ..size
        };
        cards.insert(pane, card);
    }
}

/// Room left around the frames when the board is fitted to the window.
const FIT_MARGIN: f64 = 40.0;

/// The camera that shows every frame, centred in a `width` × `height`
/// viewport. Never past 100%: a single small tab is shown at its size,
/// not blown up.
pub fn fit(frames: impl IntoIterator<Item = BoardRect>, width: f64, height: f64) -> Camera {
    let frames: Vec<BoardRect> = frames.into_iter().collect();
    let Some(left) = frames.iter().map(|f| f.x).reduce(f64::min) else {
        return Camera::default();
    };
    let top = frames.iter().map(|f| f.y).fold(f64::INFINITY, f64::min);
    let right = frames.iter().map(|f| f.x + f.w).fold(f64::NEG_INFINITY, f64::max);
    let bottom = frames.iter().map(|f| f.y + f.h).fold(f64::NEG_INFINITY, f64::max);
    let bounds = BoardRect { x: left, y: top, w: right - left, h: bottom - top };
    let zoom = ((width - 2.0 * FIT_MARGIN) / bounds.w)
        .min((height - 2.0 * FIT_MARGIN) / bounds.h)
        .clamp(MIN_ZOOM, 1.0);
    centre_on(bounds, width, height, zoom)
}

/// The camera that puts `rect`'s centre in the middle of a `width` ×
/// `height` viewport, at `zoom`.
pub fn centre_on(rect: BoardRect, width: f64, height: f64, zoom: f64) -> Camera {
    Camera {
        x: rect.x + rect.w / 2.0 - width / zoom / 2.0,
        y: rect.y + rect.h / 2.0 - height / zoom / 2.0,
        zoom,
    }
}

fn check(values: &[f64]) -> Result<(), String> {
    if values.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
        Ok(())
    } else {
        Err(format!("not a place on the board: {values:?}"))
    }
}

/// A camera as asked for, its zoom held to what the board can show.
pub fn camera(x: f64, y: f64, zoom: f64) -> Result<Camera, String> {
    check(&[x, y, zoom])?;
    Ok(Camera {
        x,
        y,
        zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
    })
}

/// A card as asked for, kept inside its frame (below the label) and at
/// least big enough to show something.
pub fn card(x: f64, y: f64, w: f64, h: f64) -> Result<BoardRect, String> {
    check(&[x, y, w, h])?;
    Ok(BoardRect {
        x: x.max(FRAME_PADDING),
        y: y.max(FRAME_HEADER),
        w: w.max(MIN_CARD_WIDTH),
        h: h.max(MIN_CARD_HEIGHT),
    })
}

pub fn frame_origin(x: f64, y: f64) -> Result<BoardPoint, String> {
    check(&[x, y])?;
    Ok(BoardPoint { x, y })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str) -> Node {
        Node::Leaf {
            pane_id: id.into(),
        }
    }

    fn split(dir: SplitDir, children: Vec<Node>) -> Node {
        let n = children.len();
        Node::Split {
            dir,
            ratios: vec![1.0 / n as f32; n],
            children,
        }
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> BoardRect {
        BoardRect { x, y, w, h }
    }

    #[test]
    fn a_tabs_splits_become_its_first_cards() {
        let root = split(SplitDir::Row, vec![leaf("a"), split(SplitDir::Column, vec![leaf("b"), leaf("c")])]);
        let cards = default_cards(&root);
        let (a, b, c) = (cards["a"], cards["b"], cards["c"]);
        // Side by side, the right half stacked, a gap between each.
        assert_eq!(a.x, FRAME_PADDING + CARD_GAP / 2.0);
        assert_eq!(a.y, FRAME_HEADER + CARD_GAP / 2.0);
        assert_eq!(a.w, TAB_WIDTH / 2.0 - CARD_GAP);
        assert_eq!(a.h, TAB_HEIGHT - CARD_GAP);
        assert_eq!(b.x, c.x);
        assert_eq!(b.x - (a.x + a.w), CARD_GAP);
        assert_eq!(c.y - (b.y + b.h), CARD_GAP);
        assert!((c.y + c.h - (a.y + a.h)).abs() < 1e-3, "the columns end level");
    }

    #[test]
    fn a_frame_wraps_its_cards() {
        let cards = default_cards(&leaf("a"));
        let frame = frame_rect(BoardPoint { x: 100.0, y: 50.0 }, &cards);
        assert_eq!((frame.x, frame.y), (100.0, 50.0));
        let a = cards["a"];
        assert_eq!(frame.w, a.x + a.w + FRAME_PADDING);
        assert_eq!(frame.h, a.y + a.h + FRAME_PADDING);
    }

    #[test]
    fn frames_go_in_a_row() {
        assert_eq!(next_frame_origin([]), BoardPoint { x: 0.0, y: 0.0 });
        let frames = [rect(0.0, 0.0, 500.0, 300.0), rect(900.0, 400.0, 200.0, 100.0)];
        assert_eq!(next_frame_origin(frames), BoardPoint { x: 1100.0 + FRAME_GAP, y: 0.0 });
    }

    #[test]
    fn a_split_pane_gets_a_card_beside_its_source() {
        let mut cards = Cards::new();
        cards.insert("a".into(), rect(16.0, 36.0, 300.0, 200.0));
        let right = card_beside(&cards, "a", SplitDir::Row).unwrap();
        assert_eq!(right, rect(16.0 + 300.0 + CARD_GAP, 36.0, 300.0, 200.0));
        let below = card_beside(&cards, "a", SplitDir::Column).unwrap();
        assert_eq!(below, rect(16.0, 36.0 + 200.0 + CARD_GAP, 300.0, 200.0));
        assert_eq!(card_beside(&cards, "gone", SplitDir::Row), None);
    }

    #[test]
    fn a_split_card_never_covers_another() {
        let mut cards = Cards::new();
        cards.insert("a".into(), rect(0.0, 0.0, 100.0, 100.0));
        // Right of `a` is taken, and so is below it: the next free place
        // along the split's direction.
        cards.insert("b".into(), rect(110.0, 0.0, 100.0, 100.0));
        cards.insert("c".into(), rect(0.0, 150.0, 50.0, 50.0));
        let card = card_beside(&cards, "a", SplitDir::Row).unwrap();
        assert_eq!(card, rect(2.0 * (100.0 + CARD_GAP), 0.0, 100.0, 100.0));
        assert!(is_free(&cards, &card));
        // A column split looks below first, and finds it free past `c`.
        let card = card_beside(&cards, "a", SplitDir::Column).unwrap();
        assert!(is_free(&cards, &card));
        assert_eq!(card.x, 0.0);
    }

    #[test]
    fn settling_follows_the_tree() {
        let root = split(SplitDir::Row, vec![leaf("a"), leaf("b")]);
        // A tab saved before the board: its splits.
        let mut cards = Cards::new();
        settle(&root, &mut cards);
        assert_eq!(cards, default_cards(&root));

        // A card for a pane that left goes; a pane without one gets one
        // clear of the others; a card already placed stays put.
        let mut cards = Cards::new();
        let a = rect(400.0, 300.0, 200.0, 150.0);
        cards.insert("a".into(), a);
        cards.insert("gone".into(), rect(0.0, 0.0, 10.0, 10.0));
        settle(&root, &mut cards);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards["a"], a);
        assert!(!overlaps(&cards["a"], &cards["b"]));
    }

    #[test]
    fn no_default_card_is_smaller_than_a_card_can_be() {
        // Eight panes side by side: an eighth of 1200 is under the minimum.
        let root = split(SplitDir::Row, (0..8).map(|i| leaf(&i.to_string())).collect());
        let cards = default_cards(&root);
        for c in cards.values() {
            assert!(c.w >= MIN_CARD_WIDTH - 1e-6 && c.h >= MIN_CARD_HEIGHT, "{c:?}");
        }
        // Still in a row, a gap apart, none covering another.
        let (a, b) = (cards["0"], cards["1"]);
        assert!((b.x - (a.x + a.w) - CARD_GAP).abs() < 1e-6);
        assert_eq!(a.y, b.y);
        // A tab with room for its panes keeps the nominal size.
        let two = default_cards(&split(SplitDir::Row, vec![leaf("a"), leaf("b")]));
        assert_eq!(two["a"].w, TAB_WIDTH / 2.0 - CARD_GAP);
    }

    #[test]
    fn fitting_shows_every_frame_and_never_magnifies() {
        let frames = [rect(0.0, 0.0, 1000.0, 500.0), rect(1100.0, 200.0, 900.0, 800.0)];
        let cam = fit(frames, 1080.0, 620.0);
        // 2000 x 1000 board units into a 1000 x 540 window (margins off).
        assert!((cam.zoom - 0.5).abs() < 1e-9, "{cam:?}");
        // Centred: the board's middle (1000, 500) is the viewport's middle.
        assert!((cam.x + 1080.0 / cam.zoom / 2.0 - 1000.0).abs() < 1e-9);
        assert!((cam.y + 620.0 / cam.zoom / 2.0 - 500.0).abs() < 1e-9);
        // One small frame is shown at its own size.
        assert_eq!(fit([rect(0.0, 0.0, 200.0, 100.0)], 1080.0, 620.0).zoom, 1.0);
        assert_eq!(fit([], 1080.0, 620.0), Camera::default());
    }

    #[test]
    fn centring_puts_the_rect_in_the_middle() {
        let cam = centre_on(rect(100.0, 100.0, 200.0, 100.0), 1000.0, 600.0, 1.0);
        assert_eq!(cam, Camera { x: 200.0 - 500.0, y: 150.0 - 300.0, zoom: 1.0 });
    }

    #[test]
    fn what_the_board_is_told_is_checked() {
        assert_eq!(camera(1.0, 2.0, 100.0).unwrap().zoom, MAX_ZOOM);
        assert_eq!(camera(1.0, 2.0, 0.0).unwrap().zoom, MIN_ZOOM);
        assert!(camera(f64::NAN, 0.0, 1.0).is_err());
        assert!(camera(0.0, f64::INFINITY, 1.0).is_err());
        // Kept inside its frame, and big enough to show something.
        assert_eq!(card(-50.0, 0.0, 10.0, 10.0).unwrap(), rect(FRAME_PADDING, FRAME_HEADER, MIN_CARD_WIDTH, MIN_CARD_HEIGHT));
        assert!(card(0.0, 0.0, 1e300, 10.0).is_err());
        assert!(frame_origin(-5000.0, 12.5).is_ok());
        assert!(frame_origin(f64::NAN, 0.0).is_err());
    }
}
