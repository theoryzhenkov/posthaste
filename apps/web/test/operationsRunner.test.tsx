import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import type { ReactNode } from 'react'
import { act, renderHook, waitFor } from '@testing-library/react'
import {
  QueryClient,
  QueryClientProvider,
  type InfiniteData,
} from '@tanstack/react-query'

import { OperationsProvider } from '../src/components/OperationsProvider'
import { moveToMailboxOp } from '../src/operations'
import { useOperations } from '../src/operationsContext'
import { queryKeys } from '../src/queryKeys'
import type {
  MessageCommandResult,
  MessagePage,
  MessageSummary,
} from '../src/api/types'
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

const okResult: MessageCommandResult = { detail: null, events: [] }
let runtimeAdapter: FakeRuntimeAdapter

function messageSummary(
  overrides: Partial<MessageSummary> = {},
): MessageSummary {
  return {
    id: 'm1',
    sourceId: 'primary',
    sourceName: 'Primary',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Subject',
    fromName: 'Sender',
    fromEmail: 'sender@example.com',
    to: [],
    preview: 'preview',
    receivedAt: '2026-04-28T12:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    ...overrides,
  }
}

let queryClient: QueryClient

function seedView(
  selection:
    | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
    | { kind: 'smart-mailbox'; id: string },
  items: MessageSummary[],
) {
  const data: InfiniteData<MessagePage, string | null> = {
    pageParams: [null],
    pages: [{ items, nextCursor: null }],
  }
  queryClient.setQueryData(
    queryKeys.messages(selection, undefined, undefined),
    data,
  )
}

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>
      <OperationsProvider>{children}</OperationsProvider>
    </QueryClientProvider>
  )
}

beforeEach(() => {
  runtimeAdapter = createFakeRuntimeAdapter({
    defaultMessageCommandResult: okResult,
  })
  setRuntimeAdapterForTesting(runtimeAdapter)
  queryClient = new QueryClient({
    defaultOptions: {
      queries: { gcTime: Number.POSITIVE_INFINITY, retry: false },
    },
  })
})

afterEach(() => {
  resetRuntimeAdapterForTesting()
  queryClient.clear()
})

const inboxView = {
  kind: 'source-mailbox' as const,
  sourceId: 'primary',
  mailboxId: 'inbox',
}

describe('operation runner', () => {
  it('moves optimistically and undoes back to the captured mailbox', async () => {
    seedView(inboxView, [messageSummary({ mailboxIds: ['inbox'] })])
    const { result } = renderHook(() => useOperations(), { wrapper })

    await act(async () => {
      result.current.run(
        moveToMailboxOp(
          { sourceId: 'primary', messageId: 'm1' },
          'trash',
          'Message trashed',
        ),
      )
    })

    await waitFor(() =>
      expect(runtimeAdapter.messageCommandCalls.length).toBe(1),
    )
    expect(runtimeAdapter.messageCommandCalls[0]).toEqual({
      messageId: 'm1',
      sourceId: 'primary',
      command: { kind: 'replaceMailboxes', mailboxIds: ['trash'] },
    })
    await waitFor(() => expect(result.current.canUndo).toBe(true))

    await act(async () => {
      result.current.undo()
    })

    await waitFor(() =>
      expect(runtimeAdapter.messageCommandCalls.length).toBe(2),
    )
    // Undo restores the mailbox the message actually came from.
    expect(runtimeAdapter.messageCommandCalls[1]).toEqual({
      messageId: 'm1',
      sourceId: 'primary',
      command: { kind: 'replaceMailboxes', mailboxIds: ['inbox'] },
    })
    await waitFor(() => expect(result.current.canRedo).toBe(true))
  })

  it('two rapid undos revert distinct entries to their own before-images', async () => {
    const archiveView = {
      kind: 'source-mailbox' as const,
      sourceId: 'primary',
      mailboxId: 'archive',
    }
    seedView(inboxView, [messageSummary({ id: 'm1', mailboxIds: ['inbox'] })])
    seedView(archiveView, [
      messageSummary({
        id: 'm2',
        conversationId: 'c2',
        mailboxIds: ['archive'],
      }),
    ])
    const { result } = renderHook(() => useOperations(), { wrapper })

    await act(async () => {
      result.current.run(
        moveToMailboxOp(
          { sourceId: 'primary', messageId: 'm1' },
          'trash',
          'Trashed',
        ),
      )
    })
    await act(async () => {
      result.current.run(
        moveToMailboxOp(
          { sourceId: 'primary', messageId: 'm2' },
          'trash',
          'Trashed',
        ),
      )
    })
    await waitFor(() =>
      expect(runtimeAdapter.messageCommandCalls.length).toBe(2),
    )

    // Fire both undos in the same tick: the synchronous pop must select
    // distinct entries (m2 then m1), not undo m2 twice.
    await act(async () => {
      result.current.undo()
      result.current.undo()
    })

    await waitFor(() =>
      expect(runtimeAdapter.messageCommandCalls.length).toBe(4),
    )
    const undoCommands = runtimeAdapter.messageCommandCalls.slice(2)
    expect(undoCommands).toEqual([
      {
        messageId: 'm2',
        sourceId: 'primary',
        command: { kind: 'replaceMailboxes', mailboxIds: ['archive'] },
      },
      {
        messageId: 'm1',
        sourceId: 'primary',
        command: { kind: 'replaceMailboxes', mailboxIds: ['inbox'] },
      },
    ])
  })
})
