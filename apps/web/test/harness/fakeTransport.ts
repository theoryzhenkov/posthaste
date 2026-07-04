/**
 * The harness FAKE TRANSPORT: a controllable frame/event stream standing in for
 * the SSE/WS link, behind the entity-store adapter's `base` seam.
 *
 * This FORMALIZES the ad-hoc `makeBase(...).push(...)` fake in
 * `entityStoreAdapter.test.ts` — the client's adapter binds ONE
 * `subscribeRuntimeFrames` handler set to its base; this fake captures that
 * handler set and exposes drive handles to emit frames and to reproduce the
 * link-lifecycle failure modes the resilience RFC (F1-F5) targets, all AT THE
 * TS SEAM the shim surfaces (the wasm near-end engine's classification is
 * Rust-side; here we assert what the client layer *sees*):
 *
 *  - `emitFrame` — deliver any parsed `RuntimeFrame` (viewReplace, a
 *    `message.updated` notification, a mutation verdict, a heartbeat).
 *  - `severWith(status)` / `severLink()` — a stream error. A 4xx (the M40 / F1
 *    "404 on reopen after a reaped or restarted link") surfaces as a PERMANENT
 *    error the way the shim's `onStatus('permanentError')` does; a status-less
 *    sever surfaces as TRANSIENT.
 *  - `gapFrame()` — a D49 reset: a seq gap the far-end could not replay, which
 *    the shim delivers as `onReset` (the near node must rebuild from snapshots).
 *  - `reconnect()` — re-open the severed stream so `emitFrame` flows again.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D119
 */
import type {
  RuntimeAdapter,
  RuntimeFrame,
  RuntimeFrameHandlers,
  RuntimeMailListRowState,
  RuntimeMailListViewState,
  RuntimeMutationReceipt,
  RuntimeRunMutationRequest,
  RuntimeViewSnapshot,
} from '../../src/runtime/types'

/** A minimal served row description (`entityStoreAdapter.test.ts` shape). */
export interface TransportRow {
  messageId: string
  receivedAt: string
  keywords?: string[]
  mailboxIds?: string[]
}

export function transportRow(
  messageId: string,
  receivedAt: string,
  keywords: string[] = [],
  mailboxIds: string[] = ['inbox'],
): TransportRow {
  return { messageId, receivedAt, keywords, mailboxIds }
}

function snapshot(
  rows: TransportRow[],
): RuntimeViewSnapshot<RuntimeMailListViewState> {
  return {
    viewId: 'v1',
    descriptor: { family: 'mailList', payload: {} },
    revision: 1,
    lifecycle: 'ready',
    readWatermark: null,
    coverage: { ranges: [] },
    data: {
      scope: null,
      projectionKind: 'message',
      sort: null,
      windowRequest: null,
      rows: rows.map((row) => ({
        rowKey: `s:${row.messageId}`,
        resourceRef: `message:s:${row.messageId}`,
        projection: {
          id: row.messageId,
          sourceId: 's',
          receivedAt: row.receivedAt,
          keywords: row.keywords ?? [],
          mailboxIds: row.mailboxIds ?? ['inbox'],
          isRead: (row.keywords ?? []).includes('$seen'),
          isFlagged: (row.keywords ?? []).includes('$flagged'),
          subject: row.messageId,
        } as unknown as RuntimeMailListRowState['projection'],
        orderKey: row.messageId,
      })),
      continuation: {
        beforeCursor: null,
        afterCursor: null,
        hasBefore: false,
        hasAfter: false,
      },
      readWatermark: null,
      coverage: { ranges: [] },
      knownTotalCount: rows.length,
      pendingMutations: [],
      anchor: null,
    },
    pendingMutations: [],
    error: null,
  }
}

/** Build a `message.updated` notification frame (the authoritative row+count
 *  path the store ingests). */
export function messageUpdatedFrame(
  messageId: string,
  projection: Record<string, unknown>,
  countDeltas: Array<{
    mailboxId: string
    unreadCount: number
    totalCount: number
  }> = [],
  accountId = 's',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'notification',
    linkSeq: 100,
    kind: 'message.updated',
    payload: {
      seq: 1,
      accountId,
      topic: 'message.updated',
      occurredAt: 'now',
      payload: { messageId, projection, countDeltas },
    },
  } as RuntimeFrame<RuntimeMailListViewState>
}

export interface FakeTransport {
  /** The base `RuntimeAdapter` handed to `createEntityStoreAdapter`. */
  base: RuntimeAdapter
  /** How many times the client bound a frame subscription (F5 re-bind probe). */
  subscribeCount(): number
  /** Mutation requests the client forwarded to the base. */
  forwardedMutations: RuntimeRunMutationRequest[]
  /** Deliver a parsed frame to the bound client handler. */
  emitFrame(frame: RuntimeFrame<RuntimeMailListViewState>): void
  /** Sever the stream with an HTTP status: 4xx ⇒ permanent (M40/F1), else transient. */
  severWith(status: number): void
  /** Sever the stream with no status ⇒ transient (network drop). */
  severLink(): void
  /** A D49 reset (seq gap the far-end could not replay) ⇒ `onReset`. */
  gapFrame(): void
  /** Re-open a severed stream so `emitFrame` flows again. */
  reconnect(): void
  /**
   * The M44 recovery edge: the near-end engine re-prepared a FRESH link (new
   * id). Adopts the new id server-side, re-opens the stream, and delivers
   * `onLinkReestablished` to the bound client handler — the shape the shim's
   * `onLinkReestablished` callback surfaces on a genuine re-prepare.
   */
  reestablishLink(newLinkId: string): void
  /** The `linkId` each view-open (mail-list + object) was issued against, in
   *  order — proves the client adopted the fresh link (RC3): a re-open after
   *  `reestablishLink` records the NEW id, not the dead one. */
  viewOpenLinkIds: string[]
  /** Script the object-view (`openRuntimeLinkView`, e.g. accountStatus) data a
   *  re-open re-serves — used to model the sync-Ready status flip that lands in
   *  the re-prepare gap. */
  setObjectViewData(data: unknown): void
}

export interface FakeTransportOptions {
  /** Rows the base serves from `openRuntimeLinkMessageListView`. */
  rows?: TransportRow[]
  /** Rows the base serves from `extendRuntimeLinkView` (the extended window). */
  extendedRows?: TransportRow[]
}

/**
 * Create the fake transport. Only the base surfaces the entity-store controller
 * touches are implemented; the rest are inert (cast, like the existing fake).
 */
export function createFakeTransport(
  options: FakeTransportOptions = {},
): FakeTransport {
  const rows = options.rows ?? [
    transportRow('m1', '2026-04-29T10:00:00Z'),
    transportRow('m2', '2026-04-28T10:00:00Z'),
  ]
  const extendedRows = options.extendedRows ?? [
    ...rows,
    transportRow('m3', '2026-04-27T10:00:00Z'),
    transportRow('m4', '2026-04-26T10:00:00Z'),
  ]

  let handlers: RuntimeFrameHandlers | null = null
  let severed = false
  let subscribeCount = 0
  let currentLinkId = 'sess'
  let objectViewData: unknown = { status: 'idle' }
  const viewOpenLinkIds: string[] = []
  const forwardedMutations: RuntimeRunMutationRequest[] = []
  const receipt: RuntimeMutationReceipt = {
    runtimeMutationId: 'r-1',
    clientMutationId: 'c-1',
    name: 'message.setKeywords',
    state: 'accepted',
    error: null,
  }

  const base = {
    openRuntimeLink: async () => ({ linkId: currentLinkId }),
    closeRuntimeLink: async () => ({ ok: true }),
    openRuntimeLinkMessageListView: async (request: { linkId?: string }) => {
      viewOpenLinkIds.push(request.linkId ?? '')
      return { viewId: 'v1', snapshot: snapshot(rows) }
    },
    openRuntimeLinkView: async (request: { linkId?: string }) => {
      viewOpenLinkIds.push(request.linkId ?? '')
      return {
        viewId: 'ov1',
        snapshot: {
          ...snapshot([]),
          data: objectViewData,
        },
      }
    },
    extendRuntimeLinkView: async () => ({
      viewId: 'v1',
      snapshot: snapshot(extendedRows),
    }),
    closeRuntimeLinkView: async () => ({ ok: true }),
    subscribeRuntimeFrames: (
      _request: unknown,
      bound: RuntimeFrameHandlers,
    ) => {
      handlers = bound
      severed = false
      subscribeCount += 1
      return () => {
        if (handlers === bound) handlers = null
      }
    },
    runRuntimeMutation: async (request: RuntimeRunMutationRequest) => {
      forwardedMutations.push(request)
      return { ...receipt, clientMutationId: request.clientMutationId }
    },
  } as unknown as RuntimeAdapter

  return {
    base,
    subscribeCount: () => subscribeCount,
    forwardedMutations,
    emitFrame(frame) {
      if (severed) {
        throw new Error(
          'fake transport is severed; call reconnect() before emitFrame()',
        )
      }
      handlers?.onFrame(frame)
    },
    severWith(status) {
      severed = true
      const error = new Error(`runtime stream rejected with ${status}`)
      if (status >= 400 && status < 500) {
        handlers?.onPermanentError?.(error)
      } else {
        handlers?.onTransientError?.(error)
      }
    },
    severLink() {
      severed = true
      handlers?.onTransientError?.(new Error('runtime stream network drop'))
    },
    gapFrame() {
      handlers?.onReset?.()
    },
    reconnect() {
      severed = false
    },
    reestablishLink(newLinkId) {
      currentLinkId = newLinkId
      severed = false
      handlers?.onLinkReestablished?.(newLinkId)
    },
    viewOpenLinkIds,
    setObjectViewData(data) {
      objectViewData = data
    },
  }
}
