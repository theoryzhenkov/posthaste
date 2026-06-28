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

function mailListSnapshotRows(
  revision: number,
  rows: MessageSummary[],
  hasAfter: boolean,
): RuntimeViewSnapshot<RuntimeMailListViewState> {
  const base = mailListSnapshot(revision, rows[0])
  return {
    ...base,
    revision,
    data: {
      ...base.data,
      rows: rows.map((row) => ({
        rowKey: `${row.sourceId}:${row.id}`,
        resourceRef: null,
        projection: row,
        orderKey: row.id,
      })),
      continuation: {
        beforeCursor: null,
        afterCursor: hasAfter ? 'cursor-1' : null,
        hasBefore: false,
        hasAfter,
      },
    },
  }
}

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )
}

// Stable input across the hook's own re-renders (the production caller memoizes
// these). The hook re-renders when its rows change, so inline objects would make
// the open-view effect re-run on every frame — see the loadMore test's note.
function mailListHookInput(queryKey: readonly unknown[]) {
  return {
    enabled: true,
    operation: {
      operationId: 'op_1',
      operationKind: 'mail.list',
      operationSource: 'test',
      sessionId: 'session_1',
    },
    preparedSearchQuery: {
      query: undefined,
      validation: { state: 'valid' as const },
      isBlocked: false,
    },
    queryKey,
    selectedView: {
      kind: 'source-mailbox' as const,
      sourceId: 'primary',
      mailboxId: 'inbox',
    },
    sort: { columnId: 'date', direction: 'desc' as const },
  }
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

    const hookInput = mailListHookInput(queryKey)
    const { result, unmount } = renderHook(
      () => useRuntimeMailListView(hookInput),
      { wrapper },
    )

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [message], nextCursor: null }],
        pageParams: [null],
      }),
    )
    // The hook also returns the rows as its view-model (the component reads this).
    expect(result.current.items).toEqual([message])
    expect(runtimeAdapter.messagePageCalls).toEqual([])
    expect(runtimeAdapter.runtimeSessionCalls).toEqual([
      { sourceId: 'primary', viewDelta: true },
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
    expect(result.current.items).toEqual([updatedMessage])

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

  it('applies a viewDelta in place (upsert) and on removal (reorder)', async () => {
    const queryKey = queryKeys.messages(
      { kind: 'source-mailbox', sourceId: 'primary', mailboxId: 'inbox' },
      undefined,
      { columnId: 'date', direction: 'desc' },
    )
    const secondMessage: MessageSummary = { ...message, id: 'm2' }
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeSessionMessageListView({
      viewId: 'view-1',
      snapshot: mailListSnapshotRows(1, [message, secondMessage], false),
    })

    const hookInput = mailListHookInput(queryKey)
    renderHook(() => useRuntimeMailListView(hookInput), { wrapper })

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [message, secondMessage], nextCursor: null }],
        pageParams: [null],
      }),
    )

    // Upsert in place: flag m1, no reorder — only the changed row is sent.
    runtimeAdapter.emitRuntimeFrame({
      type: 'viewDelta',
      sessionSeq: 2,
      viewId: 'view-1',
      revision: 2,
      delta: {
        order: null,
        upserts: [
          {
            rowKey: 'primary:m1',
            resourceRef: null,
            projection: updatedMessage,
            orderKey: 'm1',
          },
        ],
      },
    })
    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [updatedMessage, secondMessage], nextCursor: null }],
        pageParams: [null],
      }),
    )

    // Removal: m1 leaves the view — the new order drops it, no upserts.
    runtimeAdapter.emitRuntimeFrame({
      type: 'viewDelta',
      sessionSeq: 3,
      viewId: 'view-1',
      revision: 3,
      delta: { order: ['primary:m2'], upserts: [] },
    })
    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [secondMessage], nextCursor: null }],
        pageParams: [null],
      }),
    )
  })

  it('grows the window in place via the runtime extend operation', async () => {
    const queryKey = queryKeys.messages(
      { kind: 'source-mailbox', sourceId: 'primary', mailboxId: 'inbox' },
      undefined,
      { columnId: 'date', direction: 'desc' },
    )
    const secondMessage: MessageSummary = { ...message, id: 'm2' }
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeSessionMessageListView({
      viewId: 'view-1',
      snapshot: mailListSnapshotRows(1, [message], true),
    })
    runtimeAdapter.queueRuntimeSessionViewExtend({
      viewId: 'view-1',
      snapshot: mailListSnapshotRows(2, [message, secondMessage], false),
    })

    // Stable references so the hook effect doesn't re-run on state updates
    // (memoized by the caller in the app).
    const hookInput = {
      enabled: true,
      operation: {
        operationId: 'op_1',
        operationKind: 'mail.list',
        operationSource: 'test',
        sessionId: 'session_1',
      },
      preparedSearchQuery: {
        query: undefined,
        validation: { state: 'valid' as const },
        isBlocked: false,
      },
      queryKey,
      selectedView: {
        kind: 'source-mailbox' as const,
        sourceId: 'primary',
        mailboxId: 'inbox',
      },
      sort: { columnId: 'date', direction: 'desc' as const },
    }
    const { result } = renderHook(() => useRuntimeMailListView(hookInput), {
      wrapper,
    })

    await waitFor(() => expect(result.current.hasMore).toBe(true))

    result.current.loadMore()

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [message, secondMessage], nextCursor: null }],
        pageParams: [null],
      }),
    )
    expect(result.current.hasMore).toBe(false)
    expect(runtimeAdapter.runtimeSessionViewExtendCalls).toEqual([
      {
        sessionId: 'session-1',
        viewId: 'view-1',
        count: 100,
        sourceId: 'primary',
      },
    ])
  })

  it('surfaces a thrown open as `error` and clears `isLoading`', async () => {
    const queryKey = queryKeys.messages(
      { kind: 'source-mailbox', sourceId: 'primary', mailboxId: 'inbox' },
      undefined,
      { columnId: 'date', direction: 'desc' },
    )
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    // The IndexedDB VersionError class of failure: view open rejects, so the
    // view never opens and no snapshot ever lands.
    runtimeAdapter.queueRuntimeSessionMessageListViewError(
      new Error('IndexedDB VersionError'),
    )

    const hookInput = mailListHookInput(queryKey)
    const { result } = renderHook(() => useRuntimeMailListView(hookInput), {
      wrapper,
    })

    await waitFor(() => expect(result.current.error).not.toBeNull())
    expect(result.current.error?.message).toBe('IndexedDB VersionError')
    // The skeleton must stop: an error means we are no longer loading.
    expect(result.current.isLoading).toBe(false)
    expect(result.current.items).toEqual([])

    // Retry clears the error and re-opens; a good snapshot now lands.
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-2' })
    runtimeAdapter.queueRuntimeSessionMessageListView({
      viewId: 'view-1',
      snapshot: mailListSnapshot(1, message),
    })
    result.current.retry()

    await waitFor(() => expect(result.current.error).toBeNull())
    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [message], nextCursor: null }],
        pageParams: [null],
      }),
    )
    expect(result.current.isLoading).toBe(false)
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

    const hookInput = mailListHookInput(queryKey)
    renderHook(
      () => {
        useDaemonEvents()
        useRuntimeMailListView(hookInput)
      },
      { wrapper },
    )

    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toEqual({
        pages: [{ items: [message], nextCursor: null }],
        pageParams: [null],
      }),
    )
    expect(runtimeAdapter.runtimeSessionCalls).toEqual([{ viewDelta: true }])
    expect(runtimeAdapter.runtimeFrameSubscriptionCalls).toEqual([
      { request: { sessionId: 'session-1', afterSeq: null } },
    ])
    expect(runtimeAdapter.runtimeSessionViewOpenCalls).toHaveLength(1)
    expect(runtimeAdapter.runtimeSessionViewOpenCalls[0].sourceId).toBe(
      undefined,
    )
  })
})
