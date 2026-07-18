// Facade tests over a fake fetch and a fake EventSource: the mirror store,
// the debounced refetch loop, out-of-order discard, run rotation, reconnect
// staleness, command verbs, and prompt dispatch.

import { describe, expect, test } from 'bun:test'
import {
  canonicalQueryKey,
  MailClient,
  newId,
  type EventSourceLike,
  type FetchLike,
  type MailClientOptions,
} from './client'

class FakeEventSource implements EventSourceLike {
  onopen: (() => void) | null = null
  onmessage: ((ev: { data: string }) => void) | null = null
  onerror: (() => void) | null = null
  closed = false

  constructor(readonly url: string) {}

  close(): void {
    this.closed = true
  }

  open(): void {
    this.onopen?.()
  }

  emit(msg: object): void {
    this.onmessage?.({ data: JSON.stringify(msg) })
  }

  fail(): void {
    this.onerror?.()
  }
}

interface Call {
  url: string
  headers: Record<string, string>
  body: unknown
}

type Handler = (url: string, body: unknown) => unknown | Promise<unknown>

function fakeFetch(handler: Handler) {
  const calls: Call[] = []
  const fn: FetchLike = async (input, init) => {
    const url = String(input)
    const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined
    calls.push({ url, headers: (init?.headers ?? {}) as Record<string, string>, body })
    const json = await handler(url, body)
    return new Response(JSON.stringify(json), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  }
  return { fn, calls }
}

function makeClient(handler: Handler, extra?: Partial<MailClientOptions>) {
  const { fn, calls } = fakeFetch(handler)
  const sources: FakeEventSource[] = []
  const client = new MailClient({
    baseUrl: '',
    token: 'tok',
    fetchImpl: fn,
    eventSourceFactory: (url) => {
      const es = new FakeEventSource(url)
      sources.push(es)
      return es
    },
    debounceMs: 5,
    forgetGraceMs: 10,
    reconnectDelayMs: 1,
    ...extra,
  })
  return { client, calls, sources }
}

const tick = (ms = 20) => new Promise((r) => setTimeout(r, ms))

const queryCalls = (calls: Call[]) => calls.filter((c) => c.url === '/api/query')
const commandCalls = (calls: Call[]) => calls.filter((c) => c.url === '/api/command')

const mailListAnswer = (generation: number, subject: string) => ({
  generation,
  data: { rows: [{ subject }], nextCursor: null },
})

describe('mirror store', () => {
  test('mounting fetches once with auth; equivalent queries share one entry', async () => {
    const { client, calls } = makeClient(() => mailListAnswer(3, 'a'))
    const k1 = client.retain({ mailList: { limit: 50, freeText: null } })
    const k2 = client.retain({ mailList: { freeText: undefined, limit: 50 } })
    expect(k1).toBe(k2)
    await tick()

    expect(queryCalls(calls).length).toBe(1)
    expect(queryCalls(calls)[0]!.headers['authorization']).toBe('Bearer tok')
    expect(queryCalls(calls)[0]!.body).toEqual({ mailList: { limit: 50 } })

    const snap = client.getSnapshot(k1)
    expect(snap.status).toBe('ready')
    expect(snap.generation).toBe(3)
    expect(snap.data).toEqual({ rows: [{ subject: 'a' }], nextCursor: null })
    client.close()
  })

  test('distinct queries get distinct entries', async () => {
    const { client, calls } = makeClient(() => mailListAnswer(1, 'x'))
    const k1 = client.retain({ mailList: {} })
    const k2 = client.retain({ mailList: { isRead: false } })
    expect(k1).not.toBe(k2)
    await tick()
    expect(queryCalls(calls).length).toBe(2)
    client.close()
  })

  test('release forgets the entry after the grace period', async () => {
    const { client, calls, sources } = makeClient(() => mailListAnswer(2, 'a'))
    const key = client.retain({ mailList: {} })
    await tick()
    client.release(key)
    await tick()

    expect(client.getSnapshot(key).status).toBe('loading') // back to empty
    const before = queryCalls(calls).length
    sources[0]!.open()
    sources[0]!.emit({ generation: 99 })
    await tick()
    expect(queryCalls(calls).length).toBe(before) // nothing mounted, no refetch
    client.close()
  })

  test('release then quick remount inside the grace keeps the entry', async () => {
    const { client, calls } = makeClient(() => mailListAnswer(2, 'a'))
    const key = client.retain({ mailList: {} })
    await tick()
    client.release(key)
    client.retain({ mailList: {} })
    await tick()
    expect(client.getSnapshot(key).status).toBe('ready')
    expect(queryCalls(calls).length).toBe(1)
    client.close()
  })

  test('listeners fire on snapshot change', async () => {
    const { client } = makeClient(() => mailListAnswer(1, 'a'))
    const key = client.retain({ mailList: {} })
    let notified = 0
    client.subscribeQuery(key, () => notified++)
    await tick()
    expect(notified).toBe(1)
    client.close()
  })
})

describe('refetch loop', () => {
  test('stream generation bumps coalesce into one debounced refetch', async () => {
    let generation = 3
    const { client, calls, sources } = makeClient(() => mailListAnswer(generation, 'a'))
    const key = client.retain({ mailList: {} })
    await tick()
    sources[0]!.open()

    generation = 6
    sources[0]!.emit({ generation: 4 })
    sources[0]!.emit({ generation: 5 })
    sources[0]!.emit({ generation: 6 })
    await tick()

    expect(queryCalls(calls).length).toBe(2) // mount + one coalesced refetch
    expect(client.getSnapshot(key).generation).toBe(6)
    client.close()
  })

  test('out-of-order answers are discarded by generation stamp', async () => {
    const pending: Array<(v: unknown) => void> = []
    const { client, calls, sources } = makeClient(() => new Promise((r) => pending.push(r)))
    const key = client.retain({ mailList: {} })
    await tick()
    pending.shift()!(mailListAnswer(5, 'fresh'))
    await tick()
    expect(client.getSnapshot(key).generation).toBe(5)

    sources[0]!.open()
    sources[0]!.emit({ generation: 6 })
    await tick()
    expect(queryCalls(calls).length).toBe(2)
    pending.shift()!(mailListAnswer(4, 'stale')) // older than what is held
    await tick()

    const snap = client.getSnapshot<{ rows: Array<{ subject: string }> }>(key)
    expect(snap.generation).toBe(5)
    expect(snap.data!.rows[0]!.subject).toBe('fresh')
    client.close()
  })

  test('a stale-but-ordered stream generation is not a rotation: baselines and snapshots hold', async () => {
    let generation = 50
    const { client, sources } = makeClient(() => mailListAnswer(generation, 'a'))
    const key = client.retain({ mailList: {} })
    await tick()
    expect(client.getSnapshot(key).generation).toBe(50)

    sources[0]!.open()
    sources[0]!.emit({ generation: 49 }) // a heartbeat stamped before a racing write
    await tick()

    const snap = client.getSnapshot(key)
    expect(snap.status).toBe('ready') // no spurious stale flap
    expect(snap.generation).toBe(50)

    // The out-of-order guard stayed armed: a later, genuinely newer message
    // still refetches and the answer is accepted.
    generation = 51
    sources[0]!.emit({ generation: 51 })
    await tick()
    expect(client.getSnapshot(key).generation).toBe(51)
    client.close()
  })

  test('a runId change is a backend restart: all stale, refetched, lower generations accepted', async () => {
    let generation = 10
    const { client, calls, sources } = makeClient(() => mailListAnswer(generation, 'a'))
    const key = client.retain({ mailList: {} })
    await tick()
    sources[0]!.open()
    sources[0]!.emit({ generation: 10, runId: 'run-a' })
    await tick()
    const before = queryCalls(calls).length

    generation = 3 // the fresh run restarts the counter below what is held
    sources[0]!.emit({ generation: 2, runId: 'run-b' })
    await tick()
    expect(queryCalls(calls).length).toBe(before + 1)
    const snap = client.getSnapshot(key)
    expect(snap.status).toBe('ready')
    expect(snap.generation).toBe(3)
    client.close()
  })
})

describe('connection state', () => {
  test('disconnect keeps last answers marked stale; reconnect refetches everything mounted', async () => {
    let generation = 7
    const { client, calls, sources } = makeClient(() => mailListAnswer(generation, 'held'))
    const key = client.retain({ mailList: {} })
    await tick()
    sources[0]!.open()
    expect(client.getConnectionStatus()).toBe('connected')

    sources[0]!.fail()
    expect(client.getConnectionStatus()).toBe('reconnecting')
    const held = client.getSnapshot<{ rows: unknown[] }>(key)
    expect(held.status).toBe('stale')
    expect(held.data).toBeDefined() // last answer kept for display

    await tick() // reconnect attempt fires
    expect(sources.length).toBe(2)
    sources[1]!.fail()
    expect(client.getConnectionStatus()).toBe('stale')

    await tick()
    const before = queryCalls(calls).length
    generation = 9
    sources[2]!.open()
    await tick()
    expect(client.getConnectionStatus()).toBe('connected')
    expect(queryCalls(calls).length).toBe(before + 1)
    expect(client.getSnapshot(key).status).toBe('ready')
    expect(client.getSnapshot(key).generation).toBe(9)
    client.close()
  })

  test('connection subscribers are notified on changes', async () => {
    const { client, sources } = makeClient(() => mailListAnswer(1, 'a'))
    let notified = 0
    const unsub = client.subscribeConnection(() => notified++)
    sources[0]!.open()
    expect(notified).toBe(1)
    unsub()
    sources[0]!.fail()
    expect(notified).toBe(1)
    client.close()
  })
})

describe('verbs', () => {
  const commandHandler =
    (accept: { generation: number }): Handler =>
    (url) =>
      url === '/api/command' ? accept : mailListAnswer(1, 'a')

  test('markRead posts setKeywords with a generated id, then refetches mounted queries', async () => {
    const { client, calls } = makeClient(commandHandler({ generation: 9 }))
    const key = client.retain({ mailList: {} })
    await tick()
    const mountFetches = queryCalls(calls).length

    const accepted = await client.markRead('a1', 'm1')
    expect(accepted.generation).toBe(9)

    const cmd = commandCalls(calls)[0]!.body as {
      id: string
      command: unknown
    }
    expect(cmd.id.length).toBe(26)
    expect(cmd.command).toEqual({
      setKeywords: {
        accountId: 'a1',
        messageId: 'm1',
        change: { add: ['$seen'], remove: [] },
      },
    })
    await tick(2) // immediate refetch, no debounce wait
    expect(queryCalls(calls).length).toBe(mountFetches + 1)
    expect(client.getSnapshot(key)).toBeDefined()
    client.close()
  })

  test('flag/unflag/markUnread map onto keyword changes', async () => {
    const { client, calls } = makeClient(commandHandler({ generation: 2 }))
    await client.flag('a1', 'm1')
    await client.unflag('a1', 'm1')
    await client.markUnread('a1', 'm1')
    const changes = commandCalls(calls).map(
      (c) =>
        (c.body as { command: { setKeywords: { change: unknown } } }).command.setKeywords
          .change,
    )
    expect(changes).toEqual([
      { add: ['$flagged'], remove: [] },
      { add: [], remove: ['$flagged'] },
      { add: [], remove: ['$seen'] },
    ])
    client.close()
  })

  test('archive resolves the role mailbox through a one-shot query, then moves', async () => {
    const { client, calls } = makeClient((url, body) => {
      if (url === '/api/command') return { generation: 4 }
      expect(body).toEqual({ mailboxCounts: { accountId: 'a1' } })
      return {
        generation: 3,
        data: {
          rows: [
            { accountId: 'a1', mailbox: { id: 'mb-inbox', name: 'Inbox', role: 'inbox' } },
            { accountId: 'a1', mailbox: { id: 'mb-arch', name: 'Archive', role: 'archive' } },
          ],
        },
      }
    })
    await client.archive('a1', 'm1')
    expect((commandCalls(calls)[0]!.body as { command: unknown }).command).toEqual({
      replaceMailboxes: {
        accountId: 'a1',
        messageId: 'm1',
        change: { mailboxIds: ['mb-arch'] },
      },
    })
    client.close()
  })

  test('trash without a trash mailbox rejects without posting a command', async () => {
    const { client, calls } = makeClient(() => ({ generation: 1, data: { rows: [] } }))
    await expect(client.trash('a1', 'm1')).rejects.toThrow("no mailbox with role 'trash'")
    expect(commandCalls(calls).length).toBe(0)
    client.close()
  })

  test('send merges hold options into the request', async () => {
    const { client, calls } = makeClient(commandHandler({ generation: 5 }))
    const draft = {
      from: null,
      to: [],
      cc: [],
      bcc: [],
      subject: 's',
      body: 'b',
      inReplyTo: null,
      references: null,
      attachments: [],
      draftId: 'd1',
    }
    await client.send('a1', draft, { undoWindowSeconds: 30, sendAt: '2026-07-18T09:00:00Z' })
    const sent = (
      commandCalls(calls)[0]!.body as {
        command: { send: { request: Record<string, unknown> } }
      }
    ).command.send.request
    expect(sent.undoWindowSeconds).toBe(30)
    expect(sent.sendAt).toBe('2026-07-18T09:00:00Z')
    expect(sent.draftId).toBe('d1')
    client.close()
  })

  test('saveDraft creates with a minted id on first save, updates after', async () => {
    const { client, calls } = makeClient(commandHandler({ generation: 6 }))
    const draft = {
      from: null,
      to: [],
      cc: [],
      bcc: [],
      subject: 's',
      body: 'b',
      inReplyTo: null,
      references: null,
      attachments: [],
      draftId: null,
    }
    const first = await client.saveDraft('a1', draft)
    expect(first.draftId.length).toBe(26)
    const created = (
      commandCalls(calls)[0]!.body as {
        command: { createDraft: { draft: { draftId: string } } }
      }
    ).command.createDraft
    expect(created.draft.draftId).toBe(first.draftId)

    await client.saveDraft('a1', { ...draft, draftId: first.draftId })
    const updated = (
      commandCalls(calls)[1]!.body as { command: { updateDraft: { draftId: string } } }
    ).command.updateDraft
    expect(updated.draftId).toBe(first.draftId)

    await client.discardDraft('a1', first.draftId)
    expect(
      (commandCalls(calls)[2]!.body as { command: unknown }).command,
    ).toEqual({ discardDraft: { accountId: 'a1', draftId: first.draftId } })
    client.close()
  })
})

describe('prompts', () => {
  test('onEvent fires callbacks and never touches the mirror', async () => {
    const { client, sources } = makeClient(() => mailListAnswer(2, 'a'))
    const key = client.retain({ mailList: {} })
    await tick()
    const snapBefore = client.getSnapshot(key)

    const seen: string[] = []
    const unsub = client.onEvent('message.updated', (p) => seen.push(p.accountId))
    client.onEvent('*', (p) => seen.push(`*:${p.kind}`))
    sources[0]!.open()
    sources[0]!.emit({
      generation: 2,
      event: { kind: 'message.updated', accountId: 'a1', messageId: 'm9' },
    })
    expect(seen).toEqual(['a1', '*:message.updated'])
    expect(client.getSnapshot(key)).toBe(snapBefore) // payload folded nowhere

    unsub()
    sources[0]!.emit({
      generation: 2,
      event: { kind: 'message.updated', accountId: 'a2' },
    })
    expect(seen).toEqual(['a1', '*:message.updated', '*:message.updated'])
    client.close()
  })
})

describe('helpers', () => {
  test('canonicalQueryKey sorts keys and drops absent filters', () => {
    expect(canonicalQueryKey({ mailList: { limit: 10, accountId: null } })).toBe(
      canonicalQueryKey({ mailList: { accountId: undefined, limit: 10 } }),
    )
  })

  test('newId is 26 chars of Crockford base32 and unique', () => {
    const a = newId()
    const b = newId()
    expect(a).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/)
    expect(a).not.toBe(b)
  })

  test('event stream url carries the token as a query parameter', () => {
    const { client, sources } = makeClient(() => mailListAnswer(1, 'a'))
    expect(sources[0]!.url).toBe('/events?token=tok')
    client.close()
  })
})
