export function uniqueRails(values: number[]): number[] {
  const rails: number[] = []
  for (const value of values) {
    if (!rails.some((rail) => Math.abs(rail - value) < 1)) {
      rails.push(value)
    }
  }
  return rails
}

export function clamp(value: number, min: number, max: number): number {
  // Guard against inverted bounds: the resize paths can derive a min above the
  // max at extreme viewport sizes, and silently collapsing to `max` could land
  // outside the screen margins.
  return Math.min(Math.max(value, Math.min(min, max)), Math.max(min, max))
}
