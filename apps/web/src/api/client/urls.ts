import { baseUrl } from './core'

export function buildMessageAttachmentUrl(
  sourceId: string,
  messageId: string,
  attachmentId: string,
): string {
  // No `?download=1` content-disposition variant: the bytes are loaded via an
  // authenticated blob fetch (preview) or saved with the `download` attribute
  // (downloads), so the client owns the filename and never needs the server to
  // force attachment disposition.
  return `${baseUrl()}/sources/${encodeURIComponent(sourceId)}/messages/${encodeURIComponent(messageId)}/attachments/${encodeURIComponent(attachmentId)}`
}

export function buildMessageBodyUrl(
  sourceId: string,
  messageId: string,
  format: 'html' | 'text',
): string {
  // The body is served as a lazy resource (sanitized HTML or plain text), loaded
  // via an authenticated blob fetch like attachments — never inlined in detail.
  return `${baseUrl()}/sources/${encodeURIComponent(sourceId)}/messages/${encodeURIComponent(messageId)}/body?format=${format}`
}

export function buildAccountLogoUrl(imageId: string): string {
  return `${baseUrl()}/account-assets/logos/${encodeURIComponent(imageId)}`
}

/** @spec docs/L1-api#account-crud-lifecycle */
export function buildOAuthRedirectUri(): string {
  return `${baseUrl()}/oauth/callback`
}

/**
 * Build the SSE event stream URL, optionally resuming from a sequence number.
 * The stream is consumed via `fetchEventSource`, which authenticates with the
 * `Authorization` header (see {@link authHeaders}) — no token in the URL.
 * @spec docs/L1-api#sse-event-stream
 */
export function buildRuntimeLinkStreamUrl(input: {
  linkId: string
  afterSeq?: number | null
  sourceId?: string | null
}): string {
  const params = new URLSearchParams()
  if (input.afterSeq != null) {
    params.set('afterSeq', String(input.afterSeq))
  }
  if (input.sourceId) {
    params.set('sourceId', input.sourceId)
  }
  const search = params.toString()
  return `${baseUrl()}/runtime/sessions/${encodeURIComponent(input.linkId)}/stream${search ? `?${search}` : ''}`
}
