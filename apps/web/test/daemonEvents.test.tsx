import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import type { DomainEvent } from '../src/api/types'
import { useDaemonEvents } from '../src/hooks/useDaemonEvents'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import { resetRuntimeLinkClientForTesting } from '../src/runtime/linkClient'
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
  resetRuntimeLinkClientForTesting()
  resetRuntimeAdapterForTesting()
  queryClient.clear()
  window.sessionStorage.clear()
})

describe('useDaemonEvents runtime adapter subscription', () => {
  it('subscribes through the runtime link stream without threading a cursor', async () => {
    const { unmount } = renderHook(() => useDaemonEvents(), { wrapper })

    // Stream resume is the near-end engine's job (M9b2): the hook passes no
    // afterSeq — the engine owns and persists the cursor.
    await waitFor(() =>
      expect(runtimeAdapter.runtimeFrameSubscriptionCalls).toEqual([
        { request: { linkId: 'link-1' } },
      ]),
    )
    runtimeAdapter.emitRuntimeFrame({
      type: 'notification',
      linkSeq: 1,
      kind: event.topic,
      payload: event,
    })

    // The hook applies the event to the query cache but does NOT touch the
    // cursor storage — that moved behind the engine binding.
    expect(window.sessionStorage.getItem('mail:last-runtime-frame-seq')).toBe(
      null,
    )

    unmount()
    await waitFor(() =>
      expect(runtimeAdapter.runtimeLinkCloseCalls).toEqual([
        { linkId: 'link-1', sourceId: undefined },
      ]),
    )
  })

  it('keeps a single subscription across a stored legacy cursor (no afterSeq resume here)', async () => {
    // A cursor persisted by the engine binding must not leak back into the
    // hook's subscription request.
    window.sessionStorage.setItem('mail:last-runtime-frame-seq', '7')

    renderHook(() => useDaemonEvents(), { wrapper })

    await waitFor(() =>
      expect(runtimeAdapter.runtimeFrameSubscriptionCalls).toEqual([
        { request: { linkId: 'link-1' } },
      ]),
    )
  })
})
