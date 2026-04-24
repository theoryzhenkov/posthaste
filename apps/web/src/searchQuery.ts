export function normalizeAppliedSearchQuery(value: string): string {
  return value.trim().replace(/\s+/g, ' ')
}
