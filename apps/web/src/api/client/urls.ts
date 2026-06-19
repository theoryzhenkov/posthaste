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
export function buildRuntimeSessionStreamUrl(input: {
  sessionId: string
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
  return `${baseUrl()}/runtime/sessions/${encodeURIComponent(input.sessionId)}/stream${search ? `?${search}` : ''}`
}

export function buildViewStreamUrl(input: {
  viewId: string
  afterRevision?: number | null
  sourceId?: string | null
}): string {
  const params = new URLSearchParams()
  if (input.afterRevision != null) {
    params.set('afterRevision', String(input.afterRevision))
  }
  if (input.sourceId) {
    params.set('sourceId', input.sourceId)
  }
  const search = params.toString()
  return `${baseUrl()}/views/${encodeURIComponent(input.viewId)}/stream${search ? `?${search}` : ''}`
}

export function buildEventsUrl(input?: {
  accountId?: string
  afterSeq?: number | null
}): string {
  const params = new URLSearchParams()
  if (input?.accountId) {
    params.set('accountId', input.accountId)
  }
  if (input?.afterSeq != null) {
    params.set('afterSeq', String(input.afterSeq))
  }
  const search = params.toString()
  return `${baseUrl()}/events${search ? `?${search}` : ''}`
}
