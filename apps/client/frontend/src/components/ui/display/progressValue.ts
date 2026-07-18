export function normalizeProgressValue(value: number | null | undefined) {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return null
  }
  return Math.min(100, Math.max(0, value))
}
