/**
 * OutboxPane: a dispatch-uncertain (parked) send renders as needs-attention with
 * explicit Retry/Discard, and a failed Retry surfaces a notification instead of
 * being swallowed.
 *
 * @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
 */
import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import type { AccountOverview, Operation } from '../src/api/types'
import { OutboxPane } from '../src/components/settings-panel/OutboxPane'
import {
  clearNotifications,
  getNotificationsSnapshot,
} from '../src/notifications/store'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const parkedSend: Operation = {
  id: 'op-1',
  accountId: 'primary',
  entity: { kind: 'message', id: 'send-1' },
  kind: 'send',
  payload: {},
  state: 'dispatchUncertain',
  attempts: 1,
  lastError: 'send timed out; delivery uncertain',
  dependsOn: null,
  createdAt: '2026-07-03T00:00:00Z',
  updatedAt: '2026-07-03T00:00:00Z',
}

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )
  }
}

function installAdapter(retryImpl: () => Promise<void>) {
  const base = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting({
    ...base,
    fetchAccounts: () =>
      Promise.resolve([{ id: 'primary' } as unknown as AccountOverview]),
    listPendingOperations: () => Promise.resolve([parkedSend]),
    retryOperation: retryImpl,
  })
}

describe('OutboxPane parked send', () => {
  afterEach(() => {
    resetRuntimeAdapterForTesting()
    clearNotifications()
  })

  it('renders a parked send distinctly with Retry and Discard', async () => {
    installAdapter(() => Promise.resolve())
    const Wrapper = wrapper(new QueryClient())

    const { getByText, getByRole } = render(<OutboxPane />, {
      wrapper: Wrapper,
    })

    await waitFor(() => getByText('May not have sent'))
    // The uncertainty is explained to the user.
    getByText(/may or may not have been delivered/i)
    // Both explicit actions are offered.
    getByRole('button', { name: 'Retry' })
    getByRole('button', { name: 'Discard operation' })
  })

  it('surfaces a notification when Retry fails (no silent swallow)', async () => {
    installAdapter(() => Promise.reject(new Error('runtime unavailable')))
    const Wrapper = wrapper(new QueryClient())

    const { getByRole } = render(<OutboxPane />, { wrapper: Wrapper })

    const retry = await waitFor(() => getByRole('button', { name: 'Retry' }))
    fireEvent.click(retry)

    await waitFor(() => {
      const notifications = getNotificationsSnapshot()
      expect(notifications.length).toBeGreaterThan(0)
      expect(notifications[0].severity).toBe('error')
    })
  })
})
