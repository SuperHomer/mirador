import { create } from "zustand";
import { Camera, boardCardCamera, boardFit, setCamera } from "../bindings";

/** How long a fly-to (fit, a card at 100%) takes. */
const FLY_MS = 220;
/** Quiet time before a moved camera is saved. */
const SAVE_DELAY_MS = 400;

interface BoardStore {
  /**
   * Where the board is looked at. Taken from the snapshot once, then owned
   * here: the snapshot's copy is only what was last saved, and a pan
   * re-sending it would fight the pointer.
   */
  camera: Camera | null;
  /** The board's size on screen, for fitting and centring. */
  viewport: { width: number; height: number };
  setViewport: (width: number, height: number) => void;
  /** Moves the camera now, and saves it once it settles. */
  moveTo: (camera: Camera) => void;
  /** Moves the camera there smoothly. */
  flyTo: (camera: Camera) => void;
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
let flight = 0;

export const useBoardStore = create<BoardStore>((set, get) => ({
  camera: null,
  viewport: { width: 0, height: 0 },
  setViewport: (width, height) => set({ viewport: { width, height } }),
  moveTo: (camera) => {
    flight++;
    set({ camera });
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void setCamera(camera), SAVE_DELAY_MS);
  },
  flyTo: (target) => {
    const from = get().camera ?? target;
    const id = ++flight;
    const start = performance.now();
    const step = (now: number) => {
      // A pan or another flight since: that one wins.
      if (id !== flight) return;
      const t = Math.min(1, (now - start) / FLY_MS);
      const e = 1 - (1 - t) ** 3;
      const camera = {
        x: from.x + (target.x - from.x) * e,
        y: from.y + (target.y - from.y) * e,
        zoom: from.zoom + (target.zoom - from.zoom) * e,
      };
      if (t < 1) {
        set({ camera });
        requestAnimationFrame(step);
      } else {
        get().moveTo(target);
      }
    };
    requestAnimationFrame(step);
  },
}));

/** Shows the whole board. */
export function fitBoard() {
  const { viewport, flyTo } = useBoardStore.getState();
  if (viewport.width === 0) return;
  void boardFit(viewport.width, viewport.height).then(flyTo);
}

/** Shows a pane's card at 100%, where its content takes the pointer. */
export function flyToCard(paneId: string) {
  const { viewport, flyTo } = useBoardStore.getState();
  if (viewport.width === 0) return;
  void boardCardCamera(paneId, viewport.width, viewport.height).then(flyTo);
}
