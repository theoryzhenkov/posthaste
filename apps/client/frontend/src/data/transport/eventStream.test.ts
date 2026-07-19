// The stream state machine in isolation: connection status transitions,
// reconnect backoff, generation/run-id tracking (rotation, the late-heartbeat
// race), and domain-event prompt dispatch.

import { describe, expect, test } from 'bun:test'
import { EventStream, type EventSourceLike } from './eventStream'

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

function makeStream(url = '/events?token=tok') {
  const sources: FakeEventSource[] = []
  const stream = new EventStream({
    url,
    reconnectDelayMs: 1,
    eventSourceFactory: (u) => {
      const es = new FakeEventSource(u)
      sources.push(es)
      return es
    },
  })
  stream.connect()
  return { stream, sources }
}

const tick = (ms = 20) => new Promise((r) => setTimeout(r, ms))

describe('connection state', () => {
  test('starts reconnecting, reports connected on open', () => {
    const { stream, sources } = makeStream()
    expect(stream.getStatus()).toBe('reconnecting')
    sources[0]!.open()
    expect(stream.getStatus()).toBe('connected')
    stream.close()
  })

  test('one failure is reconnecting, two are stale; recovery reconnects', async () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()

    sources[0]!.fail()
    expect(stream.getStatus()).toBe('reconnecting')
    await tick() // reconnect attempt fires
    expect(sources.length).toBe(2)
    sources[1]!.fail()
    expect(stream.getStatus()).toBe('stale')

    await tick()
    sources[2]!.open()
    expect(stream.getStatus()).toBe('connected')
    stream.close()
  })

  test('status subscribers are notified on changes, torn down on unsubscribe', () => {
    const { stream, sources } = makeStream()
    let notified = 0
    const unsub = stream.subscribeStatus(() => notified++)
    sources[0]!.open()
    expect(notified).toBe(1)
    unsub()
    sources[0]!.fail()
    expect(notified).toBe(1)
    stream.close()
  })

  test('close tears the source down and stops reconnecting', async () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()
    stream.close()
    expect(sources[0]!.closed).toBe(true)
    await tick()
    expect(sources.length).toBe(1) // no reconnect after close
  })

  test('the machine connects to the URL it was given', () => {
    const { stream, sources } = makeStream('/events?token=secret')
    expect(sources[0]!.url).toBe('/events?token=secret')
    stream.close()
  })
})

describe('generation tracking', () => {
  test('advances notify once per message, in order', () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()
    const seen: number[] = []
    stream.subscribeGeneration((g) => seen.push(g))
    sources[0]!.emit({ generation: 4 })
    sources[0]!.emit({ generation: 5 })
    expect(seen).toEqual([4, 5])
    stream.close()
  })

  test('a stale-but-ordered generation is not news and does not notify', () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()
    const seen: number[] = []
    stream.subscribeGeneration((g) => seen.push(g))
    sources[0]!.emit({ generation: 50 })
    sources[0]!.emit({ generation: 49 }) // a heartbeat stamped before a racing write
    expect(seen).toEqual([50])
    // The guard stays armed: a genuinely newer message still notifies.
    sources[0]!.emit({ generation: 51 })
    expect(seen).toEqual([50, 51])
    stream.close()
  })

  test('a runId change is a backend restart: notifies even at a lower generation', () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()
    const seen: number[] = []
    stream.subscribeGeneration((g) => seen.push(g))
    sources[0]!.emit({ generation: 10, runId: 'run-a' })
    sources[0]!.emit({ generation: 2, runId: 'run-b' }) // fresh run, counter restarted
    expect(seen).toEqual([10, 2])
    // The new run's counter is the baseline now.
    sources[0]!.emit({ generation: 3, runId: 'run-b' })
    expect(seen).toEqual([10, 2, 3])
    stream.close()
  })

  test('observeGeneration raises the baseline silently (the command-reply race)', () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()
    const seen: number[] = []
    stream.subscribeGeneration((g) => seen.push(g))
    stream.observeGeneration(7) // a command reply already delivered generation 7
    sources[0]!.emit({ generation: 7 }) // the stream echo is not news
    expect(seen).toEqual([])
    sources[0]!.emit({ generation: 8 })
    expect(seen).toEqual([8])
    stream.close()
  })

  test('malformed and generation-less messages are ignored', () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()
    const seen: number[] = []
    stream.subscribeGeneration((g) => seen.push(g))
    sources[0]!.onmessage?.({ data: 'not json' })
    sources[0]!.emit({ hello: true })
    expect(seen).toEqual([])
    stream.close()
  })
})

describe('prompts', () => {
  test('onEvent dispatches exact topics and the * wildcard; unsubscribe holds', () => {
    const { stream, sources } = makeStream()
    sources[0]!.open()
    const seen: string[] = []
    const unsub = stream.onEvent('message.updated', (p) => seen.push(p.accountId))
    stream.onEvent('*', (p) => seen.push(`*:${p.kind}`))
    sources[0]!.emit({
      generation: 2,
      event: { kind: 'message.updated', accountId: 'a1', messageId: 'm9' },
    })
    expect(seen).toEqual(['a1', '*:message.updated'])

    unsub()
    sources[0]!.emit({
      generation: 2,
      event: { kind: 'message.updated', accountId: 'a2' },
    })
    expect(seen).toEqual(['a1', '*:message.updated', '*:message.updated'])
    stream.close()
  })
})
