import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import type { MessageDetail, MessageSummary } from '../src/api/types'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import { resetRuntimeLinkClientForTesting } from '../src/runtime/linkClient'
import type { RuntimeViewSnapshot } from '../src/runtime/types'
import { useRuntimeObjectView } from '../src/runtime/useRuntimeObjectView'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

let queryClient: QueryClient
let runtimeAdapter: FakeRuntimeAdapter

const summary: MessageSummary = {
  id: 'm1',
  sourceId: 'primary',
  sourceName: 'Primary',
  sourceThreadId: 't1',
  conversationId: 'c1',
  subject: 'Subject',
  fromName: 'Sender',
  fromEmail: 'sender@example.com',
  to: [],
  preview: 'Preview',
  receivedAt: '2026-04-28T12:00:00Z',
  hasAttachment: false,
  isRead: true,
  isFlagged: false,
  mailboxIds: ['inbox'],
  keywords: ['$seen'],
}

const detailWithBody: MessageDetail = {
  ...summary,
  bodyHtml: '<p>Hello</p>',
  bodyText: 'Hello',
  attachments: [],
}

const flaggedHeaderOnly: MessageDetail = {
  ...summary,
  isFlagged: true,
  keywords: ['$flagged', '$seen'],
  bodyHtml: null,
  bodyText: null,
  attachments: [],
}

function snapshot<TData>(
  family: string,
  revision: number,
  data: TData,
): RuntimeViewSnapshot<TData> {
  return {
    viewId: 'view-1',
    descriptor: { family, payload: {} },
    revision,
    lifecycle: 'ready',
    readWatermark: null,
    coverage: { kind: 'complete' },
    data,
    pendingMutations: [],
    error: null,
  }
}

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )
}

beforeEach(() => {
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
})

describe('useRuntimeObjectView', () => {
  it('opens an object view by descriptor, seeds the cache, and applies replaces', async () => {
    const queryKey = ['conversation', 'c1'] as const
    runtimeAdapter.queueRuntimeLinkConnection({ linkId: 'link-1' })
    runtimeAdapter.queueRuntimeLinkView({
      viewId: 'view-1',
      snapshot: snapshot('conversation', 1, { conversationId: 'c1', count: 1 }),
    })

    const { unmount } = renderHook(
      () =>
        useRuntimeObjectView({
          enabled: true,
          family: 'conversation',
          payload: { conversationId: 'c1' },
          queryKey,
          sourceId: 'primary',
        }),
      { wrapper },
    )

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        conversationId: 'c1',
        count: 1,
      }),
    )
    expect(runtimeAdapter.runtimeLinkObjectViewOpenCalls).toHaveLength(1)
    expect(runtimeAdapter.runtimeLinkObjectViewOpenCalls[0].descriptor).toEqual(
      { family: 'conversation', payload: { conversationId: 'c1' } },
    )

    runtimeAdapter.emitRuntimeFrame({
      type: 'viewReplace',
      linkSeq: 2,
      viewId: 'view-1',
      revision: 2,
      snapshot: snapshot('conversation', 2, { conversationId: 'c1', count: 2 }),
    })

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        conversationId: 'c1',
        count: 2,
      }),
    )

    unmount()
    await waitFor(() =>
      expect(runtimeAdapter.runtimeLinkViewCloseCalls).toEqual([
        { linkId: 'link-1', viewId: 'view-1', sourceId: 'primary' },
      ]),
    )
  })

  it('merges header-only detail replaces without dropping the loaded body', async () => {
    const queryKey = ['message', 'primary', 'm1'] as const
    // The HTTP detail query has already loaded the body.
    queryClient.setQueryData(queryKey, detailWithBody)
    runtimeAdapter.queueRuntimeLinkConnection({ linkId: 'link-1' })
    runtimeAdapter.queueRuntimeLinkView({
      viewId: 'view-1',
      snapshot: snapshot('messageDetail', 1, flaggedHeaderOnly),
    })

    const merge = (
      previous: MessageDetail | undefined,
      next: MessageDetail,
    ): MessageDetail => {
      const nextHasBody = next.bodyHtml != null || next.bodyText != null
      if (nextHasBody || !previous) {
        return next
      }
      return {
        ...next,
        bodyHtml: previous.bodyHtml,
        bodyText: previous.bodyText,
        attachments: previous.attachments,
      }
    }

    renderHook(
      () =>
        useRuntimeObjectView<MessageDetail>({
          enabled: true,
          family: 'messageDetail',
          merge,
          payload: { sourceId: 'primary', messageId: 'm1' },
          queryKey,
          sourceId: 'primary',
        }),
      { wrapper },
    )

    await waitFor(() => {
      const data = queryClient.getQueryData<MessageDetail>(queryKey)
      expect(data?.isFlagged).toBe(true)
      // Optimistic header update kept the previously-loaded body.
      expect(data?.bodyHtml).toBe('<p>Hello</p>')
    })
  })
})
