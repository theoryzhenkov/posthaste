export function externalEmailLinkUrl(rawHref: string | null): string | null {
  const value = rawHref?.trim()
  if (!value) {
    return null
  }

  try {
    const parsed = new URL(value)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
      ? parsed.toString()
      : null
  } catch {
    return null
  }
}
