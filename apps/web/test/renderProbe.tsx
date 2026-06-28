/* eslint-disable react-refresh/only-export-components -- test helper, not HMR'd */
/**
 * Render-layer flicker probe (Layer D): drives the REAL `entityStoreAdapter`
 * + real WASM `EntityStore` + real `useRuntimeMailListView` hook + a real React
 * render from a fixture frame stream, and records the rendered + cache
 * trajectory so [`RenderLog`] can assert no transient stale state (a flash of
 * past state). No hand-ported adapter glue — the production adapter is the code
 * under test.
 *
 * Fixtures supply external inputs at the adapter's injection seams
 * (`EntityStoreAdapterDeps`): `base` = a `FakeRuntimeAdapter` whose
 * `emitRuntimeFrame` is the replay point (the adapter cannot tell a live frame
 * from a replayed one), `makeHandle` = the real WASM factory, `outbox` = a real
 * `MemoryOutboxStore`, `queryClient` = a test client. Observables are the
 * rendered row-set per commit (the observer's `query.data`, which reflects
 * `placeholderData` too) + the raw cache.
 *
 * @spec docs/eph/DESIGN-L2-render-flicker-tracker
 */
import { useEffect, useMemo } from 'react'
import type { ReactNode } from 'react'
import { act, render, waitFor } from '@testing-library/react'
import {
  QueryClient,
  QueryClientProvider,
  useInfiniteQuery,
} from '@tanstack/react-query'
import { readFileSync } from 'node:fs'
import assert from 'node:assert'
import { join } from 'node:path'

import type { MessagePage, MessageSummary } from '../src/api/types'
import { queryKeys } from '../src/queryKeys'
import { useRuntimeMailListView } from '../src/components/message-list/useRuntimeMailListView'
import { createEntityStoreAdapter } from '../src/runtime/replica/entityStoreAdapter'
import type {
  EntityStoreHandle,
  EntityStoreHandleFactory,
} from '../src/runtime/replica/handle'
import { MemoryOutboxStore } from '../src/runtime/replica/outboxStore'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { resetRuntimeSessionClientForTesting } from '../src/runtime/sessionClient'
import { runtimeSessionClient } from '../src/runtime/sessionClient'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
  RuntimeMutationReceipt,
  RuntimeRunMutationRequest,
  RuntimeViewSnapshot,
} from '../src/runtime/types'

// --- the real WASM handle factory (bun-compatible init; the handle is the
// shipped `EntityStoreHandle`, identical to what the browser instantiates) ---

const WASM_DIR = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
let cachedFactory: EntityStoreHandleFactory | undefined

async function loadRealHandleFactory(): Promise<EntityStoreHandleFactory> {
  cachedFactory ??= await (async () => {
    const mod = (await import(
      join(WASM_DIR, 'posthaste_link_wasm.js')
    )) as unknown as {
      initSync(input: { module: BufferSource }): unknown
      EntityStoreHandle: new () => EntityStoreHandle
    }
    mod.initSync({
      module: readFileSync(join(WASM_DIR, 'posthaste_link_wasm_bg.wasm')),
    })
    return () => new mod.EntityStoreHandle()
  })()
  return cachedFactory
}

// --- stable hook props (mirrors MessageList's memoization) ---

const SELECTED_VIEW = {
  kind: 'source-mailbox' as const,
  sourceId: 's',
  mailboxId: 'inbox',
}
const SORT = { columnId: 'date' as const, direction: 'desc' as const }
const OPERATION = {
  operationId: 'op_1',
  operationKind: 'mail.list',
  operationSource: 'test',
  sessionId: 'sess',
}
const PREPARED_QUERY = {
  query: undefined,
  validation: { state: 'valid' as const },
  isBlocked: false,
}

/** The cache shape the hook writes (`InfiniteData<MessagePage>`). */
type MailListCache = {
  pages: MessagePage[]
  pageParams: (string | null)[]
}

function queryKey(): readonly unknown[] {
  return queryKeys.messages(SELECTED_VIEW, undefined, SORT)
}

/** One row as a renderer would see it: the observable fields, canonicalized so
 *  set-valued fields compare order-insensitively. */
export interface RenderedRow {
  messageId: string
  isRead: boolean
  isFlagged: boolean
  mailboxIds: string[]
  keywords: string[]
}

/** The rendered rows of the view at one point in the frame stream. */
export interface RenderSnapshot {
  after: string
  rows: RenderedRow[]
}

function renderedRowOf(item: MessageSummary): RenderedRow {
  return {
    messageId: item.id,
    isRead: !!item.isRead,
    isFlagged: !!item.isFlagged,
    mailboxIds: [...(item.mailboxIds ?? [])].sort(),
    keywords: [...(item.keywords ?? [])].sort(),
  }
}

function itemsOf(cache: MailListCache | undefined): MessageSummary[] {
  return cache?.pages.flatMap((p) => p.items) ?? []
}

// --- the detector (test-only logic; the production adapter is unmodified) ---

/** Whether a sequence reverts: some value appears, is replaced, and later
 *  reappears (`a … b … a`) — the signature of a visible flicker. */
function reverts<T>(
  seq: T[],
  eq: (a: T, b: T) => boolean = (a, b) => a === b,
): boolean {
  for (let i = 0; i < seq.length; i++) {
    let left = false
    for (let j = i + 1; j < seq.length; j++) {
      if (!eq(seq[j], seq[i])) {
        left = true
      } else if (left) {
        return true
      }
    }
  }
  return false
}

/** The recorded render trajectory of a view across a frame stream. */
export class RenderLog {
  constructor(public readonly snapshots: RenderSnapshot[]) {}

  /** The ids that ever appeared in the trajectory. */
  private ids(): string[] {
    const seen = new Set<string>()
    for (const snap of this.snapshots) {
      for (const row of snap.rows) seen.add(row.messageId)
    }
    return [...seen]
  }

  /** Presence (true = rendered) of `id` across snapshots. */
  private presence(id: string): boolean[] {
    return this.snapshots.map((s) => s.rows.some((r) => r.messageId === id))
  }

  /** One field's canonicalized value across snapshots where the row is
   *  present (absent snaps skipped — a row's field can't revert while absent). */
  private fieldSeq<U>(id: string, field: (r: RenderedRow) => U): U[] {
    return this.snapshots
      .filter((s) => s.rows.some((r) => r.messageId === id))
      .map((s) => field(s.rows.find((r) => r.messageId === id)!))
  }

  /** Assert no observable flicker. With `messageId`: that row's presence +
   *  `isRead`/`isFlagged`/`mailboxIds`/`keywords` must not revert. Without:
   *  the same for every id that ever appeared (whole-view snapshot regression). */
  assertNoFlicker(messageId?: string): void {
    const ids = messageId ? [messageId] : this.ids()
    for (const id of ids) {
      const presence = this.presence(id)
      assert(
        !reverts(presence),
        `row ${id} disappeared then reappeared (presence flicker)\n${this.dump()}`,
      )
      const mailboxSeq = this.fieldSeq(id, (r) => r.mailboxIds.join(','))
      assert(
        !reverts(mailboxSeq, (a, b) => a === b),
        `row ${id} mailboxIds reverted (move flicker)\n${this.dump()}`,
      )
      const keywordSeq = this.fieldSeq(id, (r) => r.keywords.join(','))
      assert(
        !reverts(keywordSeq, (a, b) => a === b),
        `row ${id} keywords reverted (keyword flicker)\n${this.dump()}`,
      )
      const readSeq = this.fieldSeq(id, (r) => r.isRead)
      assert(
        !reverts(readSeq, (a, b) => a === b),
        `row ${id} isRead reverted (read flicker)\n${this.dump()}`,
      )
      const flagSeq = this.fieldSeq(id, (r) => r.isFlagged)
      assert(
        !reverts(flagSeq, (a, b) => a === b),
        `row ${id} isFlagged reverted (flag flicker)\n${this.dump()}`,
      )
    }
  }

  /** A human-readable trajectory dump for diagnosis. */
  dump(): string {
    let out = 'render trajectory:\n'
    for (const snap of this.snapshots) {
      const rows = snap.rows.map(
        (r) =>
          `${r.messageId}[${r.mailboxIds.join('+')}${
            r.keywords.length ? '/' + r.keywords.join('+') : ''
          }${r.isRead ? '·read' : ''}${r.isFlagged ? '·flagged' : ''}]`,
      )
      out += `  [${snap.after.padStart(16)}] ${rows.join(', ')}\n`
    }
    return out
  }
}

// --- the probe: real adapter + real WASM + real hook + real render ---

function MailListHost({ spyRef }: { spyRef: { current: MessageSummary[] } }) {
  const key = useMemo(() => queryKey(), [])
  useRuntimeMailListView({
    enabled: true,
    operation: OPERATION,
    preparedSearchQuery: PREPARED_QUERY,
    queryKey: key,
    selectedView: SELECTED_VIEW,
    sort: SORT,
  })
  // The disabled infinite query observes the cache the hook writes — mirrors
  // MessageList. `placeholderData: (prev) => prev` is the production setting
  // (a suspected flash source); the spy captures the observer's `data`, which
  // reflects the placeholder, so a placeholder flash is visible to the probe.
  const query = useInfiniteQuery({
    queryKey: key,
    queryFn: async (): Promise<MessagePage> => ({
      items: [] as MessageSummary[],
      nextCursor: null,
    }),
    enabled: false,
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    placeholderData: (previousData) => previousData,
  })
  const items = itemsOf(query.data as MailListCache | undefined)
  // Capture the committed observer state (reflects `placeholderData` too) into
  // the ref after commit — refs cannot be written during render.
  useEffect(() => {
    spyRef.current = items
  })
  return (
    <>
      {items.map((i) => (
        <div key={i.id} data-testid="row">
          {i.id}
        </div>
      ))}
    </>
  )
}

export interface RenderProbeOptions {
  /** Initial open-snapshot rows. */
  rows: MessageSummary[]
  /** A stable account/source id (default `'s'`). */
  sourceId?: string
}

export class RenderProbe {
  private readonly restoreAdapter: () => void
  private readonly spy: { current: MessageSummary[] }
  private readonly snapshots: RenderSnapshot[] = []
  private readonly unmountFn: () => void
  private readonly adapter: {
    runRuntimeMutation: (
      r: RuntimeRunMutationRequest,
    ) => Promise<RuntimeMutationReceipt>
  }
  readonly queryClient: QueryClient
  readonly fakeBase: FakeRuntimeAdapter
  readonly viewId: string
  /** Every frame that reached the session client (tap for diagnosis). */
  readonly frames: RuntimeFrame<RuntimeMailListViewState>[] = []
  private frameTap?: () => void

  private constructor(args: {
    queryClient: QueryClient
    fakeBase: FakeRuntimeAdapter
    adapter: RenderProbe['adapter']
    restoreAdapter: () => void
    unmount: () => void
    viewId: string
    spy: { current: MessageSummary[] }
  }) {
    this.queryClient = args.queryClient
    this.fakeBase = args.fakeBase
    this.adapter = args.adapter
    this.restoreAdapter = args.restoreAdapter
    this.unmountFn = args.unmount
    this.viewId = args.viewId
    this.spy = args.spy
  }

  /** Wire the real adapter over a fake base + real WASM handle + real outbox,
   *  install it, render the host, and wait for the initial rows to commit. */
  static async open(opts: RenderProbeOptions): Promise<RenderProbe> {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    const fakeBase = createFakeRuntimeAdapter()
    const viewId = 'v1'
    fakeBase.queueRuntimeSession({ sessionId: 'sess' })
    fakeBase.queueRuntimeSessionMessageListView({
      viewId,
      snapshot: viewSnapshot(viewId, opts.rows, opts.sourceId ?? 's'),
    })
    const makeHandle = await loadRealHandleFactory()
    const adapter = createEntityStoreAdapter({
      base: fakeBase,
      makeHandle,
      outbox: new MemoryOutboxStore(),
      queryClient,
      now: () => 1,
    })
    const restoreAdapter = setRuntimeAdapterForTesting(adapter)

    const spy: { current: MessageSummary[] } = { current: [] }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )
    const { unmount } = render(<MailListHost spyRef={spy} />, {
      wrapper,
    })

    const probe = new RenderProbe({
      queryClient,
      fakeBase,
      adapter,
      restoreAdapter,
      unmount,
      viewId,
      spy,
    })
    // The hook's open resolves in a detached `openMessageListView().then(...)`
    // microtask, so the cache write is outside `act` and the observer re-render
    // never commits. Wait on the cache (a plain `getQueryData` read, act-free),
    // then flush one `act` so the observer commits + the spy captures rows.
    await waitFor(() => expect(probe.cacheRows().length).toBe(opts.rows.length))
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
    })
    // Tap the session-client stream for diagnosis (what reaches the hook).
    probe.frameTap = runtimeSessionClient.subscribe({
      onFrame: (f) =>
        probe.frames.push(f as RuntimeFrame<RuntimeMailListViewState>),
    })
    probe.record('open')
    return probe
  }

  /** Drive one frame through the fake base (the adapter's transport seam),
   *  wrapped in `act` so the re-render commits before recording. */
  async emitFrame(
    frame: RuntimeFrame<RuntimeMailListViewState>,
  ): Promise<void> {
    await act(async () => {
      this.fakeBase.emitRuntimeFrame(frame)
      // Flush React Query's notifyManager-deferred observer re-render so the
      // spy captures the committed render, not the pre-update one.
      await new Promise((r) => setTimeout(r, 0))
    })
  }

  /** Run a mutation through the real adapter (acceptMutation + outbox + forward
   *  to the fake base). Settlement arrives later via a `mutationNotification`
   *  frame (see {@link mutationNotificationFrame}). */
  async runMutation(
    request: RuntimeRunMutationRequest,
  ): Promise<RuntimeMutationReceipt> {
    let receipt!: RuntimeMutationReceipt
    await act(async () => {
      receipt = await this.adapter.runRuntimeMutation(request)
      await new Promise((r) => setTimeout(r, 0))
    })
    return receipt
  }

  /** Inject a stale cache write directly, bypassing the adapter. For red-first
   *  flash injection: prove the detector sees a flash. */
  async writeCache(rows: MessageSummary[]): Promise<void> {
    await act(async () => {
      this.queryClient.setQueryData<MailListCache>(queryKey() as never, {
        pages: [{ items: rows, nextCursor: null }],
        pageParams: [null],
      })
      await new Promise((r) => setTimeout(r, 0))
    })
  }

  /** The last committed rendered rows (the observer's `data`, incl. placeholder). */
  renderedRows(): RenderedRow[] {
    return this.spy.current.map(renderedRowOf)
  }

  /** The raw cache rows (`getQueryData` — does NOT reflect `placeholderData`). */
  cacheRows(): RenderedRow[] {
    return itemsOf(
      this.queryClient.getQueryData<MailListCache>(queryKey() as never),
    ).map(renderedRowOf)
  }

  /** Record the current rendered state as one snapshot. */
  record(after: string): void {
    this.snapshots.push({ after, rows: this.renderedRows() })
  }

  /** The recorded trajectory. */
  intoLog(): RenderLog {
    return new RenderLog(this.snapshots)
  }

  unmount(): void {
    this.frameTap?.()
    this.unmountFn()
    this.restoreAdapter()
    resetRuntimeSessionClientForTesting()
    resetRuntimeAdapterForTesting()
  }
}

// --- frame + snapshot builders (mirror the runtime wire format) ---

function viewSnapshot(
  viewId: string,
  rows: MessageSummary[],
  sourceId: string,
): RuntimeViewSnapshot<RuntimeMailListViewState> {
  return {
    viewId,
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
        rowKey: `${sourceId}:${row.id}`,
        resourceRef: null,
        projection: row,
        orderKey: row.id,
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

/** A `viewReplace` carrying `rows` (a full re-serve of the view). */
export function viewReplaceFrame(
  viewId: string,
  revision: number,
  rows: MessageSummary[],
  sourceId = 's',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'viewReplace',
    sessionSeq: 2,
    viewId,
    revision,
    snapshot: viewSnapshot(viewId, rows, sourceId),
  }
}

/** A `message.updated` notification re-serving `projection` (a per-message base). */
export function messageUpdatedFrame(
  messageId: string,
  projection: MessageSummary,
  sourceId = 's',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'notification',
    sessionSeq: 100,
    kind: 'message.updated',
    payload: {
      seq: 1,
      accountId: sourceId,
      topic: 'message.updated',
      occurredAt: 'now',
      payload: { messageId, projection, countDeltas: [] },
    },
  }
}

/** The terminal verdict for a named mutation (`confirmed` retires the op). */
export function mutationNotificationFrame(
  clientMutationId: string,
  verdict: 'confirmed' | 'rejected',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'mutationNotification',
    sessionSeq: 5,
    clientMutationId,
    notification:
      verdict === 'confirmed'
        ? { type: 'confirmed' }
        : {
            type: 'rejected',
            error: {
              code: 'conflict',
              message: 'rejected',
              retryable: false,
            },
          },
  }
}
