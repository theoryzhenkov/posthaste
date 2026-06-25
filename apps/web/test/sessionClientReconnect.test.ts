import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { waitFor } from '@testing-library/react'

import { setupDomEnvironment } from './dom-env'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import {
  resetRuntimeSessionClientForTesting,
  runtimeSessionClient,
} from '../src/runtime/sessionClient'

setupDomEnvironment()

let adapter: FakeRuntimeAdapter

beforeEach(() => {
  window.sessionStorage.clear()
  adapter = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting(adapter)
})

afterEach(() => {
  resetRuntimeSessionClientForTesting()
  resetRuntimeAdapterForTesting()
  window.sessionStorage.clear()
})

describe('runtimeSessionClient reconnect', () => {
  // Regression: when the frame stream hard-closes (an intermittent WKWebView
  // transport drop), the client used to go dead until a full page reload, so
  // live updates silently stopped. It must now reopen the stream on its own.
  it('reopens the frame stream after a hard close, without a reload', async () => {
    const unsubscribe = runtimeSessionClient.subscribe({ onFrame() {} })

    await waitFor(() =>
      expect(adapter.runtimeFrameSubscriptionCalls.length).toBe(1),
    )

    // Simulate the transport dropping out from under us.
    adapter.emitRuntimeFrameStreamClosed(new Error('stream closed'))

    // The client schedules a ~1s reconnect that resubscribes from the session's
    // current state (the runtime replies with a collapsed catch-up).
    await waitFor(
      () => expect(adapter.runtimeFrameSubscriptionCalls.length).toBe(2),
      { timeout: 3000 },
    )

    unsubscribe()
  })
})
