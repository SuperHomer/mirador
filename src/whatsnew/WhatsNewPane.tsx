import { memo, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  NoteBlock,
  NoteSpan,
  ReleaseNotes,
  closePane,
  focusPane,
  releaseImage,
  whatsNew,
} from "../bindings";
import { useConfigStore } from "../state/configStore";

type Props = {
  paneId: string;
  version: string;
  /** The version updated from, when the pane covers a skipped range. */
  since: string | null;
  focused: boolean;
};

/**
 * Release notes for one version, or for every release an update skipped,
 * newest first. The markdown is parsed in Rust (see cmux-core/src/notes.rs)
 * into blocks of plain text, so this component only ever maps data to
 * elements — remote note text is never interpreted as markup in the
 * webview that holds the IPC bridge.
 */
export function WhatsNewPane({ paneId, version, since, focused }: Props) {
  const [releases, setReleases] = useState<ReleaseNotes[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const colors = useConfigStore((s) => s.config?.colors);

  useEffect(() => {
    let stale = false;
    void whatsNew(version, since)
      .then((r) => {
        if (!stale) setReleases(r);
      })
      .catch((e: unknown) => {
        if (!stale) setError(String(e));
      });
    return () => {
      stale = true;
    };
  }, [version, since]);

  // A range of one is the ordinary pane: the release is the header. Several
  // get a header naming the span, and a section — title and link — each.
  const single = releases?.length === 1 ? releases[0] : null;
  const title = !releases
    ? `v${version}`
    : single
      ? single.title
      : `${releases.length} releases since v${since}`;

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
        <span className="whatsnew-badge">What's New</span>
        <span className="whatsnew-title" title={title}>
          {title}
        </span>
        {single && <GitHubLink url={single.url} />}
        <button
          className="whatsnew-close"
          title="Close"
          onClick={() => void closePane(paneId)}
        >
          ✕
        </button>
      </div>

      <div className="whatsnew-body">
        {error ? (
          <p className="whatsnew-error">
            Release notes for v{version} could not be loaded: {error}
          </p>
        ) : !releases ? (
          <p className="whatsnew-loading">Loading release notes…</p>
        ) : single ? (
          <article className="whatsnew-notes">
            {single.blocks.map((block, i) => (
              <BlockView key={i} block={block} depth={1} />
            ))}
          </article>
        ) : (
          <article className="whatsnew-notes">
            {releases.map((release) => (
              <section className="whatsnew-release" key={release.version}>
                <div className="whatsnew-release-head">
                  <h2>{release.title}</h2>
                  <GitHubLink url={release.url} />
                </div>
                {release.blocks.map((block, i) => (
                  <BlockView key={i} block={block} depth={2} />
                ))}
              </section>
            ))}
          </article>
        )}
      </div>
    </div>
  );
}

const GitHubLink = ({ url }: { url: string }) => (
  <button
    className="whatsnew-link"
    title="Open the release on GitHub"
    onClick={() => void openUrl(url)}
  >
    GitHub ↗
  </button>
);

/**
 * `depth` is how many headings sit above the note: the pane's header alone
 * for one release, a section title as well for several.
 */
const BlockView = ({ block, depth }: { block: NoteBlock; depth: number }) => {
  switch (block.kind) {
    case "heading": {
      // The release title is already a heading above this one, so a
      // note's own top-level heading ranks below it.
      const Tag = (`h${Math.min(block.level + depth, 4)}` as "h2" | "h3" | "h4");
      return (
        <Tag>
          <Spans spans={block.spans} />
        </Tag>
      );
    }
    case "paragraph":
      return (
        <p>
          <Spans spans={block.spans} />
        </p>
      );
    case "list":
      return (
        <ul>
          {block.items.map((item, i) => (
            <li key={i}>
              <Spans spans={item} />
            </li>
          ))}
        </ul>
      );
    case "code":
      return (
        <pre className="whatsnew-code">
          <code>{block.text}</code>
        </pre>
      );
    case "image":
      return <NoteImage alt={block.alt} url={block.url} />;
    case "rule":
      return <hr />;
  }
};

/**
 * The same formats the backend will store, recognised from the bytes
 * rather than from the url — a release note's image has no file extension
 * when it was uploaded to GitHub.
 */
function imageType(bytes: Uint8Array): string | null {
  const starts = (sig: number[]) => sig.every((b, i) => bytes[i] === b);
  if (starts([0x89, 0x50, 0x4e, 0x47])) return "image/png";
  if (starts([0x47, 0x49, 0x46, 0x38])) return "image/gif";
  if (starts([0xff, 0xd8, 0xff])) return "image/jpeg";
  if (starts([0x52, 0x49, 0x46, 0x46]) && bytes.length > 12) {
    const tag = String.fromCharCode(...bytes.slice(8, 12));
    if (tag === "WEBP") return "image/webp";
  }
  if (bytes.length > 12) {
    const brand = String.fromCharCode(...bytes.slice(4, 12));
    if (brand === "ftypavif") return "image/avif";
  }
  return null;
}

/**
 * A screenshot or GIF from the notes.
 *
 * The bytes come over IPC and become a blob, so the page never holds a
 * remote url and the privileged webview never makes the request. If they
 * do not arrive — offline before it was ever cached, too large, or not an
 * image — the alt text becomes a link, which is what the note would have
 * shown anyway.
 */
const NoteImage = ({ alt, url }: { alt: string; url: string }) => {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let stale = false;
    let objectUrl: string | null = null;
    setSrc(null);
    setFailed(false);
    void releaseImage(url)
      .then((buffer) => {
        if (stale) return;
        const bytes = new Uint8Array(buffer);
        const type = imageType(bytes);
        if (!type) {
          setFailed(true);
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes], { type }));
        setSrc(objectUrl);
      })
      .catch(() => {
        if (!stale) setFailed(true);
      });
    return () => {
      stale = true;
      // Freed on unmount, or a pane left open on a GIF holds it forever.
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [url]);

  if (failed) {
    return (
      <p>
        <a
          href={url}
          title={url}
          onClick={(e) => {
            e.preventDefault();
            void openUrl(url);
          }}
        >
          {alt || "image"}
        </a>
      </p>
    );
  }
  return (
    <figure className="whatsnew-figure">
      {src ? (
        <img src={src} alt={alt} />
      ) : (
        <span className="whatsnew-figure-loading">{alt || "image"}</span>
      )}
    </figure>
  );
};

const Spans = ({ spans }: { spans: NoteSpan[] }) => (
  <>
    {spans.map((span, i) => {
      switch (span.kind) {
        case "code":
          return <code key={i}>{span.text}</code>;
        case "strong":
          return <strong key={i}>{span.text}</strong>;
        case "em":
          return <em key={i}>{span.text}</em>;
        case "link":
          return (
            // Opened in the real browser: a release note's links go to
            // GitHub and the web, not into a terminal's webview.
            <a
              key={i}
              href={span.href}
              title={span.href}
              onClick={(e) => {
                e.preventDefault();
                void openUrl(span.href);
              }}
            >
              {span.text}
            </a>
          );
        default:
          return <span key={i}>{span.text}</span>;
      }
    })}
  </>
);

export default memo(WhatsNewPane);
