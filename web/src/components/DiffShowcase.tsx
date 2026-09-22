const ADD_BG = "rgba(166,227,161,.13)";
const DEL_BG = "rgba(243,139,168,.13)";
const ADD_NUM_BG = "rgba(166,227,161,.22)";
const DEL_NUM_BG = "rgba(243,139,168,.22)";

type Row = { old?: number; now?: number; kind: "ctx" | "add" | "del"; text: string };

const ROWS: Row[] = [
  { old: 18, now: 18, kind: "ctx", text: "  const ttl = Number(env.TOKEN_TTL ?? 3600);" },
  { old: 19, kind: "del", text: "  return jwt.sign(payload, secret);" },
  { now: 19, kind: "add", text: "  return jwt.sign(payload, secret, {" },
  { now: 20, kind: "add", text: "    expiresIn: ttl," },
  { now: 21, kind: "add", text: "  });" },
  { old: 20, now: 22, kind: "ctx", text: "}" },
];

/** Status letter, as the real tree draws it. */
function Status({ letter }: { letter: "M" | "A" }) {
  return (
    <span
      className="mono"
      style={{
        width: 13,
        flexShrink: 0,
        textAlign: "center",
        fontSize: 9.5,
        fontWeight: 700,
        color: letter === "A" ? "var(--green)" : "var(--muted)",
      }}
    >
      {letter}
    </span>
  );
}

function Stat({ add, del }: { add: number; del: number }) {
  return (
    <span style={{ fontSize: 10, flexShrink: 0 }}>
      <span style={{ color: "var(--green)" }}>+{add}</span>{" "}
      <span style={{ color: "var(--red)" }}>−{del}</span>
    </span>
  );
}

function TreeFile({
  letter,
  name,
  add,
  del,
  selected,
}: {
  letter: "M" | "A";
  name: string;
  add: number;
  del: number;
  selected?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 5,
        padding: "2px 6px 2px 18px",
        borderRadius: 4,
        background: selected ? "var(--surface)" : undefined,
        color: selected ? "var(--text)" : "var(--subtext)",
        fontSize: 11,
        whiteSpace: "nowrap",
      }}
    >
      <Status letter={letter} />
      <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{name}</span>
      <span style={{ marginLeft: "auto" }}>
        <Stat add={add} del={del} />
      </span>
    </div>
  );
}

function DiffRow({ row }: { row: Row }) {
  const bg = row.kind === "add" ? ADD_BG : row.kind === "del" ? DEL_BG : undefined;
  const numBg =
    row.kind === "add" ? ADD_NUM_BG : row.kind === "del" ? DEL_NUM_BG : "var(--bg-alt)";
  const marker = row.kind === "add" ? "+" : row.kind === "del" ? "−" : " ";
  const num = (n?: number) => (
    <span
      style={{
        width: 30,
        flexShrink: 0,
        paddingRight: 6,
        textAlign: "right",
        background: numBg,
        color: row.kind === "ctx" ? "var(--muted)" : "var(--text)",
      }}
    >
      {n ?? ""}
    </span>
  );
  return (
    <div style={{ display: "flex", height: 17, lineHeight: "17px", background: bg }}>
      {num(row.old)}
      {num(row.now)}
      <span style={{ padding: "0 8px", color: "var(--text)", whiteSpace: "pre" }}>
        {marker}
        {row.text}
      </span>
    </div>
  );
}

/** The chrome row and the two columns, as the real pane lays them out. */
function DiffMockup() {
  return (
    <div
      style={{
        maxWidth: 940,
        margin: "44px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--bg)",
        boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        textAlign: "left",
      }}
    >
      {/* chrome */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "7px 10px",
          background: "var(--bg-alt)",
          borderBottom: "1px solid var(--surface)",
        }}
      >
        <span style={{ color: "var(--muted)", fontSize: 12 }}>☰</span>
        <span style={{ fontSize: 12, fontWeight: 600 }}>uncommitted changes</span>
        <span style={{ fontSize: 11, color: "var(--muted)" }}>
          3 files <Stat add={59} del={5} />
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 3 }}>
          <span
            style={{
              fontSize: 10.5,
              padding: "2px 8px",
              borderRadius: 5,
              background: "var(--surface)",
              color: "var(--text)",
            }}
          >
            Working tree
          </span>
          <span style={{ fontSize: 10.5, padding: "2px 8px", color: "var(--muted)" }}>
            Staged
          </span>
        </span>
        <span style={{ color: "var(--muted)", fontSize: 12 }}>⟳</span>
      </div>

      <div className="mono" style={{ display: "flex", height: 240 }}>
        {/* file tree */}
        <div
          style={{
            width: 216,
            flexShrink: 0,
            background: "var(--bg-alt)",
            borderRight: "1px solid var(--surface)",
            padding: "6px 4px",
            overflow: "hidden",
          }}
        >
          <div style={{ padding: "2px 6px", fontSize: 11, color: "var(--muted)" }}>
            ▾ src/auth
          </div>
          <TreeFile letter="M" name="token.ts" add={12} del={3} selected />
          <TreeFile letter="A" name="refresh.ts" add={38} del={0} />
          <div style={{ padding: "2px 6px", fontSize: 11, color: "var(--muted)", marginTop: 2 }}>
            ▾ tests
          </div>
          <TreeFile letter="M" name="auth.spec.ts" add={9} del={2} />
        </div>

        {/* hunks */}
        <div style={{ flex: 1, minWidth: 0, padding: 6, overflow: "hidden" }}>
          <div
            style={{
              border: "1px solid var(--accent)",
              borderRadius: 6,
              overflow: "hidden",
              marginBottom: 7,
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "5px 8px",
                background: "var(--bg-alt)",
                borderBottom: "1px solid var(--surface)",
                fontSize: 11,
              }}
            >
              <span style={{ color: "var(--muted)" }}>▾</span>
              <Status letter="M" />
              <span style={{ color: "var(--text)" }}>src/auth/token.ts</span>
              <span style={{ marginLeft: "auto" }}>
                <Stat add={12} del={3} />
              </span>
            </div>
            <div style={{ background: "var(--crust)", fontSize: 11 }}>
              <div
                style={{
                  display: "flex",
                  height: 17,
                  lineHeight: "17px",
                  background: "var(--surface)",
                  color: "var(--muted)",
                  padding: "0 8px",
                }}
              >
                @@ -18,6 +18,7 @@ export function sign(payload: Claims)
              </div>
              {ROWS.map((r, i) => (
                <DiffRow key={i} row={r} />
              ))}
            </div>
          </div>

          {/* the next file, to show the list continues */}
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "5px 8px",
              border: "1px solid var(--surface)",
              borderRadius: 6,
              background: "var(--bg-alt)",
              fontSize: 11,
            }}
          >
            <span style={{ color: "var(--muted)" }}>▾</span>
            <Status letter="A" />
            <span style={{ color: "var(--text)" }}>src/auth/refresh.ts</span>
            <span style={{ marginLeft: "auto" }}>
              <Stat add={38} del={0} />
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}

export default function DiffShowcase() {
  return (
    <section
      id="diff"
      style={{
        padding: "66px 48px",
        borderTop: "1px solid var(--surface)",
        background: "var(--crust)",
      }}
    >
      <div style={{ textAlign: "center", maxWidth: 720, margin: "0 auto" }}>
        <span
          className="mono"
          style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}
        >
          REVIEW
        </span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          See what the agent changed
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
          A turn ends and the work is buried in scrollback. Open a diff pane beside it
          instead: file tree on the left, hunks on the right, in your terminal's own
          theme. Untracked files are included — a file git has never seen is still work
          the agent just did.
        </p>
        <div
          style={{
            display: "flex",
            justifyContent: "center",
            alignItems: "center",
            gap: 10,
            marginTop: 22,
            fontSize: 13.5,
            color: "var(--subtext)",
          }}
        >
          <kbd
            className="mono"
            style={{
              background: "var(--bg-alt)",
              border: "1px solid var(--surface)",
              borderRadius: 6,
              padding: "4px 10px",
              fontSize: 12.5,
              color: "var(--text)",
            }}
          >
            ⌘G
          </kbd>
          <span>or</span>
          <span className="mono" style={{ color: "var(--text)" }}>
            mira diff
          </span>
          <span style={{ color: "var(--muted)" }}>· a commit, a range, --staged</span>
        </div>
      </div>
      <DiffMockup />
    </section>
  );
}
