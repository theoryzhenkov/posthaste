export const EMAIL_LINK_HREF_ATTR = 'data-ph-href'

// Move every anchor's `href` onto a data attribute so the sandboxed reader
// iframe physically cannot navigate when a link is clicked — we then open the
// captured URL externally from the click handler. This fixes link handling by
// construction rather than relying on `preventDefault()` reaching across the
// iframe boundary (which is unreliable in some webviews).
export function neutralizeEmailLinks(html: string): string {
  if (typeof DOMParser === 'undefined') {
    return html
  }
  const doc = new DOMParser().parseFromString(html, 'text/html')
  for (const anchor of Array.from(doc.querySelectorAll('a[href]'))) {
    const href = anchor.getAttribute('href')
    anchor.removeAttribute('href')
    if (href !== null) {
      anchor.setAttribute(EMAIL_LINK_HREF_ATTR, href)
    }
  }
  return doc.body.innerHTML
}

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
