import { useState } from "react";

export default function CopyButton({
  text,
  idle = "Copy commands",
  done = "Copied \u2713",
  style,
}: {
  text: string;
  idle?: string;
  done?: string;
  style?: React.CSSProperties;
}) {
  const [copied, setCopied] = useState(false);
  const onClick = () => {
    navigator.clipboard?.writeText(text).catch(() => {});
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1600);
  };
  return (
    <button onClick={onClick} style={style}>
      {copied ? done : idle}
    </button>
  );
}
