/**
 * SSE event listener that receives domain events from the backend and
 * dispatches them as cache invalidations and browser `CustomEvent`s.
 *
 * Resumes from the last seen sequence number stored in `sessionStorage`.
 *
 * Subscribes through the runtime adapter so transport details (SSE, loopback
 * URLs, and bearer headers) stay outside UI code. The token no longer appears
 * in any URL.
 *
 * @spec docs/L1-api#sse-event-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { syncLogger } from '../logger'
import { LOG_EVENTS } from '../logEvents'
import type { DomainEvent } from '../api/types'
import { applyDomainEvent } from '../domainCache'
import { shouldSuppressLocalEcho } from '../mailState'
import { runtimeSubscriptions } from '../runtime/subscriptions'

/** `sessionStorage` key for the last processed event sequence number. */
const EVENT_CURSOR_STORAGE_KEY = 'mail:last-event-seq'

/** Custom browser event name used to relay domain events to components. */
export const MAIL_DOMAIN_EVENT_NAME = 'mail:domain-event'

/** Re-dispatch a domain event as a browser `CustomEvent` for component listeners. */
function dispatchDomainEvent(payload: DomainEvent) {
  window.dispatchEvent(
    new CustomEvent<DomainEvent>(MAIL_DOMAIN_EVENT_NAME, { detail: payload }),
  )
}

/**
 * Subscribes to the runtime domain-event stream, processes incoming events
 * (keyword changes, mailbox changes, message arrivals), and keeps the React
 * Query cache in sync.
 *
 * @spec docs/L1-api#sse-event-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function useDaemonEvents() {
  const queryClient = useQueryClient()

  useEffect(() => {
    const storedSeq = window.sessionStorage.getItem(EVENT_CURSOR_STORAGE_KEY)
    const afterSeq = storedSeq ? Number.parseInt(storedSeq, 10) : null
    const unsubscribe = runtimeSubscriptions.events(
      { afterSeq: Number.isFinite(afterSeq) ? afterSeq : null },
      {
        onEvent(payload) {
          window.sessionStorage.setItem(
            EVENT_CURSOR_STORAGE_KEY,
            String(payload.seq),
          )

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

    return unsubscribe
  }, [queryClient])
}
