/**
 * The REAL frame→dispatch path with the REAL wire shape: the production
 * subscription chain — entity-store adapter (real WASM store) →
 * `runtimeLinkClient` (the one shared frame subscription) → the REAL
 * `useDaemonEvents` hook → `applyDomainEvent` — fed a notification frame that
 * mirrors, key for key, the frame captured VERBATIM from the real runtime by
 * the Rust integration test
 * `own_set_keywords_echo_arrives_on_the_link_stream_with_change_flags`
 * (crates/posthaste-authority-server/tests/authority_server_handle.rs; payload
 * emitted by `set_keywords_tx` in
 * crates/posthaste-store/src/mutations/commands.rs, envelope serialized by the
 * camelCase `DomainEvent` serde in
 * crates/posthaste-domain-model/src/model/records.rs and forwarded verbatim by
 * `forward_notification` in crates/posthaste-runtime/src/far_end/links.rs).
 *
 * Pinned behavior: the frame passes the hook's shape guard, reaches the
 * `message.updated` handler, and — because `event.payload.changes.keywords`
 * is true on the REAL shape — fires the count invalidation so the
 * smart-mailbox count query refetches. This is the suite that fails if the
 * wire shape and the client guard ever drift apart again (the class the
 * synthesized-fixture-only harness could not catch).
 */
import { afterEach, describe, expect, it } from 'bun:test'
import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClientProvider, QueryObserver } from '@tanstack/react-query'

import { createClientHarness } from './harness'
import { useDaemonEvents } from '../src/hooks/useDaemonEvents'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { resetRuntimeLinkClientForTesting } from '../src/runtime/linkClient'
import { __resetCountInvalidationForTesting } from '../src/domain-cache/mailboxCounts'
import { __resetLiveStoreForTesting } from '../src/live-store/store'
import { queryKeys } from '../src/queryKeys'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
} from '../src/runtime/types'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

// Shape-verbatim from the Rust capture (see the module doc); only ids/dates
// are renamed. Every key and nesting level — the camelCase DomainEvent
// envelope under `frame.payload`, `changes` at `payload.payload.changes` — is
// the capture's.
const CAPTURED_PROJECTION = {
  conversationId: 'conv-m1',
  fromEmail: 'alice@example.com',
  fromName: 'Alice',
  hasAttachment: false,
  id: 'm1',
  isFlagged: false,
  isRead: true,
  keywords: ['$seen'],
  mailboxIds: ['inbox'],
  preview: 'Preview',
  receivedAt: '2026-04-29T10:00:00Z',
  rfcMessageId: '<m1@example.test>',
  sourceId: 's',
  sourceName: 's',
  sourceThreadId: 'thread-m1',
  subject: 'm1',
  to: [],
}

const CAPTURED_ECHO_FRAME = {
  type: 'notification',
  linkSeq: 2,
  kind: 'message.updated',
  payload: {
    seq: 23,
    accountId: 's',
    topic: 'message.updated',
    occurredAt: '2026-07-07T10:41:46.469639502Z',
    mailboxId: 'inbox',
    messageId: 'm1',
    payload: {
      assertion: { after: CAPTURED_PROJECTION, before: null },
      changes: { keywords: true },
      keywords: ['$seen'],
      messageId: 'm1',
      projection: CAPTURED_PROJECTION,
    },
  },
} as unknown as RuntimeFrame<RuntimeMailListViewState>

let cleanup: (() => void)[] = []

afterEach(() => {
  for (const fn of cleanup.reverse()) fn()
  cleanup = []
  resetRuntimeLinkClientForTesting()
  resetRuntimeAdapterForTesting()
  __resetLiveStoreForTesting()
})

describe('useDaemonEvents over the real adapter chain, real wire shape', () => {
  it('dispatches the captured message.updated frame and refetches the smart-mailbox counts', async () => {
    const h = await createClientHarness({ coalescer: 'synchronous' })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })

    // Install the REAL entity-store adapter as the app adapter so
    // linkClient/runtimeStream route through it (the production wiring).
    setRuntimeAdapterForTesting(h.adapter)

    // Mount the REAL hook exactly as DaemonEventBridge does.
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={h.queryClient}>
        {children}
      </QueryClientProvider>
    )
    const { unmount } = renderHook(() => useDaemonEvents(), { wrapper })
    cleanup.push(unmount)

    // An active smartMailboxes observer with a counting queryFn.
    let fetches = 0
    const observer = new QueryObserver(h.queryClient, {
      queryKey: queryKeys.smartMailboxes,
      queryFn: () => {
        fetches += 1
        return Promise.resolve([{ id: 'smart-1', unreadMessages: 0 }])
      },
    })
    const unsub = observer.subscribe(() => {})
    cleanup.push(unsub)
    await waitFor(() => expect(fetches).toBeGreaterThanOrEqual(1))
    const before = fetches

    // Wait for the linkClient's shared frame subscription to bind (async
    // behind the adapter-ready gate), then deliver the captured echo the way
    // the transport does.
    await waitFor(() => expect(h.transport.subscribeCount()).toBeGreaterThan(1))
    h.emitFrame(CAPTURED_ECHO_FRAME)
    await h.flush()

    await waitFor(() => expect(fetches).toBeGreaterThan(before))
  })
})
