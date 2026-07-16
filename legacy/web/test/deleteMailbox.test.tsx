import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import type { Mailbox } from '../src/api/types'
import { DeleteMailboxDialog } from '../src/components/sidebar/DeleteMailboxDialog'
import { isMailboxDeletable } from '../src/components/sidebar/model'
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

const mkMailbox = (
  id: string,
  role: string | null,
  totalEmails = 0,
): Mailbox => ({
  id,
  name: id,
  role,
  unreadEmails: 0,
  totalEmails,
})

type DeleteCall = {
  accountId: string
  mailboxId: string
  removeEmails: boolean
}

function installFakeAdapter(calls: DeleteCall[], result: Mailbox[] = []): void {
  const fake = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting({
    ...fake,
    deleteMailbox: (accountId, mailboxId, input) => {
      calls.push({ accountId, mailboxId, removeEmails: input.removeEmails })
      return Promise.resolve(result)
    },
  })
}

function wrap(node: ReactNode): ReactNode {
  return (
    <QueryClientProvider client={new QueryClient()}>{node}</QueryClientProvider>
  )
}

describe('DeleteMailboxDialog', () => {
  it('shows the message count for a non-empty mailbox and only mutates on confirm', async () => {
    const calls: DeleteCall[] = []
    installFakeAdapter(calls)

    const { getByRole, getByText } = render(
      wrap(
        <DeleteMailboxDialog
          sourceId="acct"
          mailbox={mkMailbox('m-receipts', null, 5)}
          open
          onOpenChange={() => {}}
        />,
      ),
    )

    // The confirm-with-count copy names the count from the mailbox total.
    expect(getByText(/5 messages/)).toBeDefined()
    // Nothing is deleted until the user confirms.
    expect(calls).toHaveLength(0)

    fireEvent.click(getByRole('button', { name: 'Delete messages' }))

    await waitFor(() => expect(calls).toHaveLength(1))
    expect(calls[0]).toEqual({
      accountId: 'acct',
      mailboxId: 'm-receipts',
      removeEmails: true,
    })
  })

  it('an empty mailbox deletes without removeEmails', async () => {
    const calls: DeleteCall[] = []
    installFakeAdapter(calls)

    const { getByRole } = render(
      wrap(
        <DeleteMailboxDialog
          sourceId="acct"
          mailbox={mkMailbox('m-empty', null, 0)}
          open
          onOpenChange={() => {}}
        />,
      ),
    )

    fireEvent.click(getByRole('button', { name: 'Delete' }))

    await waitFor(() => expect(calls).toHaveLength(1))
    expect(calls[0]?.removeEmails).toBe(false)
  })
})

describe('MailboxItem delete affordance (role protection)', () => {
  it('offers delete for a plain user mailbox', () => {
    expect(isMailboxDeletable(mkMailbox('m-receipts', null))).toBe(true)
  })

  it('never offers delete for a protected role mailbox', () => {
    for (const role of [
      'inbox',
      'sent',
      'trash',
      'drafts',
      'archive',
      'junk',
    ]) {
      expect(isMailboxDeletable(mkMailbox(`m-${role}`, role))).toBe(false)
    }
  })
})
