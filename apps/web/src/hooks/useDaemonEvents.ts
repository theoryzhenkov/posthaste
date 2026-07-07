/**
 * Runtime notification listener that receives domain-event notifications from
 * the link-scoped `RuntimeFrame` stream and dispatches them as cache
 * invalidations and browser `CustomEvent`s.
 *
 * Stream resume is the near-end engine's job (M9b2): the engine owns the
 * `afterSeq` cursor and persists it across reloads — callers no longer thread
 * a sequence number.
 *
 * Subscribes through the runtime adapter so transport details (SSE, loopback
 * URLs, and bearer headers) stay outside UI code. The token no longer appears
 * in any URL.
 *
 * @spec docs/runtime/adapter/L1#renderer-one-frame-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { syncLogger } from '../logger'
import { LOG_EVENTS } from '../logEvents'
import { applyDomainEvent, isDomainEventShape } from '../domainCache'
import { runtimeLinkClient } from '../runtime/linkClient'

/**
 * Subscribes to runtime notification frames, processes incoming domain events
 * (keyword changes, mailbox changes, message arrivals), and keeps the React
 * Query cache in sync.
 *
 * @spec docs/runtime/adapter/L1#renderer-one-frame-stream
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function useDaemonEvents() {
  const queryClient = useQueryClient()

  useEffect(() => {
    const unsubscribe = runtimeLinkClient.subscribe({
      onFrame(frame) {
        if (frame.type !== 'notification') {
          return
        }
        if (!isDomainEventShape(frame.payload)) {
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
        // The renderer holds no optimistic overlay, so every runtime event is
        // authoritative and always applies — there is no local echo to drop.
        applyDomainEvent(queryClient, payload)
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
    })

    return unsubscribe
  }, [queryClient])
}
