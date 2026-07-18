// The MailClient facade: the only way UI code touches the API.
//
// It owns the HTTP wrappers (auth header), the SSE subscription, generation
// tracking, and a mirror store of mounted queries. Components read state
// through live query hooks (see hooks.tsx) and change it through the verb
// methods; they never see HTTP, SSE, or generations directly.
//
// The mirror holds one entry per canonical query key. Mounting a query
// fetches it; a newer generation on the event stream refetches every mounted
// query on a short debounce; answers arriving out of order are discarded by
// their generation stamp. Nothing is ever served from memory that the
// backend has not just said — unmounting forgets the entry after a short
// grace period.

import type {
  AccountId,
  AccountsResult,
  ApiError,
  BlobId,
  Command,
  CommandAccepted,
  CommandEnvelope,
  DomainEventKind,
  DomainEventPayload,
  EventMessage,
  MailboxCountsQuery,
  MailboxCountsResult,
  MailboxId,
  MailListQuery,
  MailListResult,
  MessageDetailQuery,
  MessageDetailResult,
  MessageId,
  PendingOperationsQuery,
  PendingOperationsResult,
  Query,
  QueryEnvelope,
  SendMessageRequest,
  ThreadQuery,
  ThreadView,
} from '@/gen'
import type { ConnectionStatus, QueryStatus } from '@/domain/vocabulary'

/** What a live query hook returns: the latest answer with its generation. */
export interface LiveResult<T> {
  data: T | undefined
  generation: number
  status: QueryStatus
  error: Error | null
}

/** A failed HTTP call, carrying the typed error envelope fields. */
export class MailApiError extends Error {
  readonly kind: ApiError['kind']
  readonly retryable: boolean
  readonly httpStatus: number

  constructor(err: ApiError, httpStatus: number) {
    super(err.message)
    this.name = 'MailApiError'
    this.kind = err.kind
    this.retryable = err.retryable
    this.httpStatus = httpStatus
  }
}

/** The fetch shape the facade needs; injectable for tests. */
export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>

/** The subset of EventSource the facade uses; injectable for tests. */
export interface EventSourceLike {
  onopen: (() => void) | null
  onmessage: ((ev: { data: string }) => void) | null
  onerror: (() => void) | null
  close(): void
}

export interface MailClientOptions {
  /** Origin prefix for every request; '' when served behind the dev proxy. */
  baseUrl: string
  /** Session secret or capability token; sent as a bearer header, and as
   * `?token=` on the event stream (EventSource cannot set headers). */
  token: string
  /** Coalescing window for stream-triggered refetches. */
  debounceMs?: number
  /** How long an unmounted query entry survives before it is forgotten. */
  forgetGraceMs?: number
  /** Base delay between stream reconnect attempts (doubles per failure). */
  reconnectDelayMs?: number
  /** Open the event stream immediately; on by default. */
  autoConnect?: boolean
  fetchImpl?: FetchLike
  eventSourceFactory?: (url: string) => EventSourceLike
}

/** Connection options for the boot-time MailClient. The desktop shell
 * injects the embedded backend's port and per-launch session token as window
 * globals via an initialization script, before any bundle code runs; when
 * they are present the client talks to the loopback API directly. Absent
 * (browser tab behind the vite dev proxy), requests stay same-origin with no
 * token — the proxy injects the Authorization header. */
export function bootstrapClientOptions(): Pick<MailClientOptions, 'baseUrl' | 'token'> {
  if (typeof window !== 'undefined') {
    const globals = window as unknown as {
      __POSTHASTE_PORT__?: unknown
      __POSTHASTE_TOKEN__?: unknown
    }
    if (
      typeof globals.__POSTHASTE_PORT__ === 'number' &&
      typeof globals.__POSTHASTE_TOKEN__ === 'string'
    ) {
      return {
        baseUrl: `http://127.0.0.1:${globals.__POSTHASTE_PORT__}`,
        token: globals.__POSTHASTE_TOKEN__,
      }
    }
  }
  return { baseUrl: '', token: '' }
}

const ULID_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'

/** A ULID-shaped id: 48-bit millisecond timestamp + 80 random bits, Crockford
 * base32. Used as the command idempotency id and for client-minted draft ids. */
export function newId(): string {
  let ts = Date.now()
  const time: string[] = []
  for (let i = 0; i < 10; i++) {
    time.unshift(ULID_ALPHABET[ts % 32]!)
    ts = Math.floor(ts / 32)
  }
  const rand = new Uint8Array(16)
  if (typeof crypto !== 'undefined' && crypto.getRandomValues) {
    crypto.getRandomValues(rand)
  } else {
    for (let i = 0; i < rand.length; i++) rand[i] = Math.floor(Math.random() * 256)
  }
  let out = time.join('')
  for (let i = 0; i < 16; i++) out += ULID_ALPHABET[rand[i]! % 32]!
  return out
}

/** Sorts keys and drops absent filters (undefined and null encode the same
 * "no filter" on the wire) so that equivalent queries share one entry. */
function canonicalize(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(canonicalize)
  if (v !== null && typeof v === 'object') {
    const out: Record<string, unknown> = {}
    for (const k of Object.keys(v).sort()) {
      const val = (v as Record<string, unknown>)[k]
      if (val === undefined || val === null) continue
      out[k] = canonicalize(val)
    }
    return out
  }
  return v
}

/** The canonical identity of a query: two queries with the same key share one
 * mirror entry and one fetch. */
export function canonicalQueryKey(q: Query): string {
  return JSON.stringify(canonicalize(q))
}

const EMPTY_SNAPSHOT: LiveResult<never> = {
  data: undefined,
  generation: 0,
  status: 'loading',
  error: null,
}

interface Entry {
  key: string
  /** Canonical request body, posted verbatim on every (re)fetch. */
  body: string
  refcount: number
  listeners: Set<() => void>
  snapshot: LiveResult<unknown>
  /** Answers below this generation are discarded (out-of-order guard).
   * Reset to 0 when the backend run changes. */
  discardBelow: number
  /** Guards error reporting: only the newest fetch may write an error. */
  fetchSeq: number
  forgetTimer: ReturnType<typeof setTimeout> | null
}

type EventCallback = (payload: DomainEventPayload, generation: number) => void

export class MailClient {
  private readonly baseUrl: string
  private readonly token: string
  private readonly debounceMs: number
  private readonly forgetGraceMs: number
  private readonly reconnectDelayMs: number
  private readonly fetchImpl: FetchLike
  private readonly eventSourceFactory: (url: string) => EventSourceLike

  private readonly entries = new Map<string, Entry>()
  private readonly eventListeners = new Map<string, Set<EventCallback>>()
  private readonly connectionListeners = new Set<() => void>()
  private readonly generationListeners = new Set<(generation: number) => void>()

  private stream: EventSourceLike | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private refetchTimer: ReturnType<typeof setTimeout> | null = null
  private connection: ConnectionStatus = 'reconnecting'
  private consecutiveFailures = 0
  private everConnected = false
  private closed = false

  /** Newest generation heard from any source (stream or command replies). */
  private latestGeneration = 0
  /** Backend run id, when the stream carries one; a change voids every
   * generation baseline. */
  private runId: string | undefined

  constructor(opts: MailClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/$/, '')
    this.token = opts.token
    this.debounceMs = opts.debounceMs ?? 100
    this.forgetGraceMs = opts.forgetGraceMs ?? 500
    this.reconnectDelayMs = opts.reconnectDelayMs ?? 1000
    this.fetchImpl = opts.fetchImpl ?? ((input, init) => fetch(input, init))
    this.eventSourceFactory =
      opts.eventSourceFactory ?? ((url) => new EventSource(url) as EventSourceLike)
    if (opts.autoConnect !== false) this.connect()
  }

  // ---------------------------------------------------------------- stream

  connect(): void {
    if (this.stream || this.closed) return
    const url = `${this.baseUrl}/events?token=${encodeURIComponent(this.token)}`
    const es = this.eventSourceFactory(url)
    this.stream = es
    es.onopen = () => {
      this.consecutiveFailures = 0
      const wasDown = this.connection !== 'connected'
      this.setConnection('connected')
      // Recovery is the connect path: anything may have happened while the
      // stream was down, so refetch every mounted query.
      if (wasDown && this.everConnected) this.refetchMounted(true)
      this.everConnected = true
    }
    es.onmessage = (ev) => this.handleStreamMessage(ev.data)
    es.onerror = () => {
      es.close()
      if (this.stream !== es) return
      this.stream = null
      this.consecutiveFailures++
      this.setConnection(this.consecutiveFailures >= 2 ? 'stale' : 'reconnecting')
      this.markAllStale(false)
      const delay =
        this.reconnectDelayMs * Math.min(2 ** (this.consecutiveFailures - 1), 16)
      this.reconnectTimer = setTimeout(() => {
        this.reconnectTimer = null
        this.connect()
      }, delay)
    }
  }

  /** Tears the client down; it cannot be reused. */
  close(): void {
    this.closed = true
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    if (this.refetchTimer) clearTimeout(this.refetchTimer)
    this.stream?.close()
    this.stream = null
    for (const e of this.entries.values()) {
      if (e.forgetTimer) clearTimeout(e.forgetTimer)
    }
    this.entries.clear()
  }

  getConnectionStatus(): ConnectionStatus {
    return this.connection
  }

  subscribeConnection = (cb: () => void): (() => void) => {
    this.connectionListeners.add(cb)
    return () => this.connectionListeners.delete(cb)
  }

  private setConnection(next: ConnectionStatus): void {
    if (this.connection === next) return
    this.connection = next
    for (const cb of this.connectionListeners) cb()
  }

  private handleStreamMessage(raw: string): void {
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
      this.markAllStale(true)
      this.refetchMounted(true)
      this.dispatchGeneration(generation)
      return
    }
    if (generation > this.latestGeneration) {
      this.latestGeneration = generation
      this.dispatchGeneration(generation)
    }
    this.scheduleRefetch()
  }

  /** Subscribes to generation advances heard on the event stream (including
   * run rotations, which void every baseline). The external mirror — the
   * react-query cache — invalidates everything it holds on each advance;
   * there is no per-key policy. */
  subscribeGeneration(cb: (generation: number) => void): () => void {
    this.generationListeners.add(cb)
    return () => {
      this.generationListeners.delete(cb)
    }
  }

  private dispatchGeneration(generation: number): void {
    for (const cb of this.generationListeners) cb(generation)
  }

  private scheduleRefetch(): void {
    if (this.refetchTimer) return
    this.refetchTimer = setTimeout(() => {
      this.refetchTimer = null
      this.refetchMounted(false)
    }, this.debounceMs)
  }

  /** Refetches mounted entries that are behind the latest generation (or all
   * of them, on reconnect and run rotation). */
  private refetchMounted(all: boolean): void {
    for (const e of this.entries.values()) {
      const behind =
        e.snapshot.generation < this.latestGeneration || e.snapshot.status !== 'ready'
      if (all || behind) void this.fetchEntry(e)
    }
  }

  /** Keeps the last answers for display but marks them stale; with
   * `voidBaselines` the out-of-order guard resets too, so answers from a
   * fresh backend run (with smaller generations) are accepted. */
  private markAllStale(voidBaselines: boolean): void {
    for (const e of this.entries.values()) {
      if (voidBaselines) e.discardBelow = 0
      if (e.snapshot.status === 'ready') {
        this.setSnapshot(e, { ...e.snapshot, status: 'stale' })
      }
    }
  }

  // ---------------------------------------------------------------- prompts

  /** Subscribes to domain events for UI reactions (notifications, the undo
   * toast). Payloads are prompts: they trigger the callback and nothing else —
   * they are never folded into the mirror. `kind` is an exact topic like
   * `message.updated`, or `*` for every event. */
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

  private dispatchDomainEvent(payload: DomainEventPayload, generation: number): void {
    for (const cb of this.eventListeners.get(payload.kind) ?? []) cb(payload, generation)
    for (const cb of this.eventListeners.get('*') ?? []) cb(payload, generation)
  }

  // ----------------------------------------------------------------- mirror

  /** Mounts a query: creates (or joins) its mirror entry and fetches if new.
   * Returns the canonical key used by `subscribeQuery`/`getSnapshot`. */
  retain(query: Query): string {
    const key = canonicalQueryKey(query)
    let e = this.entries.get(key)
    if (!e) {
      e = {
        key,
        body: JSON.stringify(canonicalize(query)),
        refcount: 0,
        listeners: new Set(),
        snapshot: EMPTY_SNAPSHOT,
        discardBelow: 0,
        fetchSeq: 0,
        forgetTimer: null,
      }
      this.entries.set(key, e)
      void this.fetchEntry(e)
    }
    e.refcount++
    if (e.forgetTimer) {
      clearTimeout(e.forgetTimer)
      e.forgetTimer = null
    }
    return key
  }

  /** Unmounts a query; the entry is forgotten after a short grace period so a
   * quick remount (navigation) keeps the shared entry. */
  release(key: string): void {
    const e = this.entries.get(key)
    if (!e) return
    e.refcount--
    if (e.refcount > 0) return
    e.forgetTimer = setTimeout(() => {
      const cur = this.entries.get(key)
      if (cur && cur.refcount <= 0) this.entries.delete(key)
    }, this.forgetGraceMs)
  }

  subscribeQuery(key: string, cb: () => void): () => void {
    const e = this.entries.get(key)
    if (!e) return () => {}
    e.listeners.add(cb)
    return () => e.listeners.delete(cb)
  }

  getSnapshot<T>(key: string): LiveResult<T> {
    return (this.entries.get(key)?.snapshot ?? EMPTY_SNAPSHOT) as LiveResult<T>
  }

  private setSnapshot(e: Entry, next: LiveResult<unknown>): void {
    e.snapshot = next
    for (const cb of e.listeners) cb()
  }

  private async fetchEntry(e: Entry): Promise<void> {
    const seq = ++e.fetchSeq
    try {
      const res = await this.post('/api/query', e.body)
      if (this.entries.get(e.key) !== e) return // forgotten while in flight
      if (!res.ok) throw await this.errorFrom(res)
      const env = (await res.json()) as QueryEnvelope<unknown>
      if (env.generation < e.discardBelow) return // out-of-order answer
      e.discardBelow = env.generation
      if (env.generation > this.latestGeneration) this.latestGeneration = env.generation
      this.setSnapshot(e, {
        data: env.data,
        generation: env.generation,
        status: 'ready',
        error: null,
      })
    } catch (err) {
      if (this.entries.get(e.key) !== e || seq !== e.fetchSeq) return
      const error = err instanceof Error ? err : new Error(String(err))
      // A held answer stays displayable; without one the query is in error.
      this.setSnapshot(e, {
        ...e.snapshot,
        status: e.snapshot.data !== undefined ? 'stale' : 'error',
        error,
      })
    }
  }

  // ------------------------------------------------------------------ http

  private async post(path: string, body: string): Promise<Response> {
    return this.fetchImpl(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        authorization: `Bearer ${this.token}`,
      },
      body,
    })
  }

  private async errorFrom(res: Response): Promise<Error> {
    try {
      const body = (await res.json()) as ApiError
      if (body && typeof body.message === 'string') return new MailApiError(body, res.status)
    } catch {
      // fall through to the generic error
    }
    return new Error(`request failed with HTTP ${res.status}`)
  }

  // -------------------------------------------------------------- one-shot

  /** One-shot read: evaluates the query once, outside the mirror. */
  async query(q: { mailList: MailListQuery }): Promise<QueryEnvelope<MailListResult>>
  async query(q: { thread: ThreadQuery }): Promise<QueryEnvelope<ThreadView>>
  async query(q: { messageDetail: MessageDetailQuery }): Promise<QueryEnvelope<MessageDetailResult>>
  async query(q: { mailboxCounts: MailboxCountsQuery }): Promise<QueryEnvelope<MailboxCountsResult>>
  async query(q: { accounts: Record<string, never> }): Promise<QueryEnvelope<AccountsResult>>
  async query(q: { pendingOperations: PendingOperationsQuery }): Promise<QueryEnvelope<PendingOperationsResult>>
  async query(q: Query): Promise<QueryEnvelope<unknown>>
  async query(q: Query): Promise<QueryEnvelope<unknown>> {
    const res = await this.post('/api/query', JSON.stringify(canonicalize(q)))
    if (!res.ok) throw await this.errorFrom(res)
    return (await res.json()) as QueryEnvelope<unknown>
  }

  // ------------------------------------------------------------------ blobs

  /** Authenticated URL for a blob GET (attachment downloads, inline parts).
   * Blobs are immutable, so the browser may cache the response; the token
   * rides as a query parameter because plain anchors cannot set headers.
   * With an empty token (dev proxy injects the header) the parameter is
   * omitted. */
  blobUrl(blobId: BlobId): string {
    const token = this.token ? `?token=${encodeURIComponent(this.token)}` : ''
    return `${this.baseUrl}/api/blobs/${encodeURIComponent(blobId)}${token}`
  }

  /** Authenticated URL for an account logo GET, same token rules as blobs. */
  accountLogoUrl(imageId: string): string {
    const token = this.token ? `?token=${encodeURIComponent(this.token)}` : ''
    return `${this.baseUrl}/api/account-assets/logos/${encodeURIComponent(imageId)}${token}`
  }

  // ------------------------------------------------------------------ verbs

  /** Posts one typed command with a fresh idempotency id (or the caller's),
   * then immediately refetches every mounted query so answers catch up to
   * the returned generation — rows change because the backend's answer
   * changed, never because the client edited a list. */
  async command(command: Command, id: string = newId()): Promise<CommandAccepted> {
    const envelope: CommandEnvelope = { id, command }
    const res = await this.post('/api/command', JSON.stringify(envelope))
    if (!res.ok) throw await this.errorFrom(res)
    const accepted = (await res.json()) as CommandAccepted
    if (accepted.generation > this.latestGeneration) {
      this.latestGeneration = accepted.generation
    }
    this.refetchMounted(false)
    return accepted
  }

  markRead(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    return this.setKeywords(accountId, messageId, ['$seen'], [])
  }

  markUnread(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    return this.setKeywords(accountId, messageId, [], ['$seen'])
  }

  flag(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    return this.setKeywords(accountId, messageId, ['$flagged'], [])
  }

  unflag(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    return this.setKeywords(accountId, messageId, [], ['$flagged'])
  }

  private setKeywords(
    accountId: AccountId,
    messageId: MessageId,
    add: string[],
    remove: string[],
  ): Promise<CommandAccepted> {
    return this.command({ setKeywords: { accountId, messageId, change: { add, remove } } })
  }

  /** Replaces the message's mailboxes outright (a move, not an add). */
  move(
    accountId: AccountId,
    messageId: MessageId,
    mailboxIds: MailboxId[],
  ): Promise<CommandAccepted> {
    return this.command({ replaceMailboxes: { accountId, messageId, change: { mailboxIds } } })
  }

  /** Moves the message to the account's archive mailbox, resolved by role. */
  async archive(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    const mailboxId = await this.mailboxWithRole(accountId, 'archive')
    return this.move(accountId, messageId, [mailboxId])
  }

  /** Moves the message to the account's trash mailbox, resolved by role. */
  async trash(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    const mailboxId = await this.mailboxWithRole(accountId, 'trash')
    return this.move(accountId, messageId, [mailboxId])
  }

  /** Resolves the account's mailbox carrying the given role (inbox, junk, …). */
  async mailboxWithRole(accountId: AccountId, role: string): Promise<MailboxId> {
    const { data } = await this.query({ mailboxCounts: { accountId } })
    const row = data.rows.find((r) => r.accountId === accountId && r.mailbox.role === role)
    if (!row) {
      throw new Error(`account ${accountId} has no mailbox with role '${role}'`)
    }
    return row.mailbox.id
  }

  /** Submits the message. Hold semantics — the undo-send window and the
   * send-later time — travel inside the request itself; acceptance means
   * "recorded and visible", and the verdict arrives as pending-operations
   * state. The returned `operationId` is the command's idempotency id, which
   * the backend also uses as the send's outbox operation id — so the caller
   * can watch exactly this send in the pending-operations query. */
  async send(
    accountId: AccountId,
    request: SendMessageRequest,
    opts?: { undoWindowSeconds?: number; sendAt?: string },
  ): Promise<{ accepted: CommandAccepted; operationId: string }> {
    const merged: SendMessageRequest = { ...request }
    if (opts?.undoWindowSeconds !== undefined) merged.undoWindowSeconds = opts.undoWindowSeconds
    if (opts?.sendAt !== undefined) merged.sendAt = opts.sendAt
    const operationId = newId()
    const accepted = await this.command({ send: { accountId, request: merged } }, operationId)
    return { accepted, operationId }
  }

  /** Creates the draft on first save (minting its stable id) and updates it
   * on every save after; the caller keeps the returned draftId on the
   * request for subsequent saves and for `send`. */
  async saveDraft(
    accountId: AccountId,
    draft: SendMessageRequest,
  ): Promise<{ draftId: string; accepted: CommandAccepted }> {
    if (draft.draftId) {
      const accepted = await this.command({
        updateDraft: { accountId, draftId: draft.draftId, draft },
      })
      return { draftId: draft.draftId, accepted }
    }
    const draftId = newId()
    const accepted = await this.command({
      createDraft: { accountId, draft: { ...draft, draftId } },
    })
    return { draftId, accepted }
  }

  discardDraft(accountId: AccountId, draftId: string): Promise<CommandAccepted> {
    return this.command({ discardDraft: { accountId, draftId } })
  }
}
