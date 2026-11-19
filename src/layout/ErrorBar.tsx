import { useUiStore } from "../state/uiStore";

/**
 * What the person last asked for that failed (`reportFailure`), until they
 * dismiss it or the next failure replaces it. Nothing is drawn otherwise.
 */
export function ErrorBar() {
  const error = useUiStore((s) => s.error);
  const dismiss = () => useUiStore.getState().setError(null);
  if (!error) return null;
  return (
    <div className="editor-bar error app-error" role="alert">
      <span>{error}</span>
      <button type="button" aria-label="Dismiss" title="Dismiss" onClick={dismiss}>
        ×
      </button>
    </div>
  );
}
