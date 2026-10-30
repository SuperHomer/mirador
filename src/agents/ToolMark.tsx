import { ToolKind } from "../bindings";

/** What a tool is, in the same neutral line as the harness marks. */
export function ToolMark({ kind }: { kind: ToolKind }) {
  const props = {
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.5,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  switch (kind) {
    case "terminal":
      return (
        <svg {...props}>
          <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
          <path d="M4.5 6l2 2-2 2M8.5 10.5h3" />
        </svg>
      );
    case "browser":
      return (
        <svg {...props}>
          <circle cx="8" cy="8" r="6.5" />
          <path d="M1.5 8h13M8 1.5c-2.2 2.2-2.2 10.8 0 13M8 1.5c2.2 2.2 2.2 10.8 0 13" />
        </svg>
      );
    case "diff":
      return (
        <svg {...props}>
          <path d="M5 2v6M2 5h6M2 12.5h6M10.5 3.5h3.5v9" />
        </svg>
      );
    case "graph":
      return (
        <svg {...props}>
          <circle cx="4" cy="3.5" r="1.5" />
          <circle cx="4" cy="12.5" r="1.5" />
          <circle cx="12" cy="6" r="1.5" />
          <path d="M4 5v6M12 7.5c0 2.5-3 3-8 3.5" />
        </svg>
      );
  }
}
