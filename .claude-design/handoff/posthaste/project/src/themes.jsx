// Posthaste theme presets — complete aesthetic directions.
//
// Each preset returns a full token object on top of the base light/dark
// phTokens(). The current default (Cool Gray) is included as "neutral".
// A preset may also attach per-preset CSS (set on the Prototype root) for
// things like panel backdrops, custom scrollbars, etc.
//
// Shape: { id, label, description, mode: 'light'|'dark', tokens(), css?, style? }

const PH_THEMES = {
  // ───────────────────────────────────────────────
  // 1. Neutral — existing Cool Gray (default)
  // ───────────────────────────────────────────────
  neutral: {
    id: 'neutral',
    label: 'Neutral',
    description: 'Cool gray, balanced contrast',
    modes: ['light', 'dark'],
    tokens: (mode) => phTokens(mode),
  },

  // ───────────────────────────────────────────────
  // 2. Paper & Ink — editorial print
  // ───────────────────────────────────────────────
  paperInk: {
    id: 'paperInk',
    label: 'Paper & Ink',
    description: 'Bright white, thin rules, editorial serifs, ink-red accents',
    modes: ['light'],
    tokens: () => {
      const base = phTokens('light');
      return {
        ...base,
        bg: '#fefdf8',
        bgElev: '#f7f4ea',
        bgSidebar: '#faf7ed',
        bgList: '#fefdf8',
        bgListAlt: '#f7f4ea',
        bgReader: '#ffffff',
        bgTitlebar: '#f4f0e3',
        border: '#1a1814',         // high-contrast ink border
        borderSoft: '#d9d4c5',
        borderStrong: '#1a1814',
        fg: '#1a1814',
        fgMuted: '#5a554a',
        fgSubtle: '#78725f',
        fgFaint: '#a39d88',
        selBg: '#fff3cf',
        selFg: '#1a1814',
        focusRing: '#c8372d',
        hoverBg: '#f2ecdb',
        shadow: 'none',            // no shadows — flat print
        accent: {
          ...base.accent,
          coral: '#c8372d',        // brick red ink
          coralDeep: '#8f2118',
          coralSoft: '#f5d4cf',
          blue: '#1e4ea8',         // classic ink blue
          sage: '#3d6e3d',
          amber: '#b8850e',
          violet: '#5e3a9a',
          rose: '#b8386c',
        },
        signal: { unread: '#1e4ea8', flag: '#c8372d' },
        font: { ...base.font, sans: base.font.display },
        style: 'editorial',
      };
    },
  },

  // ───────────────────────────────────────────────
  // 3. Brutalist Mono — stark grayscale, no rounding
  // ───────────────────────────────────────────────
  brutalist: {
    id: 'brutalist',
    label: 'Brutalist',
    description: 'Monospace everywhere, 2px borders, zero rounding',
    modes: ['light', 'dark'],
    tokens: (mode) => {
      const base = phTokens(mode);
      const isDark = mode === 'dark';
      return {
        ...base,
        bg: isDark ? '#0a0a0a' : '#fafafa',
        bgElev: isDark ? '#161616' : '#f0f0f0',
        bgSidebar: isDark ? '#000000' : '#ededed',
        bgList: isDark ? '#0a0a0a' : '#fafafa',
        bgListAlt: isDark ? '#131313' : '#f2f2f2',
        bgReader: isDark ? '#000000' : '#ffffff',
        bgTitlebar: isDark ? '#000000' : '#e0e0e0',
        border: isDark ? '#ffffff' : '#000000',
        borderSoft: isDark ? '#2a2a2a' : '#c8c8c8',
        borderStrong: isDark ? '#ffffff' : '#000000',
        fg: isDark ? '#ffffff' : '#000000',
        fgMuted: isDark ? '#a0a0a0' : '#505050',
        fgSubtle: isDark ? '#707070' : '#707070',
        fgFaint: isDark ? '#505050' : '#909090',
        selBg: isDark ? '#ffffff' : '#000000',
        selFg: isDark ? '#000000' : '#ffffff',
        focusRing: isDark ? '#ffffff' : '#000000',
        hoverBg: isDark ? '#1a1a1a' : '#e8e8e8',
        shadow: 'none',
        radius: { xs: 0, sm: 0, md: 0, lg: 0, xl: 0 },
        accent: {
          ...base.accent,
          // Flat primary colors — no oklch
          coral:     '#ff3333',
          coralDeep: '#cc0000',
          coralSoft: isDark ? '#661515' : '#ffd4d4',
          blue:      '#1d4ed8',
          sage:      '#16a34a',
          amber:     '#ea9b0a',
          violet:    '#7c3aed',
          rose:      '#ec4899',
        },
        signal: { unread: '#1d4ed8', flag: '#ff3333' },
        font: {
          ...base.font,
          sans: base.font.mono,
          display: base.font.mono,
        },
        style: 'brutalist',
      };
    },
  },

  // ───────────────────────────────────────────────
  // 4. Glass & Vapor — frosted blur over gradient mesh
  // ───────────────────────────────────────────────
  glass: {
    id: 'glass',
    label: 'Glass',
    description: 'Frosted panels over a soft gradient mesh',
    modes: ['dark', 'light'],
    tokens: (mode) => {
      const base = phTokens(mode);
      const isDark = mode === 'dark';
      return {
        ...base,
        bg: 'transparent',         // mesh painted by root
        bgElev: isDark ? 'rgba(255,255,255,0.06)' : 'rgba(255,255,255,0.55)',
        bgSidebar: isDark ? 'rgba(20,18,30,0.4)' : 'rgba(255,255,255,0.55)',
        bgList: isDark ? 'rgba(24,22,34,0.35)' : 'rgba(255,255,255,0.5)',
        bgListAlt: isDark ? 'rgba(255,255,255,0.025)' : 'rgba(255,255,255,0.3)',
        bgReader: isDark ? 'rgba(18,16,26,0.45)' : 'rgba(255,255,255,0.7)',
        bgTitlebar: isDark ? 'rgba(15,13,22,0.55)' : 'rgba(255,255,255,0.6)',
        border: isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.08)',
        borderSoft: isDark ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.04)',
        borderStrong: isDark ? 'rgba(255,255,255,0.16)' : 'rgba(0,0,0,0.14)',
        fg: isDark ? '#f0ecff' : '#1a1728',
        fgMuted: isDark ? '#b0a8c8' : '#584f70',
        fgSubtle: isDark ? '#8880a0' : '#786f90',
        fgFaint: isDark ? '#666078' : '#9f96b8',
        selBg: isDark ? 'rgba(180,130,255,0.25)' : 'rgba(180,130,255,0.35)',
        selFg: isDark ? '#f5f0ff' : '#2a1a50',
        focusRing: '#b882ff',
        hoverBg: isDark ? 'rgba(255,255,255,0.05)' : 'rgba(0,0,0,0.03)',
        shadow: isDark
          ? '0 8px 32px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.08)'
          : '0 8px 32px rgba(100,80,200,0.12), inset 0 1px 0 rgba(255,255,255,0.6)',
        accent: {
          ...base.accent,
          coral:     '#ff7aa2',
          coralDeep: '#ff9fc0',
          coralSoft: isDark ? 'rgba(255,122,162,0.15)' : 'rgba(255,122,162,0.2)',
          blue:      '#8ab4ff',
          sage:      '#8eeac1',
          amber:     '#ffd27a',
          violet:    '#b882ff',
          rose:      '#ff9fc0',
        },
        signal: { unread: '#8ab4ff', flag: '#ff7aa2' },
        style: 'glass',
      };
    },
  },

  // ───────────────────────────────────────────────
  // 5. Acid — pure black, electric green, dev-tool
  // ───────────────────────────────────────────────
  acid: {
    id: 'acid',
    label: 'Acid',
    description: 'Pure black + electric lime, mechanical precision',
    modes: ['dark'],
    tokens: () => {
      const base = phTokens('dark');
      return {
        ...base,
        bg: '#000000',
        bgElev: '#0a0a0a',
        bgSidebar: '#050505',
        bgList: '#000000',
        bgListAlt: '#080808',
        bgReader: '#000000',
        bgTitlebar: '#050505',
        border: '#1a1a1a',
        borderSoft: '#121212',
        borderStrong: '#d0ff00',
        fg: '#e0ffe0',
        fgMuted: '#7aa87a',
        fgSubtle: '#507050',
        fgFaint: '#2a4a2a',
        selBg: 'rgba(208,255,0,0.15)',
        selFg: '#d0ff00',
        focusRing: '#d0ff00',
        hoverBg: 'rgba(208,255,0,0.05)',
        shadow: '0 0 0 1px rgba(208,255,0,0.15)',
        accent: {
          ...base.accent,
          coral:     '#ff0050',
          coralDeep: '#ff3070',
          coralSoft: 'rgba(255,0,80,0.15)',
          blue:      '#00aaff',
          sage:      '#d0ff00',    // the lime becomes the "success/primary"
          amber:     '#ffb800',
          violet:    '#b400ff',
          rose:      '#ff0080',
        },
        signal: { unread: '#d0ff00', flag: '#ff0050' },
        font: {
          ...base.font,
          sans: base.font.mono,     // mono everywhere for dev-tool feel
        },
        style: 'acid',
      };
    },
  },

  // ───────────────────────────────────────────────
  // 6. Marzipan — pastel, rounded, soft
  // ───────────────────────────────────────────────
  marzipan: {
    id: 'marzipan',
    label: 'Marzipan',
    description: 'Soft pastels, generous rounding, friendly',
    modes: ['light'],
    tokens: () => {
      const base = phTokens('light');
      return {
        ...base,
        bg: '#f5ebe4',               // muted peach
        bgElev: '#efe1d6',
        bgSidebar: '#eadccf',
        bgList: '#f7f0ea',
        bgListAlt: '#f1e6dc',
        bgReader: '#fefaf7',
        bgTitlebar: '#ead8c8',
        border: '#d4bfa8',
        borderSoft: '#e3d2c0',
        borderStrong: '#b89878',
        fg: '#3a2818',
        fgMuted: '#6a5040',
        fgSubtle: '#8a7060',
        fgFaint: '#a89080',
        selBg: '#c8d4ef',
        selFg: '#1a2a50',
        focusRing: '#9c8fd4',
        hoverBg: '#ebe0d2',
        shadow: '0 1px 3px rgba(160,110,70,0.08), 0 8px 24px rgba(160,110,70,0.06)',
        radius: { xs: 5, sm: 8, md: 12, lg: 16, xl: 22 },
        accent: {
          ...base.accent,
          coral:     '#e8735a',       // salmon
          coralDeep: '#c84a2e',
          coralSoft: '#fbdcd0',
          blue:      '#6a8fc4',
          sage:      '#7ab084',
          amber:     '#e8a85a',
          violet:    '#9c8fd4',
          rose:      '#d890a8',
        },
        signal: { unread: '#6a8fc4', flag: '#e8735a' },
        style: 'marzipan',
      };
    },
  },

  // ───────────────────────────────────────────────
  // 7. Botanical — deep forest + cream, warm
  // ───────────────────────────────────────────────
  botanical: {
    id: 'botanical',
    label: 'Botanical',
    description: 'Deep forest green on cream, quiet and confident',
    modes: ['light'],
    tokens: () => {
      const base = phTokens('light');
      return {
        ...base,
        bg: '#f2ede0',
        bgElev: '#ebe5d2',
        bgSidebar: '#e8dfc8',           // cream sidebar — legible dark text
        bgList: '#f5f0e3',
        bgListAlt: '#ede8db',
        bgReader: '#fbf8ed',
        bgTitlebar: '#dfd4b8',
        border: '#b8ad8a',
        borderSoft: '#d1c7a8',
        borderStrong: '#8a7f5c',
        fg: '#2a2818',
        fgMuted: '#5a5340',
        fgSubtle: '#7a7058',
        fgFaint: '#a09572',
        selBg: '#d4c280',
        selFg: '#2a2818',
        focusRing: '#3d6e3d',
        hoverBg: '#ebe5d2',
        shadow: '0 2px 6px rgba(60,50,30,0.08)',
        accent: {
          ...base.accent,
          coral:     '#d35432',          // burnt orange
          coralDeep: '#9a3a1e',
          coralSoft: '#f2d0bf',
          blue:      '#385a7a',
          sage:      '#3d6e3d',          // mossy — the "primary"
          amber:     '#c88a1e',
          violet:    '#6a4a7c',
          rose:      '#a8486c',
        },
        signal: { unread: '#385a7a', flag: '#d35432' },
        sidebarInvert: true,              // tell Sidebar to style as dark on cream
        style: 'botanical',
      };
    },
  },
};

// Resolve a theme id + mode into a full token set.
function resolveTheme(themeId, mode = 'dark') {
  const def = PH_THEMES[themeId] || PH_THEMES.neutral;
  const resolvedMode = def.modes.includes(mode) ? mode : def.modes[0];
  const tokens = def.tokens(resolvedMode);
  return { ...tokens, themeId: def.id, themeLabel: def.label, themeStyle: tokens.style || 'neutral', mode: resolvedMode };
}

// Root-level CSS / background for presets that need extra paint (e.g. glass mesh).
function themeBackdrop(themeId, mode = 'dark') {
  if (themeId === 'glass') {
    if (mode === 'dark') {
      return {
        background: `
          radial-gradient(circle at 20% 10%, rgba(184,130,255,0.35) 0%, transparent 45%),
          radial-gradient(circle at 85% 25%, rgba(255,122,162,0.25) 0%, transparent 45%),
          radial-gradient(circle at 50% 90%, rgba(138,180,255,0.3) 0%, transparent 50%),
          radial-gradient(circle at 10% 85%, rgba(142,234,193,0.2) 0%, transparent 40%),
          linear-gradient(180deg, #0a0812 0%, #050410 100%)
        `,
      };
    }
    return {
      background: `
        radial-gradient(circle at 20% 10%, rgba(184,130,255,0.3) 0%, transparent 50%),
        radial-gradient(circle at 85% 25%, rgba(255,180,210,0.3) 0%, transparent 50%),
        radial-gradient(circle at 50% 90%, rgba(160,200,255,0.3) 0%, transparent 55%),
        linear-gradient(180deg, #f0e8ff 0%, #e8f0ff 100%)
      `,
    };
  }
  if (themeId === 'acid') {
    return {
      background: '#000',
      backgroundImage: `
        linear-gradient(rgba(208,255,0,0.03) 1px, transparent 1px),
        linear-gradient(90deg, rgba(208,255,0,0.03) 1px, transparent 1px)
      `,
      backgroundSize: '24px 24px',
    };
  }
  return {};
}

Object.assign(window, { PH_THEMES, resolveTheme, themeBackdrop });
