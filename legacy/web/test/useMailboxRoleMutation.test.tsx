import { afterEach, describe, expect, it, spyOn } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import * as apiClient from '../src/api/client'
import type { Mailbox } from '../src/api/types'
import { queryKeys } from '../src/queryKeys'
import { useMailboxRoleMutation } from '../src/components/settings-panel/useMailboxRoleMutation'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const mkMailbox = (id: string, role: string | null): Mailbox => ({
  id,
  name: id,
  role,
  unreadEmails: 0,
  totalEmails: 0,
})

const initialMailboxes: Mailbox[] = [
  mkMailbox('m-inbox', 'inbox'),
  mkMailbox('m-archive', 'archive'),
]

function makeWrapper(qc: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
}

afterEach(() => {
  // spyOn restores are per-test; nothing global to reset here.
})

describe('useMailboxRoleMutation', () => {
  it('optimistically applies the role + clears it from the prior holder before the patch resolves', async () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('acct'), initialMailboxes)

    let resolvePatch!: (value: Mailbox[]) => void
    const spy = spyOn(apiClient, 'patchMailbox').mockReturnValue(
      new Promise<Mailbox[]>((resolve) => {
        resolvePatch = resolve
      }),
    )

    const { result } = renderHook(
      () => useMailboxRoleMutation('acct', 'm-inbox'),
      { wrapper: makeWrapper(qc) },
    )

    // Fire the mutation (don't await — we want to observe the optimistic state
    // before the backend round-trip resolves).
    await act(() => {
      result.current.mutate('archive')
    })

    await waitFor(() => {
      const data = qc.getQueryData<Mailbox[]>(queryKeys.mailboxes('acct'))
      expect(data?.find((m) => m.id === 'm-inbox')?.role).toBe('archive')
      // The backend clears the role from the mailbox that held it; the
      // optimistic update mirrors that so two mailboxes don't show 'archive'.
      expect(data?.find((m) => m.id === 'm-archive')?.role).toBeNull()
    })

    // The patch hasn't resolved yet.
    expect(spy).toHaveBeenCalledWith('acct', 'm-inbox', { role: 'archive' })

    // Reconcile with the authoritative response.
    resolvePatch([
      mkMailbox('m-inbox', 'archive'),
      mkMailbox('m-archive', null),
    ])
    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    const settled = qc.getQueryData<Mailbox[]>(queryKeys.mailboxes('acct'))
    expect(settled?.find((m) => m.id === 'm-inbox')?.role).toBe('archive')

    spy.mockRestore()
  })

  it('rolls back the optimistic update when the patch fails', async () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('acct'), initialMailboxes)

    const spy = spyOn(apiClient, 'patchMailbox').mockRejectedValue(
      new Error('gateway rejected'),
    )

    const { result } = renderHook(
      () => useMailboxRoleMutation('acct', 'm-inbox'),
      { wrapper: makeWrapper(qc) },
    )

    await act(() => {
      result.current.mutate('archive')
    })

    await waitFor(() => expect(result.current.isError).toBe(true))

    // Rolled back to the previous state.
    const data = qc.getQueryData<Mailbox[]>(queryKeys.mailboxes('acct'))
    expect(data?.find((m) => m.id === 'm-inbox')?.role).toBe('inbox')
    expect(data?.find((m) => m.id === 'm-archive')?.role).toBe('archive')

    spy.mockRestore()
  })
})
