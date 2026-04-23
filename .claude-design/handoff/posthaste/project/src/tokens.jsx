// Posthaste design tokens — v2
// Locked type ramp (5 sizes + display), icon grid (12/14/16/20),
// decoupled selection/unread/flag signals.

const PH_TYPE = {
  // Locked ramp — nothing else should appear in the UI.
  meta: 11,    // mono caps, column headers, kbd hints
  ui:   12,    // chrome, buttons, sidebar, metadata
  body: 13,    // list rows, reader body, compose fields
  emph: 14,    // emphasized rows, sub-headings, account names
  head: 17,    // reader subject, modal titles
  sect: 22,    // section headings (rare)
};

const PH_ICON = { xs: 12, sm: 14, md: 16, lg: 20 };

// Stroke-width per icon size so the optical weight stays constant.
// (Authored at 16×16 with strokeWidth 1.4 — scale by base/size.)
const PH_STROKE = { 12: 1.1, 14: 1.25, 16: 1.4, 20: 1.6 };

const PH_TOKENS = {
  type: PH_TYPE,
  icon: PH_ICON,
  stroke: PH_STROKE,
  font: {
    sans: "'Geist', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
    mono: "'Geist Mono', ui-monospace, 'SF Mono', Menlo, monospace",
    display: "'Fraunces', 'Georgia', serif",
  },
  accent: {
    coral: 'oklch(0.68 0.17 45)',       // postmark — BRAND ONLY, used for flag + brand moments
    coralSoft: 'oklch(0.92 0.055 50)',
    coralDeep: 'oklch(0.52 0.18 38)',
    sage: 'oklch(0.68 0.08 145)',
    sageSoft: 'oklch(0.93 0.03 145)',
    blue: 'oklch(0.65 0.13 245)',       // UNREAD signal — distinct from brand coral
    amber: 'oklch(0.78 0.13 78)',
    violet: 'oklch(0.65 0.13 295)',
    rose: 'oklch(0.70 0.15 12)',
  },
  radius: { xs: 3, sm: 4, md: 6, lg: 10, xl: 14 },
  // Semantic signals — intentionally three distinct hues so selection /
  // unread / flag never collide on the eye.
  signal: {
    unread: 'oklch(0.65 0.13 245)',     // blue dot
    flag:   'oklch(0.68 0.17 45)',      // coral flag
    // selection is supplied per theme (below)
  },
};

function phTokens(theme = 'dark') {
  if (theme === 'light') {
    return {
      ...PH_TOKENS,
      bg: 'oklch(0.985 0.005 80)',
      bgElev: 'oklch(0.97 0.006 75)',
      bgSidebar: 'oklch(0.955 0.008 70)',
      bgList: 'oklch(0.98 0.005 80)',
      bgListAlt: 'oklch(0.965 0.006 75)',
      bgReader: '#fff',
      bgTitlebar: 'oklch(0.945 0.008 70)',
      border: 'oklch(0.88 0.008 70)',
      borderSoft: 'oklch(0.93 0.006 70)',
      borderStrong: 'oklch(0.78 0.01 70)',
      fg: 'oklch(0.22 0.01 60)',
      fgMuted: 'oklch(0.46 0.01 60)',     // WCAG AA-compliant on bgList
      fgSubtle: 'oklch(0.55 0.008 60)',
      fgFaint: 'oklch(0.65 0.008 60)',
      // Selection: neutral slate so it's visibly distinct from coral brand
      selBg: 'oklch(0.88 0.02 250)',
      selFg: 'oklch(0.22 0.05 250)',
      focusRing: 'oklch(0.62 0.15 250)',
      hoverBg: 'oklch(0.94 0.008 70)',
      shadow: '0 1px 2px rgba(40,30,20,0.06), 0 4px 16px rgba(40,30,20,0.04)',
    };
  }
  return {
    ...PH_TOKENS,
    bg: 'oklch(0.22 0.008 60)',
    bgElev: 'oklch(0.26 0.008 60)',
    bgSidebar: 'oklch(0.195 0.008 55)',
    bgList: 'oklch(0.235 0.008 60)',
    bgListAlt: 'oklch(0.255 0.008 60)',
    bgReader: 'oklch(0.27 0.008 60)',
    bgTitlebar: 'oklch(0.185 0.008 55)',
    border: 'oklch(0.32 0.008 60)',
    borderSoft: 'oklch(0.28 0.008 60)',
    borderStrong: 'oklch(0.4 0.01 60)',
    fg: 'oklch(0.94 0.005 80)',
    fgMuted: 'oklch(0.72 0.008 70)',    // ↑ contrast for AA
    fgSubtle: 'oklch(0.60 0.008 70)',
    fgFaint: 'oklch(0.48 0.008 60)',
    selBg: 'oklch(0.34 0.06 250)',      // slate-blue selection, not coral
    selFg: 'oklch(0.98 0.01 250)',
    focusRing: 'oklch(0.68 0.15 250)',
    hoverBg: 'oklch(0.29 0.008 60)',
    shadow: '0 2px 6px rgba(0,0,0,0.3), 0 8px 32px rgba(0,0,0,0.25)',
  };
}

window.phTokens = phTokens;
window.PH_TOKENS = PH_TOKENS;
window.PH_TYPE = PH_TYPE;
window.PH_ICON = PH_ICON;
window.PH_STROKE = PH_STROKE;
