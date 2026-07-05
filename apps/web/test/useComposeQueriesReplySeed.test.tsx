/**
 * Instant-reply-from-cache: the reply composer seeds its `>`-quote from the
 * caches the detail pane populated (headers in `mailKeys.message`, body in
 * `mailKeys.messageBody`) so the editor is usable with the quote WITHOUT waiting
 * on the authoritative `replyContext` Email/get — which still runs and supplies
 * real threading (References/Cc) that SEND gates on.
 */
import { afterEach, describe, expect, it, spyOn } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import type { MessageDetail, ReplyContext } from '../src/api/types'
import type { ComposeIntent } from '../src/composeIntent'
import { useComposeQueries } from '../src/components/compose-overlay/useComposeQueries'
import { mailKeys } from '../src/mailState'
import { runtimeViews } from '../src/runtime/views'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const SOURCE_ID = 'acct-1'
const MESSAGE_ID = 'msg-1'

function cachedDetail(overrides: Partial<MessageDetail> = {}): MessageDetail {
  return {
    id: MESSAGE_ID,
    sourceId: SOURCE_ID,
    sourceName: 'Work',
    sourceThreadId: 'thread-1',
    conversationId: 'conv-1',
    subject: 'Quarterly plan',
    fromName: 'Ada Lovelace',
    fromEmail: 'ada@example.com',
    to: [{ name: 'Me', email: 'me@example.com' }],
    preview: 'Here is the plan',
    receivedAt: '2026-06-01T10:00:00Z',
    hasAttachment: false,
    isRead: true,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    rfcMessageId: '<orig@example.com>',
    // The detail read surface serves the body-free payload.
    bodyHtml: null,
    bodyText: null,
    rawMessage: null,
    attachments: [],
    ...overrides,
  }
}

/**
 * The authoritative `replyContext` fetch never resolves during a test unless we
 * want it to — that keeps `isPlaceholderData` true so we can assert the composer
 * is usable on the cache seed alone.
 */
function stubComposeQueries(replyContext: () => Promise<ReplyContext>) {
  const pending =
    <T,>() =>
    () =>
      new Promise<T>(() => {})
  const spies = [
    spyOn(runtimeViews.compose, 'identity').mockImplementation(pending()),
    spyOn(runtimeViews.accounts, 'list').mockImplementation(pending()),
    spyOn(runtimeViews.compose, 'senderAddresses').mockImplementation(
      pending(),
    ),
    spyOn(runtimeViews.compose, 'conversationPage').mockImplementation(
      pending(),
    ),
    spyOn(runtimeViews.compose, 'replyContext').mockImplementation(
      replyContext,
    ),
  ]
  return () => spies.forEach((spy) => spy.mockRestore())
}

function renderComposeQueries(
  intent: ComposeIntent,
  seed: (client: QueryClient) => void,
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  seed(client)
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
  return renderHook(() => useComposeQueries({ intent }), { wrapper })
}

const replyIntent: ComposeIntent = {
  kind: 'reply',
  sourceId: SOURCE_ID,
  messageId: MESSAGE_ID,
}

describe('useComposeQueries — instant reply from cache', () => {
  afterEach(() => {})

  it('seeds the quote from the messageBody cache immediately (isPlaceholderData)', () => {
    const restore = stubComposeQueries(
      () => new Promise<ReplyContext>(() => {}),
    )
    try {
      const { result } = renderComposeQueries(replyIntent, (client) => {
        client.setQueryData(
          mailKeys.message(SOURCE_ID, MESSAGE_ID),
          cachedDetail(),
        )
        client.setQueryData(mailKeys.messageBody(SOURCE_ID, MESSAGE_ID), {
          bodyHtml: '<p>Here is the plan</p>',
          bodyText: 'Here is the plan\nsecond line',
        })
      })

      // The editor becomes usable on the placeholder — no wait on the fetch.
      const query = result.current.replyContextQuery
      expect(query.isPlaceholderData).toBe(true)
      expect(query.data).toBeDefined()
      // Quote is the `>`-prefixed cached text body; recipient + subject are seeded.
      expect(query.data?.quotedBody).toBe('> Here is the plan\n> second line')
      expect(query.data?.to).toEqual([
        { name: 'Ada Lovelace', email: 'ada@example.com' },
      ])
      expect(query.data?.replySubject).toBe('Re: Quarterly plan')
      expect(query.data?.inReplyTo).toBe('<orig@example.com>')
      // Placeholder carries NO real threading — the authoritative fetch supplies it.
      expect(query.data?.references).toBeNull()
    } finally {
      restore()
    }
  })

  it('falls back to the body carried on the detail payload when present', () => {
    const restore = stubComposeQueries(
      () => new Promise<ReplyContext>(() => {}),
    )
    try {
      const { result } = renderComposeQueries(replyIntent, (client) => {
        client.setQueryData(
          mailKeys.message(SOURCE_ID, MESSAGE_ID),
          cachedDetail({ bodyText: 'Inline body' }),
        )
        // No messageBody cache entry at all.
      })
      const query = result.current.replyContextQuery
      expect(query.isPlaceholderData).toBe(true)
      expect(query.data?.quotedBody).toBe('> Inline body')
    } finally {
      restore()
    }
  })

  it('does NOT seed (no placeholder) when the body is not cached — waits on the fetch', () => {
    const restore = stubComposeQueries(
      () => new Promise<ReplyContext>(() => {}),
    )
    try {
      const { result } = renderComposeQueries(replyIntent, (client) => {
        // Detail (headers) cached, but the body was never loaded/warmed.
        client.setQueryData(
          mailKeys.message(SOURCE_ID, MESSAGE_ID),
          cachedDetail(),
        )
      })
      const query = result.current.replyContextQuery
      // No placeholder → the composer streams the served quote in instead.
      expect(query.data).toBeUndefined()
      expect(query.isPlaceholderData).toBe(false)
    } finally {
      restore()
    }
  })

  it('replaces the placeholder with the authoritative context (real threading for send)', async () => {
    const authoritative: ReplyContext = {
      to: [{ name: 'Ada Lovelace', email: 'ada@example.com' }],
      cc: [{ name: null, email: 'team@example.com' }],
      originalTo: [{ name: 'Me', email: 'me@example.com' }],
      replySubject: 'Re: Quarterly plan',
      forwardSubject: 'Fwd: Quarterly plan',
      quotedBody: '> Here is the plan\n> second line',
      forwardedBody: null,
      inReplyTo: '<orig@example.com>',
      references: '<root@example.com> <orig@example.com>',
    }
    const restore = stubComposeQueries(() => Promise.resolve(authoritative))
    try {
      const { result } = renderComposeQueries(replyIntent, (client) => {
        client.setQueryData(
          mailKeys.message(SOURCE_ID, MESSAGE_ID),
          cachedDetail(),
        )
        client.setQueryData(mailKeys.messageBody(SOURCE_ID, MESSAGE_ID), {
          bodyHtml: null,
          bodyText: 'Here is the plan\nsecond line',
        })
      })
      // Starts on the placeholder (no References)...
      expect(result.current.replyContextQuery.isPlaceholderData).toBe(true)
      expect(result.current.replyContextQuery.data?.references).toBeNull()
      // ...then the authoritative fetch settles and supplies real threading.
      await waitFor(() => {
        expect(result.current.replyContextQuery.isPlaceholderData).toBe(false)
      })
      expect(result.current.replyContextQuery.data?.references).toBe(
        '<root@example.com> <orig@example.com>',
      )
      expect(result.current.replyContextQuery.data?.cc).toEqual([
        { name: null, email: 'team@example.com' },
      ])
    } finally {
      restore()
    }
  })
})
