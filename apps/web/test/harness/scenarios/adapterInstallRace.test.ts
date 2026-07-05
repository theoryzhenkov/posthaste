/**
 * Scenario — CL-C2 / R1: the adapter-install race.
 *
 * `installEntityStoreAdapter()` is async (WASM/worker load) and fire-and-forget;
 * the link layer's one shared frame subscription binds `getRuntimeAdapter()`
 * once. If a subscribe or view-open WINS the race against the install, the
 * session's frames used to route through the BASE adapter for the whole session:
 * no ingest, no counts, no synthesized `viewReplace` — total non-liveness until
 * a reload (which re-wins the race). Deleting the REST fallback raised the
 * stakes: the base adapter has no store behind it at all.
 *
 * The fix gates the first `openRuntimeLink` on the entity-store install
 * (`whenRuntimeAdapterReady`, bounded), so the subscription + view-open bind to
 * the entity-store adapter even when they arrive BEFORE the install resolves.
 * This drives the REAL `runtimeLinkClient` over the harness and proves a
 * subscribe/view-open that races ahead still ingests through the store.
 *
 * @spec docs/eph/AUDIT-L2-client-liveness.md (R1)
 */
import { afterEach, describe, expect, it } from 'bun:test'

import { createClientHarness, messageUpdatedFrame } from '../index'
import {
  __setRuntimeAdapterReadyGateForTesting,
  setRuntimeAdapterForTesting,
} from '../../../src/runtime/adapter'
import {
  resetRuntimeLinkClientForTesting,
  runtimeLinkClient,
} from '../../../src/runtime/linkClient'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
  RuntimeMessagePageRequest,
} from '../../../src/runtime/types'

const VIEW_REQUEST: RuntimeMessagePageRequest = {
  scope: { kind: 'source-mailbox', sourceId: 's', mailboxId: 'inbox' },
  limit: 50,
  sort: 'date',
  sortDir: 'desc',
  operation: { name: 'test' } as never,
}

const flaggedUpdate = (id: string, receivedAt: string) =>
  messageUpdatedFrame(id, {
    id,
    sourceId: 's',
    receivedAt,
    keywords: ['$flagged'],
    mailboxIds: ['inbox'],
    isRead: false,
    isFlagged: true,
    subject: id,
  })

let restoreAdapter: (() => void) | undefined

afterEach(() => {
  restoreAdapter?.()
  restoreAdapter = undefined
  resetRuntimeLinkClientForTesting()
})

describe('scenario CL-C2/R1: adapter-install race', () => {
  it('a subscribe + view-open that race ahead of the install still bind to the entity-store adapter', async () => {
    const h = await createClientHarness()
    resetRuntimeLinkClientForTesting()
    // The harness binds its own frame subscription in setup; measure new binds
    // relative to that baseline.
    const baseSubscribes = h.transport.subscribeCount()

    // Model the race: the BASE adapter is active and the entity-store install is
    // still in flight when the session subscribes + opens a view.
    restoreAdapter = setRuntimeAdapterForTesting(h.transport.base)
    let resolveInstall!: () => void
    __setRuntimeAdapterReadyGateForTesting(
      new Promise<void>((resolve) => {
        resolveInstall = resolve
      }),
    )

    const collected: RuntimeFrame<RuntimeMailListViewState>[] = []
    runtimeLinkClient.subscribe({ onFrame: (f) => collected.push(f) })
    const openPromise = runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    // The gate holds the first link open: nothing new has bound yet — so the
    // subscription can't have stranded on the base adapter.
    await Promise.resolve()
    expect(h.transport.subscribeCount()).toBe(baseSubscribes)

    // The install completes: the entity-store adapter becomes active and the
    // gate releases.
    setRuntimeAdapterForTesting(h.adapter)
    resolveInstall()
    await openPromise

    // The subscription bound (through the entity-store adapter's controller,
    // which subscribes the base underneath).
    expect(h.transport.subscribeCount()).toBe(baseSubscribes + 1)

    // The payoff: a `message.updated` now ingests + the store synthesizes a
    // `viewReplace` — behavior ONLY the entity-store adapter produces. A session
    // stranded on the base adapter would forward the raw notification and never
    // synthesize a row projection.
    await h.flush()
    h.transport.emitFrame(flaggedUpdate('m1', '2026-04-29T10:00:00Z'))
    await h.flush()

    const replaces = collected.filter((f) => f.type === 'viewReplace')
    expect(replaces.length).toBeGreaterThan(0)
    const last = replaces.at(-1)
    expect(
      last?.type === 'viewReplace'
        ? last.snapshot.data.rows.map(
            (r) => (r.projection as { id: string }).id,
          )
        : undefined,
    ).toContain('m1')

    h.dispose()
  })

  it('an already-installed adapter (gate resolved) opens the link without waiting', async () => {
    const h = await createClientHarness()
    resetRuntimeLinkClientForTesting()
    const baseSubscribes = h.transport.subscribeCount()
    // The common steady state: install already done, entity-store adapter active,
    // gate resolved (the reset leaves it resolved).
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)

    const collected: RuntimeFrame<RuntimeMailListViewState>[] = []
    runtimeLinkClient.subscribe({ onFrame: (f) => collected.push(f) })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    expect(h.transport.subscribeCount()).toBe(baseSubscribes + 1)

    await h.flush()
    h.transport.emitFrame(flaggedUpdate('m2', '2026-04-28T10:00:00Z'))
    await h.flush()
    expect(collected.some((f) => f.type === 'viewReplace')).toBe(true)

    h.dispose()
  })
})
