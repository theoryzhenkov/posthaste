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
import { resetRuntimeSessionClientForTesting } from '../src/runtime/sessionClient'
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
  resetRuntimeSessionClientForTesting()
  resetRuntimeAdapterForTesting()
  queryClient.clear()
  window.sessionStorage.clear()
})

describe('useDaemonEvents runtime adapter subscription', () => {
  it('subscribes through the runtime session stream and dispatches notification events', async () => {
    const received: DomainEvent[] = []
    const listener = (receivedEvent: Event) => {
      received.push((receivedEvent as CustomEvent<DomainEvent>).detail)
    }
    window.addEventListener(MAIL_DOMAIN_EVENT_NAME, listener)

    try {
      const { unmount } = renderHook(() => useDaemonEvents(), { wrapper })

      await waitFor(() =>
        expect(runtimeAdapter.runtimeFrameSubscriptionCalls).toEqual([
          { request: { sessionId: 'session-1', afterSeq: null } },
        ]),
      )
      expect(runtimeAdapter.eventSubscriptionCalls).toEqual([])
      runtimeAdapter.emitRuntimeFrame({
        type: 'notification',
        sessionSeq: 1,
        kind: event.topic,
        payload: event,
      })

      await waitFor(() => expect(received).toEqual([event]))
      expect(window.sessionStorage.getItem('mail:last-runtime-frame-seq')).toBe(
        '1',
      )

      unmount()
      await waitFor(() =>
        expect(runtimeAdapter.runtimeSessionCloseCalls).toEqual([
          { sessionId: 'session-1', sourceId: undefined },
        ]),
      )
      runtimeAdapter.emitRuntimeFrame({
        type: 'notification',
        sessionSeq: 2,
        kind: event.topic,
        payload: { ...event, seq: 43 },
      })
      expect(received).toEqual([event])
    } finally {
      window.removeEventListener(MAIL_DOMAIN_EVENT_NAME, listener)
    }
  })

  it('resumes from the stored runtime frame sequence', async () => {
    window.sessionStorage.setItem('mail:last-runtime-frame-seq', '7')

    renderHook(() => useDaemonEvents(), { wrapper })

    await waitFor(() =>
      expect(runtimeAdapter.runtimeFrameSubscriptionCalls).toEqual([
        { request: { sessionId: 'session-1', afterSeq: 7 } },
      ]),
    )
  })
})
