import { memo, useEffect, useState } from "react";
import { NoteBlock, closePane, focusPane, readMarkdown } from "../bindings";
import { useConfigStore } from "../state/configStore";
import { BlockView, ExternalLink } from "../notes/NoteBlocks";

type Props = {
  paneId: string;
  /** Absolute path of the file. */
  path: string;
  focused: boolean;
};

/** A web image is not fetched here: the page only fetches release-note
 *  images, from GitHub, through Rust. Its alt text links to it instead. */
const AltText = ({ alt, url }: { alt: string; url: string }) => (
  <p>
    <ExternalLink href={url}>{alt || "image"}</ExternalLink>
  </p>
);

/**
 * A local markdown file — a README, a plan, an agent's notes — read and
 * parsed in Rust (cmux-core/src/notes.rs), drawn as the What's New pane
 * draws release notes, with its chrome and its prose styles.
 */
export function MarkdownPane({ paneId, path, focused }: Props) {
  const [blocks, setBlocks] = useState<NoteBlock[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const colors = useConfigStore((s) => s.config?.colors);
  const name = path.split(/[\\/]/).pop() || path;

  useEffect(() => {
    let stale = false;
    setBlocks(null);
    setError(null);
    void readMarkdown(paneId)
      .then((b) => {
        if (!stale) setBlocks(b);
      })
      .catch((e: unknown) => {
        if (!stale) setError(String(e));
      });
    return () => {
      stale = true;
    };
  }, [paneId, path]);

  return (
    <div
      className={`pane whatsnew-pane${focused ? " focused" : ""}`}
      style={
        colors
          ? ({
              "--term-bg": colors.background,
              "--term-fg": colors.foreground,
            } as React.CSSProperties)
          : undefined
      }
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="whatsnew-chrome">
        <span className="whatsnew-badge">Markdown</span>
        <span className="whatsnew-title" title={path}>
          {name}
        </span>
        <button
          className="whatsnew-close markdown-close"
          title="Close"
          onClick={() => void closePane(paneId)}
        >
          ✕
        </button>
      </div>

      <div className="whatsnew-body">
        {error ? (
          <p className="whatsnew-error">{error}</p>
        ) : !blocks ? (
          <p className="whatsnew-loading">Loading {name}…</p>
        ) : blocks.length === 0 ? (
          <p className="whatsnew-loading">{name} is empty.</p>
        ) : (
          <article className="whatsnew-notes">
            {blocks.map((block, i) => (
              <BlockView key={i} block={block} depth={1} Image={AltText} />
            ))}
          </article>
        )}
      </div>
    </div>
  );
}

export default memo(MarkdownPane);
