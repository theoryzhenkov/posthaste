import { describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'

import { createAccountDirectory } from '../src/accountDirectory'
import type { AccountOverview, Mailbox, MessageSummary } from '../src/api/types'
import { useMailboxDirectory } from '../src/components/message-list/useMailboxDirectory'
import { queryKeys } from '../src/queryKeys'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function makeAccount(id: string, name: string): AccountOverview {
  return {
    id,
    name,
    fullName: null,
    emailPatterns: [`${id}@example.com`],
    driver: 'mock',
    enabled: true,
    appearance: { kind: 'initials', initials: name[0], colorHue: 200 },
    connection: {
      kind: 'manualCredentials',
      provider: 'generic',
      providerKind: 'generic',
      auth: 'password',
      baseUrl: null,
      username: `${id}@example.com`,
      imap: null,
      smtp: null,
      secret: { storage: 'os', configured: true, label: null },
    },
    createdAt: '2026-04-28T12:00:00Z',
    updatedAt: '2026-04-28T12:00:00Z',
    isDefault: id === 'account-1',
    runtime: {
      status: 'ready',
      push: 'disabled',
      lastSyncAt: null,
      lastSyncError: null,
      lastSyncErrorCode: null,
      syncProgress: null,
    },
  }
}

function makeMailbox(id: string, name: string, role: string | null): Mailbox {
  return { id, name, role, unreadEmails: 0, totalEmails: 0 }
}

function makeMessage(overrides: Partial<MessageSummary>): MessageSummary {
  return {
    id: 'message-1',
    sourceId: 'account-1',
    sourceName: 'Work',
    sourceThreadId: 'thread-1',
    conversationId: 'conversation-1',
    subject: 'Hello',
    fromName: 'Sender',
    fromEmail: 'sender@example.test',
    to: [],
    preview: 'Preview',
    receivedAt: '2026-05-31T00:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: [],
    keywords: [],
    ...overrides,
  }
}

function makeWrapper(qc: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
}

describe('useMailboxDirectory', () => {
  it('resolves a message to its mailbox name/role from the already-cached per-account read model (no fetch)', () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('account-1'), [
      makeMailbox('inbox', 'Inbox', 'inbox'),
      makeMailbox('archive', 'Archive', 'archive'),
    ])
    const directory = createAccountDirectory([makeAccount('account-1', 'Work')])

    const { result } = renderHook(() => useMailboxDirectory(directory), {
      wrapper: makeWrapper(qc),
    })

    const message = makeMessage({ mailboxIds: ['archive'] })
    const resolved = result.current.resolve(message, null)

    expect(resolved).not.toBeNull()
    expect(resolved?.mailbox.name).toBe('Archive')
    expect(resolved?.mailbox.role).toBe('archive')
    expect(resolved?.isMultiAccount).toBe(false)
  })

  it('in multi-membership, prefers a role-bearing mailbox over a role-less one', () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('account-1'), [
      makeMailbox('inbox', 'Inbox', 'inbox'),
      makeMailbox('label-project-x', 'Project X', null),
    ])
    const directory = createAccountDirectory([makeAccount('account-1', 'Work')])
    const { result } = renderHook(() => useMailboxDirectory(directory), {
      wrapper: makeWrapper(qc),
    })

    const message = makeMessage({
      mailboxIds: ['label-project-x', 'inbox'],
    })
    const resolved = result.current.resolve(message, null)
    expect(resolved?.mailbox.name).toBe('Inbox')
  })

  it('excludes the currently-viewed mailbox, preferring another membership', () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('account-1'), [
      makeMailbox('inbox', 'Inbox', 'inbox'),
      makeMailbox('label-project-x', 'Project X', null),
    ])
    const directory = createAccountDirectory([makeAccount('account-1', 'Work')])
    const { result } = renderHook(() => useMailboxDirectory(directory), {
      wrapper: makeWrapper(qc),
    })

    const message = makeMessage({
      mailboxIds: ['inbox', 'label-project-x'],
    })
    const resolved = result.current.resolve(message, 'inbox')
    expect(resolved?.mailbox.name).toBe('Project X')
  })

  it('falls back to the excluded mailbox when it is the only membership', () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('account-1'), [
      makeMailbox('inbox', 'Inbox', 'inbox'),
    ])
    const directory = createAccountDirectory([makeAccount('account-1', 'Work')])
    const { result } = renderHook(() => useMailboxDirectory(directory), {
      wrapper: makeWrapper(qc),
    })

    const message = makeMessage({ mailboxIds: ['inbox'] })
    const resolved = result.current.resolve(message, 'inbox')
    expect(resolved?.mailbox.name).toBe('Inbox')
  })

  it('flags isMultiAccount when more than one account is in scope, for the account-name prefix', () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('account-1'), [
      makeMailbox('inbox', 'Inbox', 'inbox'),
    ])
    qc.setQueryData(queryKeys.mailboxes('account-2'), [
      makeMailbox('inbox-2', 'Inbox', 'inbox'),
    ])
    const directory = createAccountDirectory([
      makeAccount('account-1', 'Work'),
      makeAccount('account-2', 'Personal'),
    ])
    const { result } = renderHook(() => useMailboxDirectory(directory), {
      wrapper: makeWrapper(qc),
    })

    const message = makeMessage({
      sourceId: 'account-1',
      mailboxIds: ['inbox'],
    })
    const resolved = result.current.resolve(message, null)
    expect(resolved?.isMultiAccount).toBe(true)
    expect(resolved?.accountName).toBe('Work')
  })

  it('resolves to null (no chip) when the account has no cached mailboxes yet', () => {
    const qc = new QueryClient()
    const directory = createAccountDirectory([makeAccount('account-1', 'Work')])
    const { result } = renderHook(() => useMailboxDirectory(directory), {
      wrapper: makeWrapper(qc),
    })

    const message = makeMessage({ mailboxIds: ['inbox'] })
    expect(result.current.resolve(message, null)).toBeNull()
  })

  it('resolves to null when the message carries a mailboxId not present in the cached mailboxes', () => {
    const qc = new QueryClient()
    qc.setQueryData(queryKeys.mailboxes('account-1'), [
      makeMailbox('inbox', 'Inbox', 'inbox'),
    ])
    const directory = createAccountDirectory([makeAccount('account-1', 'Work')])
    const { result } = renderHook(() => useMailboxDirectory(directory), {
      wrapper: makeWrapper(qc),
    })

    const message = makeMessage({ mailboxIds: ['deleted-mailbox'] })
    expect(result.current.resolve(message, null)).toBeNull()
  })
})
