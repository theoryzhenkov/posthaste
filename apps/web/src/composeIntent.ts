export type ComposeIntent =
  | { kind: 'new'; sourceId: string }
  | { kind: 'reply'; sourceId: string; messageId: string }
  | { kind: 'replyAll'; sourceId: string; messageId: string }
  | { kind: 'forward'; sourceId: string; messageId: string }
  // Resume editing an existing draft. `messageId` is the draft's id, reused as
  // the autosave draft key so edits update that draft instead of creating one.
  | { kind: 'draft'; sourceId: string; messageId: string }
  // A fresh composition seeded from a `mailto:` URI (the List-Unsubscribe
  // mailto path): to/subject/body prefill from the URI, then it behaves like
  // `new`. The user reviews and sends — nothing is auto-sent.
  | { kind: 'mailto'; sourceId: string; mailtoUri: string }

/** Compose-form seed parsed from a `mailto:` URI. */
export interface MailtoSeed {
  to: string
  subject: string
  body: string
}

/**
 * Parse a `mailto:` URI (RFC 6068) into a compose-form seed: the address list
 * before `?` becomes `to`, and the `to`/`subject`/`body` query params are
 * honored (extra `to` values append). Unknown params are ignored; a malformed
 * percent-escape degrades to its raw text rather than throwing.
 */
export function parseMailtoUri(uri: string): MailtoSeed {
  const decode = (value: string): string => {
    try {
      return decodeURIComponent(value)
    } catch {
      return value
    }
  }
  const withoutScheme = /^mailto:/i.test(uri) ? uri.slice(7) : uri
  const queryIndex = withoutScheme.indexOf('?')
  const addressPart =
    queryIndex === -1 ? withoutScheme : withoutScheme.slice(0, queryIndex)
  const to: string[] = addressPart
    .split(',')
    .map((address) => decode(address.trim()))
    .filter((address) => address.length > 0)
  let subject = ''
  let body = ''
  if (queryIndex !== -1) {
    for (const pair of withoutScheme.slice(queryIndex + 1).split('&')) {
      const eq = pair.indexOf('=')
      if (eq === -1) continue
      const key = decode(pair.slice(0, eq)).toLowerCase()
      const value = decode(pair.slice(eq + 1))
      if (key === 'to' && value) {
        to.push(value)
      } else if (key === 'subject') {
        subject = value
      } else if (key === 'body') {
        body = value
      }
    }
  }
  return { to: to.join(', '), subject, body }
}
