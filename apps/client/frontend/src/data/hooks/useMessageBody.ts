/**
 * A message's readable body, from the `messageDetail` query family. The
 * backend serves sanitized HTML and the text/plain part inline on the detail
 * answer; the reader prefers HTML and falls back to plain text only when the
 * message has no HTML body. The shared family cache means the reply composer
 * quotes from the same answer the reader displayed (instant reply).
 */
import { useMessageDetail } from '@/data'

export interface MessageBodyResult {
  bodyHtml: string | null
  bodyText: string | null
  isLoading: boolean
  error: Error | null
}

/** The body pair a `messageDetail` answer carries. `bodyText` is the
 *  text/plain part the reply quote is built from. */
export interface CachedMessageBody {
  bodyHtml: string | null
  bodyText: string | null
}

export function useMessageBody(
  sourceId: string,
  messageId: string,
): MessageBodyResult {
  const detail = useMessageDetail({ accountId: sourceId, messageId })
  return {
    bodyHtml: detail.data?.bodyHtml ?? null,
    bodyText: detail.data?.bodyText ?? null,
    isLoading: detail.isLoading,
    error: detail.error,
  }
}
