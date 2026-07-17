// Defense-in-depth sanitizer for untrusted message HTML. The rendered frame
// is already sandboxed (no scripts) and carries a CSP that blocks script and
// plugin content, but the markup itself is scrubbed too, so the safety of
// mail rendering never rests on a single attribute: active elements are
// removed, event handlers and script-scheme URLs are stripped, and a
// message cannot inject <base>/<meta> to redirect link resolution or
// navigate the frame.

/** Elements removed outright: script/plugin content, frame busters, head
 * machinery that redefines the document, and form submission surfaces. */
const BLOCKED_ELEMENTS = new Set([
  'script',
  'iframe',
  'frame',
  'frameset',
  'object',
  'embed',
  'applet',
  'base',
  'link',
  'meta',
  'form',
  'input',
  'button',
  'select',
  'textarea',
  'template',
  'portal',
  'dialog',
])

/** Attributes whose value is a single URL and must carry a safe scheme. */
const URL_ATTRIBUTES = new Set([
  'href',
  'src',
  'xlink:href',
  'action',
  'formaction',
  'poster',
  'background',
  'cite',
  'data',
])

/** True when a URL is safe to keep: relative, or an inert scheme. Blocks
 * javascript:/vbscript: outright and data: except images. */
function isSafeUrl(value: string): boolean {
  // Strip whitespace and control characters that browsers ignore when
  // resolving a scheme, so "jav\tascript:" cannot slip through.
  const compact = value.replace(/[\u0000-\u0020\u007f]/g, '').toLowerCase()
  const scheme = /^([a-z][a-z0-9+.-]*):/.exec(compact)?.[1]
  if (!scheme) return true // relative URL
  if (scheme === 'data') return compact.startsWith('data:image/')
  return ['http', 'https', 'mailto', 'tel', 'cid', 'mid'].includes(scheme)
}

/** Rewrites a srcset value, dropping candidates with unsafe URLs. */
function sanitizeSrcset(value: string): string {
  return value
    .split(',')
    .map((candidate) => candidate.trim())
    .filter((candidate) => {
      const url = candidate.split(/\s+/, 1)[0] ?? ''
      return url !== '' && isSafeUrl(url)
    })
    .join(', ')
}

function sanitizeElement(el: Element): void {
  for (const attr of Array.from(el.attributes)) {
    const name = attr.name.toLowerCase()
    if (name.startsWith('on')) {
      el.removeAttribute(attr.name)
    } else if (URL_ATTRIBUTES.has(name) && !isSafeUrl(attr.value)) {
      el.removeAttribute(attr.name)
    } else if (name === 'srcset' || name === 'imagesrcset') {
      const cleaned = sanitizeSrcset(attr.value)
      if (cleaned) el.setAttribute(attr.name, cleaned)
      else el.removeAttribute(attr.name)
    }
  }
}

/** Scrubs untrusted message HTML into markup that is inert without a
 * sandbox: no active elements, no event handlers, no script-scheme URLs.
 * Returns body markup only — anything head-level the message carried is
 * dropped. */
export function sanitizeMessageHtml(html: string): string {
  const parsed = new DOMParser().parseFromString(html, 'text/html')
  for (const el of Array.from(parsed.querySelectorAll('*'))) {
    if (BLOCKED_ELEMENTS.has(el.localName)) {
      el.remove()
      continue
    }
    sanitizeElement(el)
  }
  return parsed.body.innerHTML
}
