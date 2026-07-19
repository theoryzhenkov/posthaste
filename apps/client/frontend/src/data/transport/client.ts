// The MailClient facade: the only way UI code touches the API.
//
// It composes the two transport units — HttpTransport (fetch/auth, error
// mapping, token-in-URL rules) and EventStream (SSE lifecycle, reconnect,
// run-id/generation tracking, prompt dispatch) — behind one object, and owns
// the read/write wire shapes: canonical query bodies, command envelopes with
// idempotency ids, and the multi-step verbs (role resolution, send holds,
// draft minting). Components read state through the react-query hooks
// (queries/queries.ts) and change it through the verbs (transport/
// commands.ts); they never see HTTP, SSE, or generations directly.

import type {
  AccountId,
  AccountsResult,
  BlobId,
  Command,
  CommandAccepted,
  CommandEnvelope,
  DomainEventKind,
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
import type { ConnectionStatus } from '@/domain/vocabulary'
import { newId } from '@/lib/ambient/random'
import { HttpTransport, type FetchLike } from '@/data/transport/http'
import {
  EventStream,
  type EventCallback,
  type EventSourceLike,
} from '@/data/transport/eventStream'

export { MailApiError, type FetchLike } from '@/data/transport/http'
export { type EventSourceLike } from '@/data/transport/eventStream'

export interface MailClientOptions {
  /** Origin prefix for every request; '' when served behind the dev proxy. */
  baseUrl: string
  /** Session secret or capability token; sent as a bearer header, and as
   * `?token=` on the event stream (EventSource cannot set headers). */
  token: string
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

/** Sorts keys and drops absent filters (undefined and null encode the same
 * "no filter" on the wire) so that equivalent queries share one cache entry. */
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
 * cache entry and one fetch. */
export function canonicalQueryKey(q: Query): string {
  return JSON.stringify(canonicalize(q))
}

export class MailClient {
  private readonly http: HttpTransport
  private readonly stream: EventStream

  constructor(opts: MailClientOptions) {
    this.http = new HttpTransport(opts)
    this.stream = new EventStream({
      url: this.http.streamUrl(),
      reconnectDelayMs: opts.reconnectDelayMs,
      eventSourceFactory: opts.eventSourceFactory,
    })
    if (opts.autoConnect !== false) this.stream.connect()
  }

  /** Tears the client down; it cannot be reused. */
  close(): void {
    this.stream.close()
  }

  // ---------------------------------------------------------------- stream

  getConnectionStatus(): ConnectionStatus {
    return this.stream.getStatus()
  }

  /** Arrow property: passed unbound to useSyncExternalStore. */
  subscribeConnection = (cb: () => void): (() => void) =>
    this.stream.subscribeStatus(cb)

  /** Subscribes to generation advances heard on the event stream (including
   * run rotations, which void every baseline). The mirror — the react-query
   * cache — invalidates everything it holds on each advance (stream.ts);
   * there is no per-key policy. */
  subscribeGeneration(cb: (generation: number) => void): () => void {
    return this.stream.subscribeGeneration(cb)
  }

  /** Subscribes to domain events for UI reactions (notifications, the undo
   * toast). Payloads are prompts: they trigger the callback and nothing else —
   * they are never folded into cached answers. `kind` is an exact topic like
   * `message.updated`, or `*` for every event. */
  onEvent(kind: DomainEventKind | '*', cb: EventCallback): () => void {
    return this.stream.onEvent(kind, cb)
  }

  // ----------------------------------------------------------------- reads

  /** One-shot read: evaluates the query once; caching is react-query's job. */
  async query(q: { mailList: MailListQuery }): Promise<QueryEnvelope<MailListResult>>
  async query(q: { thread: ThreadQuery }): Promise<QueryEnvelope<ThreadView>>
  async query(q: { messageDetail: MessageDetailQuery }): Promise<QueryEnvelope<MessageDetailResult>>
  async query(q: { mailboxCounts: MailboxCountsQuery }): Promise<QueryEnvelope<MailboxCountsResult>>
  async query(q: { accounts: Record<string, never> }): Promise<QueryEnvelope<AccountsResult>>
  async query(q: { pendingOperations: PendingOperationsQuery }): Promise<QueryEnvelope<PendingOperationsResult>>
  async query(q: Query): Promise<QueryEnvelope<unknown>>
  async query(q: Query): Promise<QueryEnvelope<unknown>> {
    const json = await this.http.postJson('/api/query', JSON.stringify(canonicalize(q)))
    return json as QueryEnvelope<unknown>
  }

  // ------------------------------------------------------------------ blobs

  /** Authenticated URL for a blob GET (attachment downloads, inline parts).
   * Blobs are immutable, so the browser may cache the response. */
  blobUrl(blobId: BlobId): string {
    return this.http.getUrl(`/api/blobs/${encodeURIComponent(blobId)}`)
  }

  /** Authenticated URL for an account logo GET, same token rules as blobs. */
  accountLogoUrl(imageId: string): string {
    return this.http.getUrl(`/api/account-assets/logos/${encodeURIComponent(imageId)}`)
  }

  // ------------------------------------------------------------------ verbs

  /** Posts one typed command with a fresh idempotency id (or the caller's).
   * The returned generation raises the stream's baseline so a late heartbeat
   * stamped at or below it is not mistaken for news; the caller (runCommand)
   * invalidates the react-query mirror so answers catch up — rows change
   * because the backend's answer changed, never because the client edited a
   * list. */
  async command(command: Command, id: string = newId()): Promise<CommandAccepted> {
    const envelope: CommandEnvelope = { id, command }
    const json = await this.http.postJson('/api/command', JSON.stringify(envelope))
    const accepted = json as CommandAccepted
    this.stream.observeGeneration(accepted.generation)
    return accepted
  }

  /** Moves the message to the account's archive mailbox, resolved by role. */
  async archive(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    const mailboxId = await this.mailboxWithRole(accountId, 'archive')
    return this.replaceMailboxes(accountId, messageId, [mailboxId])
  }

  /** Moves the message to the account's trash mailbox, resolved by role. */
  async trash(accountId: AccountId, messageId: MessageId): Promise<CommandAccepted> {
    const mailboxId = await this.mailboxWithRole(accountId, 'trash')
    return this.replaceMailboxes(accountId, messageId, [mailboxId])
  }

  /** Replaces the message's mailboxes outright (a move, not an add). */
  private replaceMailboxes(
    accountId: AccountId,
    messageId: MessageId,
    mailboxIds: MailboxId[],
  ): Promise<CommandAccepted> {
    return this.command({ replaceMailboxes: { accountId, messageId, change: { mailboxIds } } })
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
}
