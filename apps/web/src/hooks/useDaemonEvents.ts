/**
 * SSE event listener that receives domain events from the backend and
 * dispatches them as cache invalidations and browser `CustomEvent`s.
 *
 * Resumes from the last seen sequence number stored in `sessionStorage`.
 *
 * Uses `fetchEventSource` (a `fetch()`-backed SSE client) rather than the
 * native `EventSource` so the stream can authenticate with the `Authorization`
 * header — the native `EventSource` cannot set headers, which is why the token
 * used to ride in an `?access_token=` query param. The token no longer appears
 * in any URL.
 *
 * @spec docs/L1-api#sse-event-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  fetchEventSource,
  EventStreamContentType,
} from '@microsoft/fetch-event-source'
import { authHeaders, buildEventsUrl } from '../api/client'
import { syncLogger } from '../logger'
import { LOG_EVENTS } from '../logEvents'
import type { DomainEvent } from '../api/types'
import { applyDomainEvent } from '../domainCache'
import { shouldSuppressLocalEcho } from '../mailState'

/** `sessionStorage` key for the last processed event sequence number. */
const EVENT_CURSOR_STORAGE_KEY = 'mail:last-event-seq'

/** Custom browser event name used to relay domain events to components. */
export const MAIL_DOMAIN_EVENT_NAME = 'mail:domain-event'

/**
 * A non-retriable stream failure (e.g. auth rejected). Throwing this from
 * `onopen`/`onerror` tells `fetchEventSource` to stop rather than reconnect, so
 * a 401/403 does not turn into a reconnect storm.
 */
class FatalStreamError extends Error {}

/** Re-dispatch a domain event as a browser `CustomEvent` for component listeners. */
function dispatchDomainEvent(payload: DomainEvent) {
  window.dispatchEvent(
    new CustomEvent<DomainEvent>(MAIL_DOMAIN_EVENT_NAME, { detail: payload }),
  )
}

/**
 * Opens a `fetch()`-backed SSE connection to the daemon stream, processes
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
    const controller = new AbortController()

    void fetchEventSource(
      buildEventsUrl({ afterSeq: Number.isFinite(afterSeq) ? afterSeq : null }),
      {
        headers: authHeaders(),
        signal: controller.signal,
        // Match the native EventSource: stay connected while the tab is hidden
        // (the default pauses the stream on hidden, which would drop events).
        openWhenHidden: true,

        async onopen(response) {
          const contentType = response.headers.get('content-type') ?? ''
          if (response.ok && contentType.startsWith(EventStreamContentType)) {
            return
          }
          // 4xx (auth/bad request) is fatal — reconnecting cannot fix it.
          // 5xx and anything else is retriable (transient daemon hiccup).
          if (response.status >= 400 && response.status < 500) {
            throw new FatalStreamError(
              `event stream rejected with ${response.status}`,
            )
          }
          throw new Error(`event stream returned ${response.status}`)
        },

        onmessage(event) {
          // Keep-alive comments and other non-data frames arrive with empty
          // data; ignore them (the parser already strips `:` comments).
          if (!event.data) {
            return
          }
          let payload: DomainEvent
          try {
            payload = JSON.parse(event.data) as DomainEvent
          } catch (error) {
            syncLogger.warn(
              {
                event: LOG_EVENTS.daemonEventMalformed,
                error,
                raw: event.data,
              },
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
        },

        onerror(error) {
          if (error instanceof FatalStreamError) {
            syncLogger.warn(
              { event: LOG_EVENTS.daemonEventStreamError, error },
              'daemon event stream failed permanently',
            )
            // Rethrow to stop fetchEventSource from reconnecting.
            throw error
          }
          syncLogger.warn(
            { event: LOG_EVENTS.daemonEventStreamError, error },
            'daemon event stream disconnected; reconnecting',
          )
          // Returning nothing lets fetchEventSource retry with its backoff.
        },
      },
    ).catch((error) => {
      // The promise rejects when the stream stops permanently (fatal error) or
      // when the AbortController fires on unmount; the latter is expected. A
      // FatalStreamError was already logged in onerror, so don't log it twice.
      if (controller.signal.aborted || error instanceof FatalStreamError) {
        return
      }
      syncLogger.warn(
        { event: LOG_EVENTS.daemonEventStreamError, error },
        'daemon event stream closed',
      )
    })

    return () => {
      controller.abort()
    }
  }, [queryClient])
}
