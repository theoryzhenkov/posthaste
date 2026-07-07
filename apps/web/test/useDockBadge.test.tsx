import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  spyOn,
} from 'bun:test'
import type { ReactNode } from 'react'
import { render, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import * as tauriWindow from '@tauri-apps/api/window'

import type { AccountOverview, Mailbox } from '../src/api/types'
import { queryKeys } from '../src/queryKeys'
import { __resetLiveStoreForTesting } from '../src/live-store/store'
import { accountInboxUnread, useDockBadge } from '../src/hooks/useDockBadge'
import { DockBadge } from '../src/hooks/DockBadge'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

// The Tauri badge sink. Spied per-test (restored in afterEach) so it never leaks
// into other files sharing this bun process.
const setBadgeCount = mock(() => Promise.resolve())
let getWindowSpy: ReturnType<typeof spyOn> | undefined

// `isTauriRuntime()` keys off `window.__TAURI_INTERNALS__`; toggle it directly
// (real guard, no module mock) to simulate the desktop webview vs. the browser.
function setTauri(on: boolean): void {
  const w = window as unknown as Record<string, unknown>
  if (on) {
    w.__TAURI_INTERNALS__ = {}
  } else {
    delete w.__TAURI_INTERNALS__
  }
}

function mailbox(id: string, role: string | null, unread: number): Mailbox {
  return { id, name: id, role, unreadEmails: unread, totalEmails: unread }
}

function account(id: string, enabled: boolean): AccountOverview {
  return { id, enabled } as unknown as AccountOverview
}

let queryClient: QueryClient

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )
}

beforeEach(() => {
  setBadgeCount.mockClear()
  __resetLiveStoreForTesting()
  setTauri(true)
  getWindowSpy = spyOn(tauriWindow, 'getCurrentWindow').mockReturnValue({
    setBadgeCount,
  } as unknown as tauriWindow.Window)
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
})

afterEach(() => {
  getWindowSpy?.mockRestore()
  setTauri(false)
})

describe('accountInboxUnread', () => {
  it('sums inbox-role mailboxes only from the react-query rows', () => {
    const mailboxes = [
      mailbox('inbox', 'inbox', 2),
      mailbox('junk', 'junk', 5),
      mailbox('archive', 'archive', 9),
    ]
    // Only the inbox-role mailbox counts; junk/archive never inflate the badge
    // even though they carry unread.
    expect(accountInboxUnread(mailboxes)).toBe(2)
  })

  it('sums multiple inbox-role mailboxes', () => {
    expect(
      accountInboxUnread([
        mailbox('inbox-a', 'inbox', 4),
        mailbox('inbox-b', 'inbox', 3),
      ]),
    ).toBe(7)
  })
})

describe('DockBadge', () => {
  it('sets the badge to the total inbox unread across accounts (react-query rows)', async () => {
    queryClient.setQueryData(queryKeys.accounts, [
      account('a', true),
      account('b', true),
    ])
    queryClient.setQueryData(queryKeys.mailboxes('a'), [
      mailbox('a-inbox', 'inbox', 3),
      mailbox('a-junk', 'junk', 5),
    ])
    queryClient.setQueryData(queryKeys.mailboxes('b'), [
      mailbox('b-inbox', 'inbox', 1),
    ])

    render(<DockBadge />, { wrapper })

    await waitFor(() => expect(setBadgeCount).toHaveBeenCalled())
    expect(setBadgeCount).toHaveBeenLastCalledWith(4)
  })

  it('re-pushes the badge when the count query data moves (the overlay/refetch path)', async () => {
    queryClient.setQueryData(queryKeys.accounts, [account('a', true)])
    queryClient.setQueryData(queryKeys.mailboxes('a'), [
      mailbox('a-inbox', 'inbox', 2),
    ])

    render(<DockBadge />, { wrapper })
    await waitFor(() => expect(setBadgeCount).toHaveBeenLastCalledWith(2))

    // A count-affecting change lands in the query cache (an invalidation
    // refetch or the optimistic overlay's setQueryData): the badge follows.
    queryClient.setQueryData(queryKeys.mailboxes('a'), [
      mailbox('a-inbox', 'inbox', 1),
    ])
    await waitFor(() => expect(setBadgeCount).toHaveBeenLastCalledWith(1))
  })

  it('clears the badge (undefined) when the inbox unread is zero', async () => {
    queryClient.setQueryData(queryKeys.accounts, [account('a', true)])
    queryClient.setQueryData(queryKeys.mailboxes('a'), [
      mailbox('a-inbox', 'inbox', 0),
    ])

    render(<DockBadge />, { wrapper })

    await waitFor(() => expect(setBadgeCount).toHaveBeenCalled())
    expect(setBadgeCount).toHaveBeenLastCalledWith(undefined)
  })

  it('is a no-op outside Tauri (browser build)', async () => {
    setTauri(false)
    queryClient.setQueryData(queryKeys.accounts, [account('a', true)])
    queryClient.setQueryData(queryKeys.mailboxes('a'), [
      mailbox('a-inbox', 'inbox', 7),
    ])

    render(<DockBadge />, { wrapper })

    // Give the effect a chance to (not) fire.
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(setBadgeCount).not.toHaveBeenCalled()
  })
})

describe('useDockBadge dedupe', () => {
  it('pushes only on change and clears at zero, never redundantly', async () => {
    const { rerender } = renderHook(({ n }) => useDockBadge(n), {
      initialProps: { n: 4 },
    })

    await waitFor(() => expect(setBadgeCount).toHaveBeenCalledTimes(1))
    expect(setBadgeCount).toHaveBeenLastCalledWith(4)

    // Same count -> no additional call.
    rerender({ n: 4 })
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(setBadgeCount).toHaveBeenCalledTimes(1)

    // Drop to zero -> clears with undefined.
    rerender({ n: 0 })
    await waitFor(() => expect(setBadgeCount).toHaveBeenCalledTimes(2))
    expect(setBadgeCount).toHaveBeenLastCalledWith(undefined)
  })
})
