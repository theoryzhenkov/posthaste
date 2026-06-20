/**
 * Runtime notification listener that receives domain-event notifications from
 * the session-scoped `RuntimeFrame` stream and dispatches them as cache
 * invalidations and browser `CustomEvent`s.
 *
 * Resumes from the last seen runtime frame sequence number stored in
 * `sessionStorage`.
 *
 * Subscribes through the runtime adapter so transport details (SSE, loopback
 * URLs, and bearer headers) stay outside UI code. The token no longer appears
 * in any URL.
 *
 * @spec docs/runtime/L2#renderer-one-frame-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { syncLogger } from '../logger'
import { LOG_EVENTS } from '../logEvents'
import type { DomainEvent } from '../api/types'
import { applyDomainEvent } from '../domainCache'
import { shouldSuppressLocalEcho } from '../mailState'
import { runtimeStream } from '../runtime/runtimeStream'

/** `sessionStorage` key for the last processed runtime frame sequence number. */
const EVENT_CURSOR_STORAGE_KEY = 'mail:last-runtime-frame-seq'

/** Custom browser event name used to relay domain events to components. */
export const MAIL_DOMAIN_EVENT_NAME = 'mail:domain-event'

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isDomainEventPayload(payload: unknown): payload is DomainEvent {
  if (!isRecord(payload)) {
    return false
  }
  return (
    typeof payload.seq === 'number' &&
    typeof payload.accountId === 'string' &&
    typeof payload.topic === 'string' &&
    typeof payload.occurredAt === 'string' &&
    isRecord(payload.payload)
  )
}

/** Re-dispatch a domain event as a browser `CustomEvent` for component listeners. */
function dispatchDomainEvent(payload: DomainEvent) {
  window.dispatchEvent(
    new CustomEvent<DomainEvent>(MAIL_DOMAIN_EVENT_NAME, { detail: payload }),
  )
}

/**
 * Subscribes to runtime notification frames, processes incoming domain events
 * (keyword changes, mailbox changes, message arrivals), and keeps the React
 * Query cache in sync.
 *
 * @spec docs/runtime/L2#renderer-one-frame-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function useDaemonEvents() {
  const queryClient = useQueryClient()

  useEffect(() => {
    const storedSeq = window.sessionStorage.getItem(EVENT_CURSOR_STORAGE_KEY)
    const afterSeq = storedSeq ? Number.parseInt(storedSeq, 10) : null
    let closed = false
    let sessionId: string | undefined
    let unsubscribe: (() => void) | undefined

    const closeSession = () => {
      if (!sessionId) {
        return
      }
      const closingSessionId = sessionId
      sessionId = undefined
      void runtimeStream.closeSession(closingSessionId).catch(() => {})
    }

    void runtimeStream
      .openSession({})
      .then((session) => {
        sessionId = session.sessionId
        if (closed) {
          closeSession()
          return
        }
        unsubscribe = runtimeStream.subscribe(
          {
            sessionId: session.sessionId,
            afterSeq: Number.isFinite(afterSeq) ? afterSeq : null,
          },
          {
            onFrame(frame) {
              window.sessionStorage.setItem(
                EVENT_CURSOR_STORAGE_KEY,
                String(frame.sessionSeq),
              )
              if (frame.type !== 'notification') {
                return
              }
              if (!isDomainEventPayload(frame.payload)) {
                syncLogger.warn(
                  {
                    event: LOG_EVENTS.daemonEventMalformed,
                    raw: JSON.stringify(frame.payload),
                  },
                  'ignoring malformed runtime notification',
                )
                return
              }
              const payload = frame.payload

              if (shouldSuppressLocalEcho(payload)) {
                return
              }

              applyDomainEvent(queryClient, payload)
              dispatchDomainEvent(payload)
            },
            onMalformedFrame({ raw, error }) {
              syncLogger.warn(
                { event: LOG_EVENTS.daemonEventMalformed, error, raw },
                'ignoring malformed daemon event',
              )
            },
            onPermanentError(error) {
              syncLogger.warn(
                { event: LOG_EVENTS.daemonEventStreamError, error },
                'daemon event stream failed permanently',
              )
            },
            onTransientError(error) {
              syncLogger.warn(
                { event: LOG_EVENTS.daemonEventStreamError, error },
                'daemon event stream disconnected; reconnecting',
              )
            },
            onClosed(error) {
              syncLogger.warn(
                { event: LOG_EVENTS.daemonEventStreamError, error },
                'daemon event stream closed',
              )
            },
          },
        )
      })
      .catch((error) => {
        syncLogger.warn(
          { event: LOG_EVENTS.daemonEventStreamError, error },
          'daemon event stream failed permanently',
        )
      })

    return () => {
      closed = true
      unsubscribe?.()
      closeSession()
    }
  }, [queryClient])
}
