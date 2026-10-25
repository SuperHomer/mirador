import { useMemo, useRef, useState } from "react";
import {
  AgentInfo,
  BoardRect,
  CardRect,
  BrowserPaneInfo,
  DiffPaneInfo,
  Node,
  SplitDir,
  GraphPaneInfo,
  AgentEditorPaneInfo,
  TabSnapshot,
  WhatsNewPaneInfo,
  closePane,
  focusPane,
  setCardRect,
  setSplitRatios,
} from "../bindings";
import { boardDrag } from "./BoardHost";
import { flyToCard } from "../state/boardStore";
import { TerminalPane } from "../terminal/TerminalPane";
import { getTerminal, isLent } from "../terminal/registry";
import { BrowserPane } from "../browser/BrowserPane";
import { DiffPane } from "../diff/DiffPane";
import { WhatsNewPane } from "../whatsnew/WhatsNewPane";
import { GraphPane } from "../graph/GraphPane";
import { AgentEditor } from "../agents/AgentEditor";
import { AgentWall } from "../agents/AgentWall";

// Mirrors `cmux_core::board`, which has the last word when a card is
// dropped: a card stays inside its frame, below its label, and big enough
// to show something.
const FRAME_PADDING = 16;
const FRAME_HEADER = 36;
const MIN_CARD_WIDTH = 160;
const MIN_CARD_HEIGHT = 100;

/** Where a card can be resized from: its sides and corners. */
const EDGES = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];

interface Frac {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface PaneBox {
  paneId: string;
  rect: Frac;
}

interface DividerBox {
  /** Path of child indices from the root to the owning split. */
  path: number[];
  /** Divider sits between children index-1 and index. */
  index: number;
  dir: SplitDir;
  ratios: number[];
  /** Fractional position of the boundary line. */
  pos: Frac;
  /** Fractional extent of the whole split container on the drag axis. */
  span: number;
  start: number;
}

/**
 * Terminals render as one flat list of absolutely-positioned panes keyed by
 * pane id — a tree-shape change never remounts an existing terminal, so
 * xterm buffers survive splits.
 *
 * On the board the same list is drawn as cards: each slot takes its card's
 * place instead of its split's, under a header, and nothing else about it
 * changes — the pane inside is the same element either way.
 */
export function SplitLayer({
  tab,
  board,
  active,
  current,
  agents,
  unreadPanes,
  agentPanes,
  browserPanes,
  remotePanes,
  diffPanes,
  whatsNewPanes,
  graphPanes,
  agentEditorPanes,
}: {
  tab: TabSnapshot;
  /** Drawn as cards on the board. */
  board: boolean;
  /** This tab is on screen: the active one, or any tab on the board. */
  active: boolean;
  /** This tab is the active one, where the keyboard goes. */
  current: boolean;
  agents: AgentInfo[];
  unreadPanes: string[];
  agentPanes: { paneId: string; command: string }[];
  browserPanes: BrowserPaneInfo[];
  remotePanes: { paneId: string; host: string }[];
  diffPanes: DiffPaneInfo[];
  whatsNewPanes: WhatsNewPaneInfo[];
  graphPanes: GraphPaneInfo[];
  agentEditorPanes: AgentEditorPaneInfo[];
}) {
  // Live ratio overrides while a divider drag is in flight.
  const [overrides, setOverrides] = useState<Map<string, number[]>>(new Map());
  const layerRef = useRef<HTMLDivElement>(null);

  const { panes, dividers } = useMemo(() => {
    const panes: PaneBox[] = [];
    const dividers: DividerBox[] = [];
    walk(tab.root, { x: 0, y: 0, w: 1, h: 1 }, [], overrides, panes, dividers);
    return { panes, dividers };
  }, [tab.root, overrides]);

  const startDrag = (d: DividerBox, downEvent: React.PointerEvent) => {
    downEvent.preventDefault();
    const layer = layerRef.current;
    if (!layer) return;
    const layerSize = d.dir === "row" ? layer.clientWidth : layer.clientHeight;
    const startCoord = d.dir === "row" ? downEvent.clientX : downEvent.clientY;
    const key = d.path.join(".");
    const base = [...d.ratios];

    const onMove = (e: PointerEvent) => {
      const coord = d.dir === "row" ? e.clientX : e.clientY;
      // Delta as a fraction of the owning split's span.
      const delta = (coord - startCoord) / (layerSize * d.span);
      const next = [...base];
      const moved = Math.max(
        -base[d.index - 1] + 0.05,
        Math.min(base[d.index] - 0.05, delta),
      );
      next[d.index - 1] = base[d.index - 1] + moved;
      next[d.index] = base[d.index] - moved;
      setOverrides((m) => new Map(m).set(key, next));
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      setOverrides((m) => {
        const ratios = m.get(key);
        if (ratios) void setSplitRatios(tab.id, d.path, ratios);
        return m;
      });
      // Overrides are cleared when the authoritative snapshot arrives.
      setTimeout(() => setOverrides(new Map()), 150);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  // The tab's agents as the wall, drawn over the whole layer. Its panes
  // stay mounted under it, hidden as a zoom hides them; the wall borrows
  // the agents' terminals while it is on screen. The board shows cards.
  const walled = !board && tab.wall;
  // One pane shown alone: it fills the layer, and the rest stay mounted at
  // their own size but out of sight — hidden, not unmounted, so terminals
  // keep their buffers, and not resized, so their PTYs are left alone.
  // The board shows every pane.
  const zoomed =
    !board &&
    !walled &&
    tab.zoomedPane &&
    panes.some((p) => p.paneId === tab.zoomedPane)
      ? tab.zoomedPane
      : null;
  const FULL: Frac = { x: 0, y: 0, w: 1, h: 1 };

  // A card in the hand: where the pointer has it. A move draws the card
  // there; a resize draws an outline and leaves the card as it is, so its
  // terminal refits — and its PTY resizes — once, when it is dropped.
  const [held, setHeld] = useState<{
    paneId: string;
    rect: BoardRect;
    resize: boolean;
  } | null>(null);

  /** Moves the card by its header (`edges` ""), or resizes it by `edges`. */
  const dragCard = (e: React.PointerEvent, card: CardRect, edges: string) => {
    void focusPane(card.paneId);
    const resize = edges !== "";
    let rect: BoardRect = card;
    boardDrag(
      e,
      (dx, dy) => {
        rect = resize ? resized(card, edges, dx, dy) : moved(card, dx, dy);
        setHeld({ paneId: card.paneId, rect, resize });
      },
      (dropped) => {
        if (!dropped) return setHeld(null);
        // Held until the snapshot says where Rust put it: there, or back
        // where it was if the place was taken.
        void setCardRect(card.paneId, rect).finally(() =>
          setTimeout(() => setHeld(null), 150),
        );
      },
    );
  };

  return (
    <div className="split-layer" ref={layerRef}>
      {panes.map((p) => {
        const hidden = walled || (zoomed !== null && p.paneId !== zoomed);
        const shown = active && !hidden;
        // On the board every tab is shown, and each has a focused pane:
        // only the active tab's takes the keyboard there.
        const focused = p.paneId === tab.focusedPane && (current || !board);
        const card = board
          ? tab.cards.find((c) => c.paneId === p.paneId)
          : undefined;
        const inHand = held?.paneId === p.paneId;
        const browser = browserPanes.find((b) => b.paneId === p.paneId);
        const diff = diffPanes.find((d) => d.paneId === p.paneId);
        const news = whatsNewPanes.find((w) => w.paneId === p.paneId);
        const graph = graphPanes.find((g) => g.paneId === p.paneId);
        const editor = agentEditorPanes.find((e) => e.paneId === p.paneId);
        return (
          <div
            key={p.paneId}
            data-pane={p.paneId}
            className={`pane-slot${card ? " card" : ""}${inHand ? " dragging" : ""}`}
            style={
              card
                ? px(inHand && !held.resize ? held.rect : card)
                : {
                    ...frac(p.paneId === zoomed ? FULL : p.rect),
                    ...(hidden
                      ? { visibility: "hidden", pointerEvents: "none" }
                      : {}),
                  }
            }
            // Below 50% a card's content takes no pointer; a click on it
            // still lands here, and makes it the pane the keyboard types to.
            onMouseDown={card ? (e) => pressCard(e, p.paneId) : undefined}
          >
            {card && (
              <CardHeader
                card={card}
                focused={focused}
                unread={unreadPanes.includes(p.paneId)}
                agent={agents.find((a) => a.paneId === p.paneId)}
                onPointerDown={(e) => dragCard(e, card, "")}
              />
            )}
            <div className={`pane-body${browser ? " browser" : ""}`}>
              {editor ? (
                <AgentEditor
                  paneId={p.paneId}
                  cwd={editor.cwd}
                  focused={focused}
                />
              ) : browser ? (
                <BrowserPane
                  paneId={p.paneId}
                  url={browser.url}
                  focused={focused}
                  board={board}
                />
              ) : graph ? (
                <GraphPane
                  paneId={p.paneId}
                  repo={graph.repo}
                  focused={focused}
                />
              ) : news ? (
                <WhatsNewPane
                  paneId={p.paneId}
                  version={news.version}
                  since={news.since}
                  focused={focused}
                />
              ) : diff ? (
                <DiffPane
                  paneId={p.paneId}
                  repo={diff.repo}
                  spec={diff.spec}
                  focused={focused}
                />
              ) : (
                <TerminalPane
                  paneId={p.paneId}
                  focused={focused}
                  visible={shown}
                  onBoard={board}
                  unread={unreadPanes.includes(p.paneId)}
                  agentCommand={
                    agentPanes.find((a) => a.paneId === p.paneId)?.command
                  }
                  remoteHost={
                    remotePanes.find((r) => r.paneId === p.paneId)?.host
                  }
                />
              )}
            </div>
            {card &&
              EDGES.map((edges) => (
                <div
                  key={edges}
                  className={`card-edge ${edges}`}
                  onPointerDown={(e) => dragCard(e, card, edges)}
                />
              ))}
          </div>
        );
      })}
      {held?.resize && <div className="card-ghost" style={px(held.rect)} />}
      {walled && (
        <div className="pane-slot" style={frac(FULL)}>
          <div className="pane-body">
            <AgentWall tabId={tab.id} visible={active} />
          </div>
        </div>
      )}
      {!zoomed &&
        !walled &&
        !board &&
        dividers.map((d) => (
          <div
            key={`${d.path.join(".")}:${d.index}`}
            className={`divider ${d.dir}`}
            style={dividerStyle(d)}
            onPointerDown={(e) => startDrag(d, e)}
          />
        ))}
    </div>
  );
}

function walk(
  node: Node,
  rect: Frac,
  path: number[],
  overrides: Map<string, number[]>,
  panes: PaneBox[],
  dividers: DividerBox[],
) {
  if (node.type === "leaf") {
    panes.push({ paneId: node.paneId, rect });
    return;
  }
  const ratios = overrides.get(path.join(".")) ?? node.ratios;
  let offset = 0;
  node.children.forEach((child, i) => {
    const r = ratios[i] ?? 0;
    const childRect: Frac =
      node.dir === "row"
        ? { x: rect.x + offset * rect.w, y: rect.y, w: r * rect.w, h: rect.h }
        : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: r * rect.h };
    if (i > 0) {
      dividers.push({
        path,
        index: i,
        dir: node.dir,
        ratios,
        pos: childRect,
        span: node.dir === "row" ? rect.w : rect.h,
        start: node.dir === "row" ? rect.x : rect.y,
      });
    }
    walk(child, childRect, [...path, i], overrides, panes, dividers);
    offset += r;
  });
}

function frac(r: Frac): React.CSSProperties {
  return {
    left: `${r.x * 100}%`,
    top: `${r.y * 100}%`,
    width: `${r.w * 100}%`,
    height: `${r.h * 100}%`,
  };
}

function px(r: BoardRect): React.CSSProperties {
  return { left: r.x, top: r.y, width: r.w, height: r.h };
}

/** `r` moved by (dx, dy), kept inside its frame. */
function moved(r: BoardRect, dx: number, dy: number): BoardRect {
  return {
    x: Math.max(FRAME_PADDING, r.x + dx),
    y: Math.max(FRAME_HEADER, r.y + dy),
    w: r.w,
    h: r.h,
  };
}

/** `r` with its `edges` ("n", "se", …) pulled by (dx, dy). */
function resized(r: BoardRect, edges: string, dx: number, dy: number): BoardRect {
  let { x, y, w, h } = r;
  if (edges.includes("e")) w = Math.max(MIN_CARD_WIDTH, r.w + dx);
  if (edges.includes("s")) h = Math.max(MIN_CARD_HEIGHT, r.h + dy);
  if (edges.includes("w")) {
    const right = r.x + r.w;
    x = Math.min(Math.max(FRAME_PADDING, r.x + dx), right - MIN_CARD_WIDTH);
    w = right - x;
  }
  if (edges.includes("n")) {
    const bottom = r.y + r.h;
    y = Math.min(Math.max(FRAME_HEADER, r.y + dy), bottom - MIN_CARD_HEIGHT);
    h = bottom - y;
  }
  return { x, y, w, h };
}

/**
 * A press on a card makes it the focused pane, and leaves its content
 * holding the keyboard.
 */
function pressCard(e: React.MouseEvent, paneId: string) {
  void focusPane(paneId);
  // From 50% up a press in the content is the content's: a terminal takes
  // the keyboard itself, and a selection starts there.
  if ((e.target as Element).closest(".pane-body")) return;
  // Anywhere else — all of the card, below 50% — the press's default
  // blurs whatever holds the keyboard. When that is this card's own
  // terminal, nothing gives it back: focusing the focused pane changes no
  // prop. So the press blurs nothing, and the card hands over the keyboard.
  e.preventDefault();
  const term = isLent(paneId) ? undefined : getTerminal(paneId);
  if (term) return term.focus();
  // A pane with no terminal keeps what holds the keyboard in its own
  // content (a search box); anything else's is let go, or keys would go
  // on to another card's terminal.
  const holder = document.activeElement as HTMLElement | null;
  const body = e.currentTarget.querySelector(".pane-body");
  if (holder && !body?.contains(holder)) holder.blur();
}

/**
 * A card's title bar: what the pane is, and an agent's status. Dragged, it
 * moves the card; double-clicked, it goes to the card at 100%.
 */
function CardHeader({
  card,
  focused,
  unread,
  agent,
  onPointerDown,
}: {
  card: CardRect;
  focused: boolean;
  unread: boolean;
  agent: AgentInfo | undefined;
  onPointerDown: (e: React.PointerEvent) => void;
}) {
  const status = agent?.status;
  return (
    <div
      className={`card-header${focused ? " focused" : ""}${unread ? " unread" : ""}${status ? ` ${status}` : ""}`}
      title={card.title}
      onPointerDown={onPointerDown}
      onDoubleClick={() => flyToCard(card.paneId)}
    >
      {status && <span className="card-dot" />}
      <span className="card-title">{agent?.name ?? card.title}</span>
      <button
        className="card-close"
        title="Close this pane"
        // The header drags the card; its button must not.
        onPointerDown={(e) => e.stopPropagation()}
        onClick={() => void closePane(card.paneId)}
      >
        ×
      </button>
    </div>
  );
}

function dividerStyle(d: DividerBox): React.CSSProperties {
  if (d.dir === "row") {
    return {
      left: `calc(${d.pos.x * 100}% - 4px)`,
      top: `${d.pos.y * 100}%`,
      width: "8px",
      height: `${d.pos.h * 100}%`,
    };
  }
  return {
    left: `${d.pos.x * 100}%`,
    top: `calc(${d.pos.y * 100}% - 4px)`,
    width: `${d.pos.w * 100}%`,
    height: "8px",
  };
}
