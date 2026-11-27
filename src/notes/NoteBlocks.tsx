import type { ComponentType } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { NoteBlock, NoteSpan } from "../bindings";

/** How a pane shows an image block: What's New fetches it, a markdown pane
 *  shows its alt text. */
export type ImageView = ComponentType<{ alt: string; url: string }>;

/**
 * Markdown parsed in Rust (cmux-core/src/notes.rs) into blocks of plain
 * text, mapped to elements. Nothing here interprets text as markup: release
 * notes and files on disk alike are only ever strings React escapes.
 *
 * `depth` is how many headings sit above the blocks — the pane's header
 * alone, or a section title as well — so a `#` in the text ranks below them.
 */
export const BlockView = ({
  block,
  depth,
  Image,
}: {
  block: NoteBlock;
  depth: number;
  Image: ImageView;
}) => {
  switch (block.kind) {
    case "heading": {
      const Tag = `h${Math.min(block.level + depth, 4)}` as "h2" | "h3" | "h4";
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
    case "list": {
      const List = block.ordered ? "ol" : "ul";
      return (
        <List>
          {block.items.map((item, i) => (
            <li key={i}>
              <Spans spans={item} />
            </li>
          ))}
        </List>
      );
    }
    case "quote":
      return (
        <blockquote>
          {block.blocks.map((inner, i) => (
            <BlockView key={i} block={inner} depth={depth} Image={Image} />
          ))}
        </blockquote>
      );
    case "code":
      return (
        <pre className="whatsnew-code">
          <code>{block.text}</code>
        </pre>
      );
    case "image":
      return <Image alt={block.alt} url={block.url} />;
    case "rule":
      return <hr />;
  }
};

/** A link that opens in the real browser, never in the app's webview. */
export const ExternalLink = ({ href, children }: { href: string; children: React.ReactNode }) => (
  <a
    href={href}
    title={href}
    onClick={(e) => {
      e.preventDefault();
      void openUrl(href);
    }}
  >
    {children}
  </a>
);

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
            <ExternalLink key={i} href={span.href}>
              {span.text}
            </ExternalLink>
          );
        default:
          return <span key={i}>{span.text}</span>;
      }
    })}
  </>
);
