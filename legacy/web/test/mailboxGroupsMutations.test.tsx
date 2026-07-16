import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import type {
  AppSettings,
  MailboxGroup,
  PatchSettingsInput,
} from '../src/api/types'
import { useMailboxGroupMutations } from '../src/components/sidebar/useMailboxGroups'
import { queryKeys } from '../src/queryKeys'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

const baseSettings = (groups: MailboxGroup[]): AppSettings => ({
  defaultAccountId: null,
  cachePolicy: 'auto' as never,
  automationRules: [],
  automationDrafts: [],
  mailboxColors: [],
  tags: [],
  smartMailboxOrder: [],
  accountOrder: [],
  mailboxGroups: groups,
})

/** Install a fake adapter whose patchSettings records the payload and echoes a
 *  settled AppSettings, so we can assert persistence routes through the settings
 *  patch (the same path as mailboxColors / smartMailboxOrder). */
function installAdapter(calls: PatchSettingsInput[]) {
  const fake = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting({
    ...fake,
    patchSettings: (input: PatchSettingsInput) => {
      calls.push(input)
      return Promise.resolve(baseSettings(input.mailboxGroups ?? []))
    },
  })
}

function makeWrapper(qc: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
}

function readGroups(qc: QueryClient): MailboxGroup[] {
  return qc.getQueryData<AppSettings>(queryKeys.settings)?.mailboxGroups ?? []
}

describe('useMailboxGroupMutations', () => {
  it('createGroup persists through settings.patch and optimistically seeds the member', async () => {
    const calls: PatchSettingsInput[] = []
    installAdapter(calls)
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.settings, baseSettings([]))

    const { result } = renderHook(() => useMailboxGroupMutations(), {
      wrapper: makeWrapper(qc),
    })

    await act(() => {
      result.current.createGroup('Finance', 'receipts')
    })

    // Persistence routes through the settings patch path.
    await waitFor(() => expect(calls).toHaveLength(1))
    const persisted = calls[0]?.mailboxGroups ?? []
    expect(persisted).toHaveLength(1)
    expect(persisted[0]?.name).toBe('Finance')
    expect(persisted[0]?.mailboxIds).toEqual(['receipts'])
    // Optimistic cache mirrors it immediately.
    expect(readGroups(qc)[0]?.mailboxIds).toEqual(['receipts'])
  })

  it('assignToGroup moves a mailbox out of its prior group (one-group-per-mailbox)', async () => {
    const calls: PatchSettingsInput[] = []
    installAdapter(calls)
    const qc = new QueryClient()
    qc.setQueryData(
      queryKeys.settings,
      baseSettings([
        { id: 'g-a', name: 'A', mailboxIds: ['receipts', 'travel'], order: 0 },
        { id: 'g-b', name: 'B', mailboxIds: ['archive'], order: 1 },
      ]),
    )

    const { result } = renderHook(() => useMailboxGroupMutations(), {
      wrapper: makeWrapper(qc),
    })

    await act(() => {
      result.current.assignToGroup('g-b', 'receipts')
    })

    await waitFor(() => expect(calls).toHaveLength(1))
    const groups = readGroups(qc)
    expect(groups.find((g) => g.id === 'g-a')?.mailboxIds).toEqual(['travel'])
    expect(groups.find((g) => g.id === 'g-b')?.mailboxIds).toEqual([
      'archive',
      'receipts',
    ])
  })

  it('removeFromGroup ungroups a mailbox and prunes a now-empty group', async () => {
    const calls: PatchSettingsInput[] = []
    installAdapter(calls)
    const qc = new QueryClient()
    qc.setQueryData(
      queryKeys.settings,
      baseSettings([
        { id: 'g-solo', name: 'Solo', mailboxIds: ['receipts'], order: 0 },
      ]),
    )

    const { result } = renderHook(() => useMailboxGroupMutations(), {
      wrapper: makeWrapper(qc),
    })

    await act(() => {
      result.current.removeFromGroup('receipts')
    })

    await waitFor(() => expect(calls).toHaveLength(1))
    // The empty group is pruned; the mailbox itself is never touched.
    expect(readGroups(qc)).toHaveLength(0)
  })

  it('deleteGroup only ungroups members — never a mailbox mutation', async () => {
    const calls: PatchSettingsInput[] = []
    installAdapter(calls)
    // If deleteGroup ever tried to delete a mailbox this would throw (the fake's
    // deleteMailbox is left unsupported).
    const qc = new QueryClient()
    qc.setQueryData(
      queryKeys.settings,
      baseSettings([
        { id: 'g-x', name: 'X', mailboxIds: ['receipts', 'travel'], order: 0 },
      ]),
    )

    const { result } = renderHook(() => useMailboxGroupMutations(), {
      wrapper: makeWrapper(qc),
    })

    await act(() => {
      result.current.deleteGroup('g-x')
    })

    await waitFor(() => expect(calls).toHaveLength(1))
    // Group entry gone; the only mutation issued was the settings patch.
    expect(calls[0]?.mailboxGroups).toEqual([])
    expect(readGroups(qc)).toHaveLength(0)
  })

  it('renameGroup keeps membership, only changes the name', async () => {
    const calls: PatchSettingsInput[] = []
    installAdapter(calls)
    const qc = new QueryClient()
    qc.setQueryData(
      queryKeys.settings,
      baseSettings([
        { id: 'g-x', name: 'Old', mailboxIds: ['receipts'], order: 0 },
      ]),
    )

    const { result } = renderHook(() => useMailboxGroupMutations(), {
      wrapper: makeWrapper(qc),
    })

    await act(() => {
      result.current.renameGroup('g-x', 'New')
    })

    await waitFor(() => expect(calls).toHaveLength(1))
    const groups = readGroups(qc)
    expect(groups[0]?.name).toBe('New')
    expect(groups[0]?.mailboxIds).toEqual(['receipts'])
  })
})
