import { describe, expect, test } from 'bun:test'

import { QueryClient, QueryObserver } from '@tanstack/react-query'

import { prepareServerSearchQuery } from '@/domain/search'
import { queryKeys } from '@/data/queries/queryKeys'
import { MailClient } from '@/data/transport/client'
import {
  MESSAGE_PAGE_SIZE,
  buildMailListQuery,
  fetchMailListWindow,
} from './model'

describe('buildMailListQuery', () => {
  test('source-mailbox view scopes by account and mailbox', () => {
    const query = buildMailListQuery(
      {
        kind: 'source-mailbox',
        sourceId: 'acct-1',
        mailboxId: 'mb-1',
        name: 'Inbox',
      },
      prepareServerSearchQuery(undefined),
      { columnId: 'date', direction: 'desc' },
    )
    expect(query.accountId).toBe('acct-1')
    expect(query.mailboxId).toBe('mb-1')
    expect(query.smartMailboxId).toBeUndefined()
    expect(query.freeText).toBeNull()
    expect(query.sort).toEqual({ field: 'date', descending: true })
  })

  test('smart-mailbox view scopes by smart mailbox id', () => {
    const query = buildMailListQuery(
      { kind: 'smart-mailbox', id: 'smart-1', name: 'All Mail' },
      prepareServerSearchQuery(undefined),
      { columnId: 'from', direction: 'asc' },
    )
    expect(query.smartMailboxId).toBe('smart-1')
    expect(query.accountId).toBeUndefined()
    expect(query.sort).toEqual({ field: 'from', descending: false })
  })

  test('a prepared search rides as freeText', () => {
    const query = buildMailListQuery(
      { kind: 'smart-mailbox', id: 'smart-1', name: 'All Mail' },
      prepareServerSearchQuery('  hello   world '),
      { columnId: 'date', direction: 'desc' },
    )
    expect(query.freeText).toBe('hello world')
  })

  test('columns without server-side sorting fall back to date', () => {
    const query = buildMailListQuery(
      { kind: 'smart-mailbox', id: 'smart-1', name: 'All Mail' },
      prepareServerSearchQuery(undefined),
      { columnId: 'preview', direction: 'asc' },
    )
    expect(query.sort).toEqual({ field: 'date', descending: false })
  })
})

// ---------------------------------------------------------------------------
// The windowed mail list (refactor-ledger item 6): one cache entry per view
// window, chunk-fetched under the server's page cap, and — the invariant the
// collapse exists for — an invalidation refetches a deep-scrolled list O(1)
// in scroll depth, not once per accumulated page.

/** A fake backend: `total` rows, limit clamped to `cap`, index cursors. */
function makeMailListClient(total: number, cap: number) {
  const requests: Array<{ limit: number | undefined; cursor: string | undefined }> = []
  const client = new MailClient({
    baseUrl: '',
    token: 'tok',
    autoConnect: false,
    fetchImpl: async (_url, init) => {
      const body = JSON.parse(String(init?.body)) as {
        mailList: { limit?: number; cursor?: string }
      }
      requests.push({ limit: body.mailList.limit, cursor: body.mailList.cursor })
      const start = body.mailList.cursor ? Number(body.mailList.cursor) : 0
      const limit = Math.min(body.mailList.limit ?? 50, cap)
      const end = Math.min(start + limit, total)
      return Response.json({
        generation: 1,
        data: {
          rows: Array.from({ length: end - start }, (_, i) => ({ id: `m${start + i}` })),
          nextCursor: end < total ? String(end) : null,
        },
      })
    },
  })
  return { client, requests }
}

describe('fetchMailListWindow', () => {
  test('fills the window in server-capped chunks, asking for the remainder each time', async () => {
    const { client, requests } = makeMailListClient(260, 200)
    const window = await fetchMailListWindow(client, {}, 500)
    expect(window.rows.length).toBe(260)
    expect(window.rows[0]!.id).toBe('m0')
    expect(window.rows[259]!.id).toBe('m259')
    expect(window.nextCursor).toBeNull()
    // The client never restates the server's cap: it asks for what is
    // missing and follows the continuation the clamp produces.
    expect(requests.map((r) => r.limit)).toEqual([500, 300])
    expect(requests.map((r) => r.cursor)).toEqual([undefined, '200'])
  })

  test('stops at the window and reports the continuation past it', async () => {
    const { client, requests } = makeMailListClient(1000, 200)
    const window = await fetchMailListWindow(client, {}, 150)
    expect(window.rows.length).toBe(150)
    expect(window.nextCursor).toBe('150')
    expect(requests.length).toBe(1)
  })

  test('an empty page with a cursor terminates instead of looping', async () => {
    const client = new MailClient({
      baseUrl: '',
      token: 'tok',
      autoConnect: false,
      fetchImpl: async () =>
        Response.json({ generation: 1, data: { rows: [], nextCursor: 'stuck' } }),
    })
    const window = await fetchMailListWindow(client, {}, 300)
    expect(window.rows.length).toBe(0)
  })
})

describe('invalidation cost of a deep-scrolled list', () => {
  test('one refetch regardless of scroll depth (O(1) in pages)', async () => {
    const deepPages = 13 // the ledger's measured worst case
    const cap = 200
    const { client } = makeMailListClient(5000, cap)
    const queryClient = new QueryClient()
    const scope = { accountId: 'a1', mailboxId: 'mb1' }
    let queryFnRuns = 0
    const windowOptions = (pages: number) => {
      const windowSize = pages * MESSAGE_PAGE_SIZE
      return queryClient.defaultQueryOptions({
        queryKey: queryKeys.mailList({ ...scope, limit: windowSize }),
        queryFn: () => {
          queryFnRuns++
          return fetchMailListWindow(client, scope, windowSize)
        },
      })
    }

    // Scroll deep: every window the hook stepped through was fetched once...
    for (let pages = 1; pages < deepPages; pages++) {
      await queryClient.fetchQuery(windowOptions(pages))
    }
    // ...and only the deepest window is still mounted (the hook holds ONE
    // query; the smaller windows are inactive cache entries).
    const observer = new QueryObserver(queryClient, windowOptions(deepPages))
    const unsubscribe = observer.subscribe(() => {})
    while (!observer.getCurrentResult().isSuccess) {
      await new Promise((r) => setTimeout(r, 1))
    }
    expect(observer.getCurrentResult().data?.rows.length).toBe(deepPages * MESSAGE_PAGE_SIZE)

    // A mutation lands: the stream policy invalidates every query. The
    // deep-scrolled list refetches as ONE query, not once per page.
    queryFnRuns = 0
    await queryClient.invalidateQueries()
    expect(queryFnRuns).toBe(1)

    unsubscribe()
    queryClient.clear()
  })
})
