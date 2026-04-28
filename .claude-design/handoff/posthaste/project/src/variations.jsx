// Variation artboards — quick visual direction explorations
// Each is a mini Posthaste mockup with a distinct aesthetic spin.

// Variation 1: "Cozy Workbench" — warm beige light, serif accents
function VariationCozy() {
  const T = {
    ...phTokens('light'),
    bg: '#f4ede0', bgSidebar: '#ebe2d0', bgList: '#f4ede0', bgReader: '#fbf7ed',
    bgTitlebar: '#e6dcc6', bgElev: '#ece2cd',
    border: '#d4c6a8', borderSoft: '#dccfb0',
    fg: '#3a2f1a', fgMuted: '#6b5a3a', fgSubtle: '#806c48', fgFaint: '#9d8962',
    accent: { ...phTokens('light').accent, coral: 'oklch(0.6 0.18 30)', coralDeep: 'oklch(0.45 0.18 25)', coralSoft: '#e6c9a3' },
  };
  return <MiniMock T={T} title="cozy workbench" serif />;
}

// Variation 2: "Terminal Warmth" — dark, monospace-heavy, amber
function VariationTerminal() {
  const T = {
    ...phTokens('dark'),
    bg: '#1d1a14', bgSidebar: '#17140f', bgList: '#1f1c15', bgReader: '#221e17',
    bgTitlebar: '#17140f', bgElev: '#262117',
    border: '#3a3425', borderSoft: '#2e2a1d',
    fg: '#e8dbb4', fgMuted: '#b09a6e', fgSubtle: '#89754d', fgFaint: '#665436',
    accent: { ...phTokens('dark').accent, coral: 'oklch(0.78 0.14 70)', coralSoft: 'oklch(0.35 0.08 70)', coralDeep: 'oklch(0.85 0.16 75)' },
    selBg: 'oklch(0.35 0.10 70)', selFg: 'oklch(0.95 0.08 80)',
  };
  return <MiniMock T={T} title="terminal warmth" mono />;
}

// Variation 3: "Scandi" — light, airy, blue-sage
function VariationScandi() {
  const T = {
    ...phTokens('light'),
    bg: '#fbfbf9', bgSidebar: '#f3f3ef', bgList: '#fbfbf9', bgReader: '#ffffff',
    bgTitlebar: '#eeede7', bgElev: '#f7f6f1',
    border: '#d8d6ce', borderSoft: '#e5e3dc',
    fg: '#1e2420', fgMuted: '#5a6560', fgSubtle: '#7a847f', fgFaint: '#9da5a0',
    accent: { ...phTokens('light').accent, coral: 'oklch(0.58 0.11 190)', coralDeep: 'oklch(0.42 0.12 190)', coralSoft: 'oklch(0.92 0.04 190)' },
    selBg: 'oklch(0.88 0.05 190)', selFg: 'oklch(0.25 0.10 190)',
  };
  return <MiniMock T={T} title="scandi" />;
}

// Variation 4: "Soft Neon" — dark with glow accents
function VariationNeon() {
  const T = {
    ...phTokens('dark'),
    bg: '#121216', bgSidebar: '#0d0d11', bgList: '#141418', bgReader: '#17171c',
    bgTitlebar: '#0d0d11', bgElev: '#1c1c22',
    border: '#26262e', borderSoft: '#1d1d24',
    fg: '#eaeaf0', fgMuted: '#9a9aa8', fgSubtle: '#70707e',
    accent: { ...phTokens('dark').accent, coral: 'oklch(0.75 0.2 330)', coralDeep: 'oklch(0.85 0.22 330)', coralSoft: 'oklch(0.28 0.1 330)' },
    selBg: 'oklch(0.3 0.12 330)', selFg: 'oklch(0.95 0.08 330)',
  };
  return <MiniMock T={T} title="soft neon" glow />;
}

// Shared mini-mock used by variations — shows sidebar + list + reader at small scale
function MiniMock({ T, title, serif, mono, glow }) {
  const displayFont = serif ? T.font.display : mono ? T.font.mono : T.font.sans;
  return (
    <div style={{
      width: '100%', height: '100%', background: T.bg, color: T.fg,
      fontFamily: T.font.sans, display: 'flex', flexDirection: 'column',
      position: 'relative', overflow: 'hidden',
    }}>
      {/* Titlebar */}
      <div style={{
        height: 32, background: T.bgTitlebar, borderBottom: `1px solid ${T.border}`,
        display: 'flex', alignItems: 'center', padding: '0 10px', gap: 10, flexShrink: 0,
      }}>
        <div style={{ display: 'flex', gap: 5 }}>
          <div style={{ width: 9, height: 9, borderRadius: '50%', background: '#ff5f57' }} />
          <div style={{ width: 9, height: 9, borderRadius: '50%', background: '#febc2e' }} />
          <div style={{ width: 9, height: 9, borderRadius: '50%', background: '#28c940' }} />
        </div>
        <PostmarkStamp size={14} color={T.accent.coral} />
        <div style={{ fontFamily: displayFont, fontSize: 12, fontWeight: serif ? 700 : 900, fontStyle: serif ? 'italic' : 'normal', letterSpacing: -0.3, color: T.fg,
          textShadow: glow ? `0 0 12px ${T.accent.coral}` : 'none' }}>
          Posthaste
        </div>
        <div style={{ flex: 1 }} />
        <div style={{ fontSize: 10, color: T.fgMuted }}>Inbox · 142</div>
      </div>
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        {/* Sidebar */}
        <div style={{ width: 120, background: T.bgSidebar, borderRight: `1px solid ${T.border}`, padding: '8px 0', display: 'flex', flexDirection: 'column', gap: 1, flexShrink: 0 }}>
          {['Inbox', 'Starred', 'Drafts', 'Sent'].map((l, i) => (
            <div key={l} style={{
              padding: '3px 10px', fontSize: 10.5, fontWeight: i === 0 ? 600 : 500,
              background: i === 0 ? T.selBg : 'transparent',
              color: i === 0 ? T.selFg : T.fg, display: 'flex', alignItems: 'center', gap: 5,
              borderRadius: 3, margin: '0 4px',
            }}>
              <div style={{ width: 4, height: 4, borderRadius: '50%', background: T.accent.coral, opacity: i === 0 ? 1 : 0.3 }} />
              {l}
              {i === 0 && <span style={{ marginLeft: 'auto', fontFamily: T.font.mono, fontSize: 9 }}>8</span>}
            </div>
          ))}
          <div style={{ padding: '10px 10px 3px', fontSize: 8.5, fontFamily: T.font.mono, fontWeight: 600, color: T.fgFaint, textTransform: 'uppercase', letterSpacing: 0.5 }}>Smart</div>
          {['Relevant', 'Read Later', 'Bills'].map((l, i) => (
            <div key={l} style={{ padding: '3px 10px', fontSize: 10.5, color: T.fg, margin: '0 4px', display: 'flex', alignItems: 'center', gap: 5 }}>
              <div style={{ width: 4, height: 4, borderRadius: '50%', background: T.accent.sage }} />
              {l}
            </div>
          ))}
        </div>
        {/* List */}
        <div style={{ width: 180, background: T.bgList, borderRight: `1px solid ${T.border}`, flexShrink: 0 }}>
          <div style={{ height: 18, display: 'flex', borderBottom: `1px solid ${T.border}`, background: T.bgTitlebar }}>
            {['SUBJ', 'FROM', 'DATE'].map((c, i) => (
              <div key={c} style={{ flex: 1, padding: '0 6px', fontSize: 8, fontFamily: T.font.mono, color: T.fgFaint, fontWeight: 600, display: 'flex', alignItems: 'center' }}>{c}</div>
            ))}
          </div>
          {PH_MESSAGES.slice(0, 7).map((m, i) => (
            <div key={m.id} style={{
              padding: '5px 8px', fontSize: 10, display: 'flex', gap: 6,
              background: i === 0 ? T.selBg : 'transparent',
              color: i === 0 ? T.selFg : (m.unread ? T.fg : T.fgMuted),
              fontWeight: m.unread ? 600 : 400,
              borderBottom: `1px solid ${T.borderSoft}`,
              alignItems: 'center',
            }}>
              {m.unread && <div style={{ width: 4, height: 4, borderRadius: '50%', background: i === 0 ? T.selFg : T.accent.coral, flexShrink: 0 }} />}
              <span style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{m.subject}</span>
              <span style={{ fontFamily: T.font.mono, fontSize: 8.5, opacity: 0.7 }}>{m.dateShort}</span>
            </div>
          ))}
        </div>
        {/* Reader */}
        <div style={{ flex: 1, background: T.bgReader, padding: 14, minWidth: 0, overflow: 'hidden' }}>
          <div style={{ fontSize: 14, fontFamily: serif ? T.font.display : T.font.sans, fontWeight: serif ? 600 : 600, color: T.fg, marginBottom: 6, lineHeight: 1.2,
            textShadow: glow ? `0 0 20px ${T.accent.coral}40` : 'none' }}>
            March 2026 billing
          </div>
          <div style={{ fontSize: 10, color: T.fgMuted, marginBottom: 12, display: 'flex', gap: 6, alignItems: 'center' }}>
            <div style={{ width: 16, height: 16, borderRadius: '50%', background: T.accent.coral, color: '#fff', fontSize: 8, fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>LC</div>
            <span style={{ fontWeight: 600, color: T.fg }}>Leo Cancelmo</span>
            <span style={{ fontFamily: T.font.mono }}>· Yesterday 18:04</span>
          </div>
          <div style={{
            padding: 8, border: `1px solid ${T.border}`, borderRadius: 4,
            display: 'flex', gap: 7, alignItems: 'center', marginBottom: 10, background: T.bg,
          }}>
            <div style={{ width: 20, height: 20, borderRadius: 3, background: T.accent.coral, color: '#fff', fontSize: 7, fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center', fontFamily: T.font.mono }}>PDF</div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 9.5, fontWeight: 500, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>TR_Billing March 2026.pdf</div>
              <div style={{ fontSize: 8.5, color: T.fgFaint, fontFamily: T.font.mono }}>53.6 KiB</div>
            </div>
          </div>
          <div style={{ fontSize: 10.5, lineHeight: 1.55, color: T.fg, fontFamily: serif ? T.font.display : T.font.sans }}>
            Hi Theo — see attached for March. Let me know if any questions about the line items this month.
          </div>
        </div>
      </div>
      {/* Status */}
      <div style={{ height: 18, background: T.bgSidebar, borderTop: `1px solid ${T.border}`, padding: '0 10px', fontSize: 9, color: T.fgMuted, fontFamily: T.font.mono, display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{ width: 5, height: 5, borderRadius: '50%', background: T.accent.sage }} />
        jmap · fastmail · sync ok
        <div style={{ flex: 1 }} />
        <span>{title}</span>
      </div>
    </div>
  );
}

Object.assign(window, { VariationCozy, VariationTerminal, VariationScandi, VariationNeon });
