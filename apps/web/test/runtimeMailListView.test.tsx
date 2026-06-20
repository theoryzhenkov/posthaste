import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import type { MessageSummary } from '../src/api/types'
import { useRuntimeMailListView } from '../src/components/message-list/useRuntimeMailListView'
import { useDaemonEvents } from '../src/hooks/useDaemonEvents'
import { queryKeys } from '../src/queryKeys'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import { resetRuntimeSessionClientForTesting } from '../src/runtime/sessionClient'
import type {
  RuntimeMailListViewState,
  RuntimeViewSnapshot,
} from '../src/runtime/types'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

let queryClient: QueryClient
let runtimeAdapter: FakeRuntimeAdapter

const message: MessageSummary = {
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

const updatedMessage: MessageSummary = {
  ...message,
  isFlagged: true,
  keywords: ['$flagged', '$seen'],
}

function mailListSnapshot(
  revision: number,
  row: MessageSummary,
): RuntimeViewSnapshot<RuntimeMailListViewState> {
  return {
    viewId: 'view-1',
    descriptor: { family: 'mailList', payload: {} },
    revision,
    lifecycle: 'ready',
    readWatermark: null,
    coverage: { kind: 'complete' },
    data: {
      scope: null,
      projectionKind: 'message',
      sort: null,
      windowRequest: null,
      rows: [
        {
          rowKey: `${row.sourceId}:${row.id}`,
          resourceRef: null,
          projection: row,
          orderKey: row.id,
        },
      ],
      continuation: {
        beforeCursor: null,
        afterCursor: null,
        hasBefore: false,
        hasAfter: false,
      },
      readWatermark: null,
      coverage: { kind: 'complete' },
      knownTotalCount: 1,
      pendingMutations: [],
      anchor: null,
    },
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
  resetRuntimeSessionClientForTesting()
  resetRuntimeAdapterForTesting()
  queryClient.clear()
})

describe('useRuntimeMailListView', () => {
  it('opens a runtime view and applies replace frames with targeted setQueryData', async () => {
    const queryKey = queryKeys.messages(
      { kind: 'source-mailbox', sourceId: 'primary', mailboxId: 'inbox' },
      undefined,
      { columnId: 'date', direction: 'desc' },
    )
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeSessionMessageListView({
      viewId: 'view-1',
      snapshot: mailListSnapshot(1, message),
    })

    const { unmount } = renderHook(
      () =>
        useRuntimeMailListView({
          enabled: true,
          operation: {
            operationId: 'op_1',
            operationKind: 'mail.list',
            operationSource: 'test',
            sessionId: 'session_1',
          },
          preparedSearchQuery: {
            query: undefined,
            validation: { state: 'valid' },
            isBlocked: false,
          },
          queryKey,
          selectedView: {
            kind: 'source-mailbox',
            sourceId: 'primary',
            mailboxId: 'inbox',
          },
          sort: { columnId: 'date', direction: 'desc' },
        }),
      { wrapper },
    )

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [message], nextCursor: null }],
        pageParams: [null],
      }),
    )
    expect(runtimeAdapter.messagePageCalls).toEqual([])
    expect(runtimeAdapter.runtimeSessionCalls).toEqual([
      { sourceId: 'primary' },
    ])
    expect(runtimeAdapter.runtimeSessionViewOpenCalls).toHaveLength(1)
    expect(runtimeAdapter.runtimeSessionViewOpenCalls[0].sourceId).toBe(
      'primary',
    )
    expect(runtimeAdapter.runtimeFrameSubscriptionCalls).toEqual([
      { request: { sessionId: 'session-1', afterSeq: 0, sourceId: 'primary' } },
    ])

    runtimeAdapter.emitRuntimeFrame({
      type: 'viewReplace',
      sessionSeq: 2,
      viewId: 'view-1',
      revision: 2,
      snapshot: mailListSnapshot(2, updatedMessage),
    })

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [updatedMessage], nextCursor: null }],
        pageParams: [null],
      }),
    )

    unmount()
    await waitFor(() =>
      expect(runtimeAdapter.runtimeSessionViewCloseCalls).toEqual([
        { sessionId: 'session-1', viewId: 'view-1', sourceId: 'primary' },
      ]),
    )
    await waitFor(() =>
      expect(runtimeAdapter.runtimeSessionCloseCalls).toEqual([
        { sessionId: 'session-1', sourceId: 'primary' },
      ]),
    )
  })

  it('shares the renderer runtime stream with notification subscribers', async () => {
    const queryKey = queryKeys.messages(
      { kind: 'source-mailbox', sourceId: 'primary', mailboxId: 'inbox' },
      undefined,
      { columnId: 'date', direction: 'desc' },
    )
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeSessionMessageListView({
      viewId: 'view-1',
      snapshot: mailListSnapshot(1, message),
    })

    renderHook(
      () => {
        useDaemonEvents()
        useRuntimeMailListView({
          enabled: true,
          operation: {
            operationId: 'op_1',
            operationKind: 'mail.list',
            operationSource: 'test',
            sessionId: 'session_1',
          },
          preparedSearchQuery: {
            query: undefined,
            validation: { state: 'valid' },
            isBlocked: false,
          },
          queryKey,
          selectedView: {
            kind: 'source-mailbox',
            sourceId: 'primary',
            mailboxId: 'inbox',
          },
          sort: { columnId: 'date', direction: 'desc' },
        })
      },
      { wrapper },
    )

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [message], nextCursor: null }],
        pageParams: [null],
      }),
    )
    expect(runtimeAdapter.runtimeSessionCalls).toEqual([{}])
    expect(runtimeAdapter.runtimeFrameSubscriptionCalls).toEqual([
      { request: { sessionId: 'session-1', afterSeq: null } },
    ])
    expect(runtimeAdapter.runtimeSessionViewOpenCalls).toHaveLength(1)
    expect(runtimeAdapter.runtimeSessionViewOpenCalls[0].sourceId).toBe(
      undefined,
    )
  })
})
