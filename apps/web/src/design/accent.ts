export const defaultAccentHue = 45

/**
 * Default surface hue — the "main color" of panes/background. 60 is the neutral
 * grey the app shipped with; users can shift it per light/dark mode.
 */
export const defaultSurfaceHue = 60

export function normalizeAccentHue(value: number): number {
  if (!Number.isFinite(value)) {
    return defaultAccentHue
  }
  return Math.round(((value % 360) + 360) % 360)
}

export function parseAccentHue(value: string | null): number {
  if (value === null) {
    return defaultAccentHue
  }
  return normalizeAccentHue(Number(value))
}

export function accentColor(
  hue: number,
  lightness = 0.68,
  chroma = 0.17,
): string {
  return `oklch(${lightness} ${chroma} ${normalizeAccentHue(hue)})`
}
