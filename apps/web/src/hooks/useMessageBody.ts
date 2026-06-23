/**
 * Fetch a message's body as a lazy resource (the same `runtime/resources.ts`
 * layer attachments use), rather than reading it inline from message detail.
 *
 * Prefers sanitized HTML; falls back to plain text only when the message has no
 * HTML body. The body never travels in the detail payload and is sanitized at a
 * single server chokepoint (`GET .../body`).
 */
import { useEffect, useState } from 'react'

import { LOG_EVENTS } from '../logEvents'
import { syncLogger } from '../logger'
import { runtimeResources } from '../runtime/resources'

export interface MessageBodyResult {
  bodyHtml: string | null
  bodyText: string | null
  isLoading: boolean
  error: Error | null
}

interface BodyState {
  key: string
  bodyHtml: string | null
  bodyText: string | null
  error: Error | null
}

export function useMessageBody(
  sourceId: string,
  messageId: string,
): MessageBodyResult {
  const targetKey = `${sourceId}\u0000${messageId}`
  const [state, setState] = useState<BodyState | null>(null)

  useEffect(() => {
    const controller = new AbortController()

    void (async () => {
      try {
        const html = await runtimeResources.text(
          { kind: 'message-body', sourceId, messageId, format: 'html' },
          { signal: controller.signal },
        )
        if (controller.signal.aborted) {
          return
        }
        if (html.trim()) {
          setState({
            key: targetKey,
            bodyHtml: html,
            bodyText: null,
            error: null,
          })
          return
        }
        // HTML-less message: fall back to the plain-text body.
        const text = await runtimeResources.text(
          { kind: 'message-body', sourceId, messageId, format: 'text' },
          { signal: controller.signal },
        )
        if (controller.signal.aborted) {
          return
        }
        setState({
          key: targetKey,
          bodyHtml: null,
          bodyText: text,
          error: null,
        })
      } catch (error) {
        if (controller.signal.aborted) {
          return
        }
        syncLogger.warn(
          { event: LOG_EVENTS.resourceFetchError, sourceId, messageId },
          'failed to load message body',
        )
        setState({
          key: targetKey,
          bodyHtml: null,
          bodyText: null,
          error:
            error instanceof Error ? error : new Error('failed to load body'),
        })
      }
    })()

    return () => controller.abort()
  }, [targetKey, sourceId, messageId])

  // Loading whenever the settled state isn't for the currently requested message.
  const ready = state?.key === targetKey ? state : null
  return {
    bodyHtml: ready?.bodyHtml ?? null,
    bodyText: ready?.bodyText ?? null,
    isLoading: ready === null,
    error: ready?.error ?? null,
  }
}
