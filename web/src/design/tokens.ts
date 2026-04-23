export const typeScale = {
  meta: 11,
  ui: 12,
  body: 13,
  emph: 14,
  head: 17,
  sect: 22,
} as const;

export type TypeScaleName = keyof typeof typeScale;
export type TypeScalePx = (typeof typeScale)[TypeScaleName];

export const iconSizes = {
  xs: 12,
  sm: 14,
  md: 16,
  lg: 20,
} as const;

export type IconSizeName = keyof typeof iconSizes;
export type IconSizePx = (typeof iconSizes)[IconSizeName];

export const iconStrokeWidths = {
  12: 1.1,
  14: 1.25,
  16: 1.4,
  20: 1.6,
} as const satisfies Record<IconSizePx, number>;

export type IconStrokeSize = keyof typeof iconStrokeWidths;

export const fontStacks = {
  sans: "'Geist Variable', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
  mono: "'Geist Mono Variable', ui-monospace, 'SF Mono', Menlo, monospace",
  display: "'Fraunces', Georgia, serif",
} as const;

export type FontStackName = keyof typeof fontStacks;

export const brandAccents = {
  coral: "oklch(0.68 0.17 45)",
  coralSoft: "oklch(0.92 0.055 50)",
  coralDeep: "oklch(0.52 0.18 38)",
  sage: "oklch(0.68 0.08 145)",
  sageSoft: "oklch(0.93 0.03 145)",
  blue: "oklch(0.65 0.13 245)",
  amber: "oklch(0.78 0.13 78)",
  violet: "oklch(0.65 0.13 295)",
  rose: "oklch(0.70 0.15 12)",
} as const;

export type BrandAccentName = keyof typeof brandAccents;

export const radii = {
  xs: 3,
  sm: 4,
  md: 6,
  lg: 10,
  xl: 14,
} as const;

export type RadiusName = keyof typeof radii;

export const semanticSignals = {
  unread: brandAccents.blue,
  flag: brandAccents.coral,
} as const;

export type SemanticSignalName = keyof typeof semanticSignals;

export const designTokenMetadata = {
  typeScale,
  iconSizes,
  iconStrokeWidths,
  fontStacks,
  brandAccents,
  radii,
  semanticSignals,
} as const;
