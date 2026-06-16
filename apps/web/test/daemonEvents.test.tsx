import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import type { DomainEvent } from '../src/api/types'
import {
  MAIL_DOMAIN_EVENT_NAME,
  useDaemonEvents,
} from '../src/hooks/useDaemonEvents'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

let queryClient: QueryClient
let runtimeAdapter: FakeRuntimeAdapter

const event: DomainEvent = {
  seq: 42,
  accountId: 'primary',
  topic: 'message.updated',
  occurredAt: '2026-04-28T12:00:00Z',
  mailboxId: null,
  messageId: 'm1',
  payload: {},
}

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )
}

beforeEach(() => {
  window.sessionStorage.clear()
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  runtimeAdapter = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting(runtimeAdapter)
})

afterEach(() => {
  resetRuntimeAdapterForTesting()
  queryClient.clear()
  window.sessionStorage.clear()
})

describe('useDaemonEvents runtime adapter subscription', () => {
  it('subscribes through the runtime adapter and dispatches domain events', async () => {
    const received: DomainEvent[] = []
    const listener = (receivedEvent: Event) => {
      received.push((receivedEvent as CustomEvent<DomainEvent>).detail)
    }
    window.addEventListener(MAIL_DOMAIN_EVENT_NAME, listener)

    try {
      const { unmount } = renderHook(() => useDaemonEvents(), { wrapper })

      expect(runtimeAdapter.eventSubscriptionCalls).toEqual([
        { request: { afterSeq: null } },
      ])
      runtimeAdapter.emitDomainEvent(event)

      await waitFor(() => expect(received).toEqual([event]))
      expect(window.sessionStorage.getItem('mail:last-event-seq')).toBe('42')

      unmount()
      runtimeAdapter.emitDomainEvent({ ...event, seq: 43 })
      expect(received).toEqual([event])
    } finally {
      window.removeEventListener(MAIL_DOMAIN_EVENT_NAME, listener)
    }
  })

  it('resumes from the stored event sequence', () => {
    window.sessionStorage.setItem('mail:last-event-seq', '7')

    renderHook(() => useDaemonEvents(), { wrapper })

    expect(runtimeAdapter.eventSubscriptionCalls).toEqual([
      { request: { afterSeq: 7 } },
    ])
  })
})
