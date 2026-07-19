// Facade tests over a fake fetch and a fake EventSource: canonical query
// bodies, command envelopes, the multi-step verbs (role resolution, send
// holds, draft minting), and the stream passthrough. The units underneath
// have their own suites (http.test.ts, eventStream.test.ts).

import { describe, expect, test } from 'bun:test'
import {
  canonicalQueryKey,
  MailClient,
  type EventSourceLike,
  type FetchLike,
} from './client'

class FakeEventSource implements EventSourceLike {
  onopen: (() => void) | null = null
  onmessage: ((ev: { data: string }) => void) | null = null
  onerror: (() => void) | null = null

  constructor(readonly url: string) {}

  close(): void {}
}

interface Call {
  url: string
  body: unknown
}

type Handler = (url: string, body: unknown) => unknown | Promise<unknown>

function makeClient(handler: Handler) {
  const calls: Call[] = []
  const sources: FakeEventSource[] = []
  const fetchImpl: FetchLike = async (input, init) => {
    const url = String(input)
    const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined
    calls.push({ url, body })
    return Response.json(await handler(url, body))
  }
  const client = new MailClient({
    baseUrl: '',
    token: 'tok',
    fetchImpl,
    eventSourceFactory: (url) => {
      const es = new FakeEventSource(url)
      sources.push(es)
      return es
    },
    reconnectDelayMs: 1,
  })
  return { client, calls, sources }
}

const commandCalls = (calls: Call[]) => calls.filter((c) => c.url === '/api/command')

describe('reads', () => {
  test('query posts the canonical body and returns the envelope', async () => {
    const { client, calls } = makeClient(() => ({ generation: 3, data: { rows: [] } }))
    const envelope = await client.query({ mailList: { freeText: null, limit: 50 } })
    expect(calls[0]!.url).toBe('/api/query')
    expect(calls[0]!.body).toEqual({ mailList: { limit: 50 } }) // absent filters dropped
    expect(envelope.generation).toBe(3)
    client.close()
  })

  test('canonicalQueryKey sorts keys and drops absent filters', () => {
    expect(canonicalQueryKey({ mailList: { limit: 10, accountId: null } })).toBe(
      canonicalQueryKey({ mailList: { accountId: undefined, limit: 10 } }),
    )
  })
})

describe('verbs', () => {
  const commandHandler =
    (accept: { generation: number }): Handler =>
    (url) =>
      url === '/api/command' ? accept : { generation: 1, data: { rows: [] } }

  test('command posts the envelope with a generated 26-char idempotency id', async () => {
    const { client, calls } = makeClient(commandHandler({ generation: 9 }))
    const accepted = await client.command({
      setKeywords: {
        accountId: 'a1',
        messageId: 'm1',
        change: { add: ['$seen'], remove: [] },
      },
    })
    expect(accepted.generation).toBe(9)
    const cmd = commandCalls(calls)[0]!.body as { id: string; command: unknown }
    expect(cmd.id.length).toBe(26)
    expect(cmd.command).toEqual({
      setKeywords: {
        accountId: 'a1',
        messageId: 'm1',
        change: { add: ['$seen'], remove: [] },
      },
    })
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

  test('send returns the command id as the operation id — the undo cancel handle', async () => {
    // Regression anchor for docs/issues/integrated-send-undo-broken.md: the
    // undo countdown toast cancels via `cancelOperation` keyed by this id, and
    // the backend adopts the send command's envelope id as the outbox
    // operation id (verified end to end by the backend's
    // `frontend_send_envelope_holds_flushes_and_cancels_by_command_id`).
    const { client, calls } = makeClient(commandHandler({ generation: 5 }))
    const { operationId } = await client.send('a1', {
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
    })
    const envelope = commandCalls(calls)[0]!.body as { id: string }
    expect(operationId).toBe(envelope.id)
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
    client.close()
  })
})

describe('stream passthrough', () => {
  test('the facade opens the stream with the token URL and relays status and prompts', () => {
    const { client, sources } = makeClient(() => ({}))
    expect(sources[0]!.url).toBe('/events?token=tok')
    expect(client.getConnectionStatus()).toBe('reconnecting')

    let statusNotified = 0
    client.subscribeConnection(() => statusNotified++)
    sources[0]!.onopen?.()
    expect(statusNotified).toBe(1)
    expect(client.getConnectionStatus()).toBe('connected')

    const generations: number[] = []
    client.subscribeGeneration((g) => generations.push(g))
    const events: string[] = []
    client.onEvent('message.updated', (p) => events.push(p.accountId))
    sources[0]!.onmessage?.({
      data: JSON.stringify({
        generation: 2,
        event: { kind: 'message.updated', accountId: 'a1' },
      }),
    })
    expect(generations).toEqual([2])
    expect(events).toEqual(['a1'])
    client.close()
  })

  test('a command reply raises the generation baseline: the stream echo is not news', async () => {
    const { client, sources } = makeClient((url) =>
      url === '/api/command' ? { generation: 7 } : {},
    )
    sources[0]!.onopen?.()
    const generations: number[] = []
    client.subscribeGeneration((g) => generations.push(g))
    await client.command({ undo: { accountId: 'a1' } })
    sources[0]!.onmessage?.({ data: JSON.stringify({ generation: 7 }) })
    expect(generations).toEqual([])
    sources[0]!.onmessage?.({ data: JSON.stringify({ generation: 8 }) })
    expect(generations).toEqual([8])
    client.close()
  })
})

describe('blobs', () => {
  test('blob and logo URLs carry the token as a query parameter', () => {
    const { client } = makeClient(() => ({}))
    expect(client.blobUrl('b/1')).toBe('/api/blobs/b%2F1?token=tok')
    expect(client.accountLogoUrl('img1')).toBe('/api/account-assets/logos/img1?token=tok')
    client.close()
  })
})
