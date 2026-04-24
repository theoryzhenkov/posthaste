export const defaultAccentHue = 45

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
