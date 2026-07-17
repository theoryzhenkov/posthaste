// The ONE invalidation policy. When the event stream reports a generation
// advance, every query react-query holds is invalidated on a short debounce —
// active queries refetch, inactive ones refetch on next mount. There is no
// per-key invalidation, no cache surgery, and no folding of event payloads
// into cached answers; payloads may only prompt (see prompts below).
//
// Reconnect is the same policy: when the facade reports the stream back up,
// everything invalidates immediately, because anything may have happened
// while it was down.

import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'
import type { DomainEventPayload } from '@/gen'
import { useMailClient } from './context'

const DEBOUNCE_MS = 100

export function useStreamInvalidation(): void {
  const client = useMailClient()
  const queryClient = useQueryClient()

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null

    const invalidateAll = () => {
      timer = null
      void queryClient.invalidateQueries()
    }

    const unsubscribeGeneration = client.subscribeGeneration(() => {
      if (!timer) timer = setTimeout(invalidateAll, DEBOUNCE_MS)
    })

    let last = client.getConnectionStatus()
    const unsubscribeConnection = client.subscribeConnection(() => {
      const status = client.getConnectionStatus()
      if (status === 'connected' && last !== 'connected') {
        if (timer) clearTimeout(timer)
        invalidateAll()
      }
      last = status
    })

    return () => {
      unsubscribeGeneration()
      unsubscribeConnection()
      if (timer) clearTimeout(timer)
    }
  }, [client, queryClient])
}

/** Subscribes to domain-event prompts (`message.arrived`, `operation.settled`,
 * `*`). Reactions may notify, toast, or focus a refetch — the payload itself
 * is never written into anything the UI renders from. */
export function useDomainEvent(
  kind: string,
  onEvent: (payload: DomainEventPayload, generation: number) => void,
): void {
  const client = useMailClient()
  const ref = useRef(onEvent)
  useEffect(() => {
    ref.current = onEvent
  })
  useEffect(
    () => client.onEvent(kind, (payload, generation) => ref.current(payload, generation)),
    [client, kind],
  )
}
