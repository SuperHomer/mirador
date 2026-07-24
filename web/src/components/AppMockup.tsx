const chip = (bg: string, color: string): React.CSSProperties => ({
  fontSize: 9.5,
  fontWeight: 600,
  borderRadius: 4,
  padding: "1px 5px",
  background: bg,
  color,
});

export default function AppMockup() {
  return (
    <div style={{ marginTop: 56, textAlign: "left" }}>
      <div
        style={{
          maxWidth: 1000,
          margin: "0 auto",
          border: "1px solid var(--surface)",
          borderRadius: 12,
          overflow: "hidden",
          background: "var(--bg)",
          boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "9px 14px",
            background: "var(--bg-alt)",
            borderBottom: "1px solid var(--surface)",
          }}
        >
          <span style={{ width: 11, height: 11, borderRadius: "50%", background: "var(--red)" }} />
          <span style={{ width: 11, height: 11, borderRadius: "50%", background: "var(--yellow)" }} />
          <span style={{ width: 11, height: 11, borderRadius: "50%", background: "var(--green)" }} />
          <span className="mono" style={{ marginLeft: 12, fontSize: 12, color: "var(--muted)" }}>
            mirador — ~/work/api
          </span>
        </div>
        <div style={{ display: "flex", height: 396 }}>
          {/* sidebar */}
          <div
            style={{
              width: 216,
              flexShrink: 0,
              background: "var(--bg-alt)",
              borderRight: "1px solid var(--surface)",
              display: "flex",
              flexDirection: "column",
              padding: 6,
              gap: 2,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 8, borderRadius: 6, background: "var(--surface)", minHeight: 40 }}>
              <span style={{ fontSize: 10, color: "var(--muted)", width: 12, textAlign: "center" }}>1</span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 12.5, fontWeight: 600, color: "var(--accent)" }}>agent · api</div>
                <div style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 3 }}>
                  <span style={{ fontSize: 10, color: "var(--subtext)" }}>feat/auth</span>
                  <span style={chip("rgba(166,227,161,.25)", "var(--green)")}>✓ CI</span>
                  <span style={chip("var(--surface)", "var(--accent)")}>:3000</span>
                </div>
              </div>
              <span style={{ background: "var(--accent)", color: "var(--bg-alt)", fontSize: 10, fontWeight: 700, borderRadius: 8, minWidth: 16, height: 16, lineHeight: "16px", textAlign: "center" }}>2</span>
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 8, borderRadius: 6, minHeight: 40 }}>
              <span style={{ fontSize: 10, color: "var(--muted)", width: 12, textAlign: "center" }}>2</span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 12.5, fontWeight: 500 }}>tests</div>
                <div style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 3 }}>
                  <span style={{ fontSize: 10, color: "var(--subtext)" }}>main</span>
                  <span style={chip("rgba(249,226,175,.25)", "var(--yellow)")}>● PR</span>
                </div>
              </div>
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 8, borderRadius: 6, minHeight: 40 }}>
              <span style={{ fontSize: 10, color: "var(--muted)", width: 12, textAlign: "center" }}>3</span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 12.5, fontWeight: 500 }}>prod-box</div>
                <div style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 3 }}>
                  <span style={chip("rgba(203,166,247,.22)", "var(--mauve)")}>ssh</span>
                  <span style={{ fontSize: 10, color: "var(--subtext)" }}>:8080</span>
                </div>
              </div>
            </div>
            <div style={{ marginTop: 4, padding: "7px 8px", fontSize: 12, color: "var(--muted)" }}>+ new tab</div>
          </div>
          {/* panes */}
          <div style={{ flex: 1, display: "flex", gap: 6, padding: 6, minWidth: 0 }}>
            <div className="mono" style={{ flex: 1, border: "1px solid var(--surface)", borderRadius: 5, background: "var(--crust)", padding: 12, fontSize: 11.5, lineHeight: 1.65, overflow: "hidden" }}>
              <div style={{ color: "var(--green)" }}>➜ <span style={{ color: "var(--accent)" }}>api</span> mira run --wait npm test</div>
              <div style={{ color: "var(--subtext)" }}>&nbsp;&nbsp;PASS  auth.spec.ts <span style={{ color: "var(--muted)" }}>(1.2s)</span></div>
              <div style={{ color: "var(--subtext)" }}>&nbsp;&nbsp;PASS  token.spec.ts <span style={{ color: "var(--muted)" }}>(0.4s)</span></div>
              <div style={{ color: "var(--green)" }}>&nbsp;&nbsp;42 passing</div>
              <div style={{ color: "var(--muted)", marginTop: 6 }}>— agent handed back exit 0 —</div>
              <div style={{ marginTop: 6, color: "var(--text)" }}>➜ claude "wire up refresh flow"<span className="cursor" style={{ width: 7, height: 14, verticalAlign: "middle", marginLeft: 2 }} /></div>
            </div>
            <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
              <div style={{ flex: 1, border: "1px solid var(--surface)", borderRadius: 5, background: "var(--bg-alt)", overflow: "hidden", display: "flex", flexDirection: "column" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 8px", borderBottom: "1px solid var(--surface)" }}>
                  <span style={{ color: "var(--muted)", fontSize: 13 }}>‹ ›</span>
                  <div className="mono" style={{ flex: 1, background: "var(--crust)", border: "1px solid var(--surface)", borderRadius: 6, fontSize: 10.5, color: "var(--subtext)", padding: "3px 9px" }}>localhost:3000/login</div>
                </div>
                <div style={{ flex: 1, background: "repeating-linear-gradient(135deg,#20202f,#20202f 9px,#1b1b28 9px,#1b1b28 18px)", display: "flex", alignItems: "center", justifyContent: "center" }}>
                  <span className="mono" style={{ fontSize: 11, color: "var(--muted)" }}>browser pane · scriptable</span>
                </div>
              </div>
              <div className="mono" style={{ flex: 1, borderRadius: 5, background: "var(--crust)", padding: 11, fontSize: 11, lineHeight: 1.6, border: "1px solid var(--accent)", boxShadow: "0 0 0 1px var(--accent), 0 0 16px rgba(137,180,250,.45)", overflow: "hidden" }}>
                <div style={{ color: "var(--accent)" }}>⏻ agent needs you</div>
                <div style={{ color: "var(--subtext)" }}>approve migration? (y/n)</div>
                <div style={{ color: "var(--muted)", marginTop: 2 }}>osc 777 · attention ring lit</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
