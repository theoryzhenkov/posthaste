// The stream state machine: one EventSource over the /events endpoint,
// reconnect with exponential backoff, connection status, generation and
// run-id tracking, and domain-event prompt dispatch. This unit only reports
// what the stream says — the refetch policy lives in stream.ts, which
// invalidates the react-query mirror on every generation notification.

import type { ConnectionStatus } from '@/domain/vocabulary'
import type { DomainEventKind, DomainEventPayload, EventMessage } from '@/gen'
import { createStore } from '@/lib/store'

/** The subset of EventSource the machine uses; injectable for tests. */
export interface EventSourceLike {
  onopen: (() => void) | null
  onmessage: ((ev: { data: string }) => void) | null
  onerror: (() => void) | null
  close(): void
}

export type EventCallback = (payload: DomainEventPayload, generation: number) => void

export interface EventStreamOptions {
  /** The fully-formed stream URL (token included; see HttpTransport.streamUrl). */
  url: string
  /** Base delay between reconnect attempts (doubles per failure, capped). */
  reconnectDelayMs?: number
  eventSourceFactory?: (url: string) => EventSourceLike
}

export class EventStream {
  private readonly url: string
  private readonly reconnectDelayMs: number
  private readonly eventSourceFactory: (url: string) => EventSourceLike

  private readonly statusStore = createStore<ConnectionStatus>('reconnecting')
  private readonly generationStore = createStore(0)
  private readonly eventListeners = new Map<string, Set<EventCallback>>()

  private source: EventSourceLike | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private consecutiveFailures = 0
  private closed = false

  /** Newest generation heard from any source (stream or command replies). */
  private latestGeneration = 0
  /** Backend run id, when the stream carries one; a change voids every
   * generation baseline. */
  private runId: string | undefined

  constructor(opts: EventStreamOptions) {
    this.url = opts.url
    this.reconnectDelayMs = opts.reconnectDelayMs ?? 1000
    this.eventSourceFactory =
      opts.eventSourceFactory ?? ((url) => new EventSource(url) as EventSourceLike)
  }

  connect(): void {
    if (this.source || this.closed) return
    const es = this.eventSourceFactory(this.url)
    this.source = es
    es.onopen = () => {
      this.consecutiveFailures = 0
      this.setStatus('connected')
    }
    es.onmessage = (ev) => this.handleMessage(ev.data)
    es.onerror = () => {
      es.close()
      if (this.source !== es) return
      this.source = null
      this.consecutiveFailures++
      this.setStatus(this.consecutiveFailures >= 2 ? 'stale' : 'reconnecting')
      const delay =
        this.reconnectDelayMs * Math.min(2 ** (this.consecutiveFailures - 1), 16)
      this.reconnectTimer = setTimeout(() => {
        this.reconnectTimer = null
        this.connect()
      }, delay)
    }
  }

  /** Tears the machine down; it cannot be reused. */
  close(): void {
    this.closed = true
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.source?.close()
    this.source = null
  }

  getStatus(): ConnectionStatus {
    return this.statusStore.get()
  }

  subscribeStatus(cb: () => void): () => void {
    return this.statusStore.subscribe(cb)
  }

  /** Subscribes to generation advances heard on the event stream (including
   * run rotations, which void every baseline). The mirror — the react-query
   * cache — invalidates everything it holds on each notification; there is
   * no per-key policy. */
  subscribeGeneration(cb: (generation: number) => void): () => void {
    return this.generationStore.subscribe(() => cb(this.generationStore.get()))
  }

  /** Raises the newest-generation baseline from a source that races the
   * stream (command replies), without notifying subscribers — the caller
   * already has the answer in hand. */
  observeGeneration(generation: number): void {
    if (generation > this.latestGeneration) this.latestGeneration = generation
  }

  /** Subscribes to domain events for UI reactions (notifications, the undo
   * toast). Payloads are prompts: they trigger the callback and nothing else.
   * `kind` is an exact topic like `message.updated`, or `*` for every event. */
  onEvent(kind: DomainEventKind | '*', cb: EventCallback): () => void {
    let set = this.eventListeners.get(kind)
    if (!set) {
      set = new Set()
      this.eventListeners.set(kind, set)
    }
    set.add(cb)
    return () => {
      set.delete(cb)
      if (set.size === 0) this.eventListeners.delete(kind)
    }
  }

  private setStatus(next: ConnectionStatus): void {
    if (this.statusStore.get() === next) return
    this.statusStore.set(next)
  }

  private handleMessage(raw: string): void {
    let msg: EventMessage
    try {
      msg = JSON.parse(raw) as EventMessage
    } catch {
      return
    }
    if (typeof msg.generation !== 'number') return
    this.observeStreamGeneration(msg.generation, msg.runId)
    if (msg.event) this.dispatchDomainEvent(msg.event, msg.generation)
  }

  /** Generation tracking. A backend restart is detected by the run id alone:
   * the stream's handshake always carries one, so a fresh run voids every
   * baseline. A message stamped lower than the newest generation heard is
   * not a restart — command replies and query answers race the stream, so an
   * already-stamped heartbeat can arrive late; the stream is level-triggered
   * and heals on its next message, and the out-of-order guard must stay
   * armed exactly through that race. */
  private observeStreamGeneration(generation: number, runId?: string): void {
    const rotated = runId !== undefined && this.runId !== undefined && runId !== this.runId
    if (runId !== undefined) this.runId = runId
    if (rotated) {
      this.latestGeneration = generation
      this.dispatchGeneration(generation)
      return
    }
    if (generation > this.latestGeneration) {
      this.latestGeneration = generation
      this.dispatchGeneration(generation)
    }
  }

  private dispatchGeneration(generation: number): void {
    // The store notifies on every set (no equality gate), so a run rotation
    // that lands on an already-seen generation number still reaches the
    // mirror.
    this.generationStore.set(generation)
  }

  private dispatchDomainEvent(payload: DomainEventPayload, generation: number): void {
    for (const cb of this.eventListeners.get(payload.kind) ?? []) cb(payload, generation)
    for (const cb of this.eventListeners.get('*') ?? []) cb(payload, generation)
  }
}
