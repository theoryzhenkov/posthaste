/**
 * SSE event listener that receives domain events from the backend and
 * dispatches them as cache invalidations and browser `CustomEvent`s.
 *
 * Resumes from the last seen sequence number stored in `sessionStorage`.
 *
 * @spec docs/L1-api#sse-event-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { buildEventsUrl } from '../api/client'
import { syncLogger } from '../logger'
import type { DomainEvent } from '../api/types'
import { applyDomainEvent } from '../domainCache'
import { shouldSuppressLocalEcho } from '../mailState'

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
 * Opens an EventSource connection to the daemon SSE stream, processes
 * incoming domain events (keyword changes, mailbox changes, message arrivals),
 * and keeps the React Query cache in sync.
 *
 * @spec docs/L1-api#sse-event-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function useDaemonEvents() {
  const queryClient = useQueryClient()

  useEffect(() => {
    const storedSeq = window.sessionStorage.getItem(EVENT_CURSOR_STORAGE_KEY)
    const afterSeq = storedSeq ? Number.parseInt(storedSeq, 10) : null
    const source = new EventSource(
      buildEventsUrl({ afterSeq: Number.isFinite(afterSeq) ? afterSeq : null }),
    )

    source.onmessage = (event) => {
      let payload: DomainEvent
      try {
        payload = JSON.parse(event.data) as DomainEvent
      } catch (error) {
        syncLogger.warn(
          { error, raw: event.data },
          'ignoring malformed daemon event',
        )
        return
      }

      window.sessionStorage.setItem(
        EVENT_CURSOR_STORAGE_KEY,
        String(payload.seq),
      )

      if (shouldSuppressLocalEcho(payload)) {
        return
      }

      applyDomainEvent(queryClient, payload)
      dispatchDomainEvent(payload)
    }

    return () => {
      source.close()
    }
  }, [queryClient])
}
