import { setWallMode, setWallView } from "../bindings";
import { useUiStore } from "../state/uiStore";

/** Shows the agent in `paneId` on its tab's wall, large in Focus and
 *  pinned there: what clicking it in the sidebar does. */
export function showOnWall(tabId: string, paneId: string) {
  useUiStore.getState().pinAgent(tabId, paneId);
  void (async () => {
    await setWallMode(paneId, "focus");
    await setWallView(paneId, "agents");
  })();
}
