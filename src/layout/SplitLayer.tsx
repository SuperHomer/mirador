import { useMemo, useRef, useState } from "react";
import { Node, SplitDir, TabSnapshot, setSplitRatios } from "../bindings";
import { TerminalPane } from "../terminal/TerminalPane";

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
 */
export function SplitLayer({ tab }: { tab: TabSnapshot }) {
  // Live ratio overrides while a divider drag is in flight.
  const [overrides, setOverrides] = useState<Map<string, number[]>>(new Map());
  const layerRef = useRef<HTMLDivElement>(null);

  const { panes, dividers } = useMemo(() => {
    const panes: PaneBox[] = [];
    const dividers: DividerBox[] = [];
    walk(
      tab.root,
      { x: 0, y: 0, w: 1, h: 1 },
      [],
      overrides,
      panes,
      dividers,
    );
    return { panes, dividers };
  }, [tab.root, overrides]);

  const startDrag = (d: DividerBox, downEvent: React.PointerEvent) => {
    downEvent.preventDefault();
    const layer = layerRef.current;
    if (!layer) return;
    const layerSize =
      d.dir === "row" ? layer.clientWidth : layer.clientHeight;
    const startCoord =
      d.dir === "row" ? downEvent.clientX : downEvent.clientY;
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

  return (
    <div className="split-layer" ref={layerRef}>
      {panes.map((p) => (
        <div key={p.paneId} className="pane-slot" style={frac(p.rect)}>
          <TerminalPane
            paneId={p.paneId}
            focused={p.paneId === tab.focusedPane}
          />
        </div>
      ))}
      {dividers.map((d) => (
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
