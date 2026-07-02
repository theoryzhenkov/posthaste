/**
 * The client near-end behind the wasm boundary (D41/M9b2): these suites drive
 * the REAL `LinkNearEnd` engine (the shipped wasm) through the binding's fake
 * transport IO — no TS policy seams left to fake. Replaces the old
 * `sessionClientReconnect` suite (the TS reconnect timer it pinned is deleted;
 * the engine owns reconnects) and the transport halves of the adapter suites.
 *
 * Behavior pinned here: reconnect-with-cursor, replay-on-connect (the outbox
 * reconciler), malformed-frame reporting, permanent-vs-transient stream
 * classification, forward retry + typed receipts, and the sent-but-unsettled
 * settlement query (D44b).
 */
import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { waitFor } from '@testing-library/react'

import { setupDomEnvironment } from './dom-env'
import {
  connectNearEnd,
  forwardNearEndMutation,
  resetNearEndForTesting,
  setNearEndOutboxHooks,
  setNearEndTransportIoForTesting,
  subscribeNearEndFrames,
  type NearEndSentUnsettled,
  type NearEndTransportIo,
} from '../src/runtime/nearEnd'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
  RuntimeMutationReceipt,
  RuntimeRunMutationRequest,
} from '../src/runtime/types'

setupDomEnvironment()

// ---- fake transport IO -------------------------------------------------------

interface FakeStream {
  url: string
  emit: (kind: string, data: string, status?: number) => void
  aborted: boolean
}

type PostResponder = (
  url: string,
  body: string,
) => { status: number; body: string }

class FakeIo implements NearEndTransportIo {
  posts: { url: string; body: string }[] = []
  gets: string[] = []
  streams: FakeStream[] = []
  /** Scripted mutation-POST responses (fallback echoes a confirmed receipt). */
  mutationResponses: PostResponder[] = []
  /** Scripted settlement-GET responses (fallback: no record). */
  settlementResponses: { status: number; body: string }[] = []

  async postJson(url: string, _headersJson: string, body: string) {
    this.posts.push({ url, body })
    if (url.includes('/mutations')) {
      const scripted = this.mutationResponses.shift()
      if (scripted) {
        return scripted(url, body)
      }
      const request = JSON.parse(body) as { clientMutationId: string }
      return {
        status: 200,
        body: JSON.stringify(confirmedReceipt(request.clientMutationId)),
      }
    }
    // Session open.
    return { status: 200, body: JSON.stringify({ sessionId: 'session-A' }) }
  }

  async getJson(url: string) {
    this.gets.push(url)
    return (
      this.settlementResponses.shift() ?? {
        status: 200,
        body: JSON.stringify({ receipt: null }),
      }
    )
  }

  openStream(
    url: string,
    onEvent: (kind: string, data: string, status: number) => void,
  ) {
    const stream: FakeStream = {
      url,
      emit: (kind, data, status = -1) => {
        if (!stream.aborted) {
          onEvent(kind, data, status)
        }
      },
      aborted: false,
    }
    this.streams.push(stream)
    return () => {
      stream.aborted = true
    }
  }
}

function confirmedReceipt(clientMutationId: string): RuntimeMutationReceipt {
  return {
    runtimeMutationId: 'rm-1',
    clientMutationId,
    name: 'message.setReadState',
    state: 'confirmed',
    error: null,
  } as RuntimeMutationReceipt
}

function sampleRequest(clientMutationId: string): RuntimeRunMutationRequest {
  return {
    name: 'message.setReadState',
    args: { sourceId: 'acct-1', messageId: 'm1', read: true },
    clientMutationId,
  }
}

let io: FakeIo

beforeEach(() => {
  window.sessionStorage.clear()
  io = new FakeIo()
  setNearEndTransportIoForTesting(io)
})

afterEach(async () => {
  await resetNearEndForTesting()
  window.sessionStorage.clear()
})

const waitForStreams = (count: number) =>
  waitFor(() => expect(io.streams.length).toBe(count), { timeout: 4000 })

describe('the near-end engine over fake IO', () => {
  it('opens the session and delivers engine-parsed frames to subscribers', async () => {
    const frames: RuntimeFrame<RuntimeMailListViewState>[] = []
    subscribeNearEndFrames({ onFrame: (frame) => frames.push(frame) })

    const { sessionId } = await connectNearEnd()
    expect(sessionId).toBe('session-A')
    // The engine's prepare POST carries the session options.
    expect(io.posts[0]?.url).toContain('/runtime/sessions?viewDelta=true')

    await waitForStreams(1)
    io.streams[0]!.emit('open', '')
    io.streams[0]!.emit(
      'message',
      JSON.stringify({ type: 'heartbeat', sessionSeq: 5 }),
    )

    await waitFor(() => expect(frames.length).toBe(1))
    expect(frames[0]).toEqual({ type: 'heartbeat', sessionSeq: 5 })
    // The binding mirrors the engine-owned cursor for reload resume.
    expect(window.sessionStorage.getItem('mail:last-runtime-frame-seq')).toBe(
      '5',
    )
  })

  it('reconnects after a close and resumes from the engine-owned cursor', async () => {
    subscribeNearEndFrames({ onFrame: () => {} })
    await connectNearEnd()
    await waitForStreams(1)

    io.streams[0]!.emit('open', '')
    io.streams[0]!.emit(
      'message',
      JSON.stringify({ type: 'heartbeat', sessionSeq: 5 }),
    )
    io.streams[0]!.emit('closed', '')

    // The engine reconnects on its own (jittered backoff, no TS timer) and
    // resubscribes WITH the resume cursor — the fact the old TS client lost.
    await waitForStreams(2)
    expect(io.streams[0]!.url).not.toContain('afterSeq')
    expect(io.streams[1]!.url).toContain('afterSeq=5')
  })

  it('reports a malformed frame instead of casting it', async () => {
    const malformed: { raw: string; error: unknown }[] = []
    const frames: unknown[] = []
    subscribeNearEndFrames({
      onFrame: (frame) => frames.push(frame),
      onMalformedFrame: (input) => malformed.push(input),
    })
    await connectNearEnd()
    await waitForStreams(1)

    io.streams[0]!.emit('open', '')
    io.streams[0]!.emit('message', 'this is not json')

    await waitFor(() => expect(malformed.length).toBe(1))
    expect(malformed[0]!.raw).toBe('this is not json')
    expect(frames).toHaveLength(0)
  })

  it('treats a 4xx stream error as permanent: surfaced, no reconnect', async () => {
    const permanent: unknown[] = []
    subscribeNearEndFrames({
      onFrame: () => {},
      onPermanentError: (error) => permanent.push(error),
    })
    await connectNearEnd()
    await waitForStreams(1)

    io.streams[0]!.emit('open', '')
    io.streams[0]!.emit('error', 'forbidden', 403)

    await waitFor(() => expect(permanent.length).toBe(1))
    // Give a would-be reconnect ample time: none may happen.
    await new Promise((resolve) => setTimeout(resolve, 700))
    expect(io.streams.length).toBe(1)
  })

  it('treats a status-less stream error as transient: reconnects with backoff', async () => {
    const transient: unknown[] = []
    subscribeNearEndFrames({
      onFrame: () => {},
      onTransientError: (error) => transient.push(error),
    })
    await connectNearEnd()
    await waitForStreams(1)

    io.streams[0]!.emit('open', '')
    io.streams[0]!.emit('error', 'network dropped')

    await waitForStreams(2)
    expect(transient.length).toBeGreaterThanOrEqual(1)
  })

  it('forward retries a transient failure then resolves the typed receipt', async () => {
    await connectNearEnd()
    io.mutationResponses.push(() => ({ status: 503, body: 'unavailable' }))

    const receipt = await forwardNearEndMutation(sampleRequest('c-retry'))

    expect(receipt.clientMutationId).toBe('c-retry')
    expect(receipt.state).toBe('confirmed')
    const mutationPosts = io.posts.filter((p) => p.url.includes('/mutations'))
    expect(mutationPosts.length).toBe(2)
    // The engine stamped ITS session onto the wire request.
    expect(mutationPosts[1]!.body).toContain('"sessionId":"session-A"')
  })

  it('forward surfaces a 4xx as permanent without a retry', async () => {
    await connectNearEnd()
    io.mutationResponses.push(() => ({
      status: 422,
      body: JSON.stringify({
        code: 'invalid_mutation',
        message: 'nope',
        retryable: false,
        correlationId: null,
        details: null,
      }),
    }))

    await expect(
      forwardNearEndMutation(sampleRequest('c-bad')),
    ).rejects.toThrow('nope')
    expect(io.posts.filter((p) => p.url.includes('/mutations')).length).toBe(1)
  })

  it('replays never-dispatched outbox records on every connect (D44a)', async () => {
    const reconciled: {
      receipt: RuntimeMutationReceipt
      sessionId: string | null
    }[] = []
    let pending: RuntimeRunMutationRequest[] = [sampleRequest('c-replay')]
    setNearEndOutboxHooks({
      neverDispatched: async () => {
        const drained = pending
        pending = []
        return drained
      },
      onReconciled: async (receipt, sessionId) => {
        reconciled.push({ receipt, sessionId })
      },
      sentUnsettled: async () => [],
      onSettlement: async () => {},
    })
    subscribeNearEndFrames({ onFrame: () => {} })
    await connectNearEnd()
    await waitForStreams(1)

    // The reconciler is level-triggered: it runs on the stream OPEN.
    io.streams[0]!.emit('open', '')

    await waitFor(() => expect(reconciled.length).toBe(1))
    expect(reconciled[0]!.receipt.clientMutationId).toBe('c-replay')
    expect(reconciled[0]!.sessionId).toBe('session-A')
    expect(io.posts.filter((p) => p.url.includes('/mutations')).length).toBe(1)
  })

  it('settles a sent-but-unsettled record from a terminal settlement query (D44b)', async () => {
    const settled: RuntimeMutationReceipt[] = []
    const record: NearEndSentUnsettled = {
      sessionId: 'session-old',
      clientMutationId: 'c-sent',
      request: sampleRequest('c-sent'),
    }
    let unsettled: NearEndSentUnsettled[] = [record]
    setNearEndOutboxHooks({
      neverDispatched: async () => [],
      onReconciled: async () => {},
      sentUnsettled: async () => {
        const drained = unsettled
        unsettled = []
        return drained
      },
      onSettlement: async (receipt) => {
        settled.push(receipt)
      },
    })
    io.settlementResponses.push({
      status: 200,
      body: JSON.stringify({ receipt: confirmedReceipt('c-sent') }),
    })
    subscribeNearEndFrames({ onFrame: () => {} })
    await connectNearEnd()
    await waitForStreams(1)
    io.streams[0]!.emit('open', '')

    await waitFor(() => expect(settled.length).toBe(1))
    // The query hit the OLD session's ledger, not the live session's.
    expect(io.gets[0]).toContain(
      '/runtime/sessions/session-old/mutations/c-sent',
    )
    // Terminal verdict: settled locally, never re-forwarded.
    expect(io.posts.filter((p) => p.url.includes('/mutations')).length).toBe(0)
  })

  it('re-forwards a sent-but-unsettled record the runtime does not know (D44b)', async () => {
    const reconciled: RuntimeMutationReceipt[] = []
    let unsettled: NearEndSentUnsettled[] = [
      {
        sessionId: 'session-old',
        clientMutationId: 'c-lost',
        request: sampleRequest('c-lost'),
      },
    ]
    setNearEndOutboxHooks({
      neverDispatched: async () => [],
      onReconciled: async (receipt) => {
        reconciled.push(receipt)
      },
      sentUnsettled: async () => {
        const drained = unsettled
        unsettled = []
        return drained
      },
      onSettlement: async () => {},
    })
    // The runtime has no record (session-continuity loss).
    io.settlementResponses.push({
      status: 200,
      body: JSON.stringify({ receipt: null }),
    })
    subscribeNearEndFrames({ onFrame: () => {} })
    await connectNearEnd()
    await waitForStreams(1)
    io.streams[0]!.emit('open', '')

    await waitFor(() => expect(reconciled.length).toBe(1))
    expect(reconciled[0]!.clientMutationId).toBe('c-lost')
    expect(io.posts.filter((p) => p.url.includes('/mutations')).length).toBe(1)
  })

  it('seeds the reconnect cursor from sessionStorage across an engine restart', async () => {
    window.sessionStorage.setItem('mail:last-runtime-frame-seq', '42')
    subscribeNearEndFrames({ onFrame: () => {} })
    await connectNearEnd()
    await waitForStreams(1)
    // A reload resumes where the last engine left off — callers never thread
    // afterSeq; the binding seeds the engine's initial cursor from storage.
    expect(io.streams[0]!.url).toContain('afterSeq=42')
  })
})
