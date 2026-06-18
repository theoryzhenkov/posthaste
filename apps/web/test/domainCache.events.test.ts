import { describe, expect, it } from 'bun:test'

import { applyDomainEvent } from '../src/domainCache'
import { EVENT_TOPICS } from '../src/domainVocabulary'
import { mailKeys } from '../src/mailState'
import { queryKeys } from '../src/queryKeys'
import {
  accountOverview,
  createQueryClient,
  domainEvent,
  messageSummary,
  seedMessageList,
} from './domainCache.fixtures'

describe('frontend domain cache event contracts', () => {
  it('invalidates message list views when a remote event can change visible rows', () => {
    const queryClient = createQueryClient()
    const messageList = queryKeys.messages({
      kind: 'source-mailbox',
      sourceId: 'primary',
      mailboxId: 'inbox',
    })
    seedMessageList(queryClient, messageList, messageSummary())

    applyDomainEvent(queryClient, domainEvent())

    expect(queryClient.getQueryState(messageList)?.isInvalidated).toBe(true)
  })

  it('invalidates settings and account read models when app settings change', () => {
    const queryClient = createQueryClient()
    queryClient.setQueryData(queryKeys.settings, {
      defaultAccountId: 'primary',
    })
    queryClient.setQueryData(queryKeys.accounts, [accountOverview()])

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: EVENT_TOPICS.SettingsUpdated,
        accountId: 'app',
        messageId: null,
        mailboxId: null,
        payload: { scope: 'app' },
      }),
    )

    expect(queryClient.getQueryState(queryKeys.settings)?.isInvalidated).toBe(
      true,
    )
    expect(queryClient.getQueryState(queryKeys.accounts)?.isInvalidated).toBe(
      true,
    )
  })

  it('falls back to broad app invalidation for unknown event topics', () => {
    const queryClient = createQueryClient()
    queryClient.setQueryData(queryKeys.settings, {
      defaultAccountId: 'primary',
    })
    queryClient.setQueryData(queryKeys.accounts, [accountOverview()])

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: 'future.topic',
        accountId: 'app',
        messageId: null,
        mailboxId: null,
      }),
    )

    expect(queryClient.getQueryState(queryKeys.settings)?.isInvalidated).toBe(
      true,
    )
    expect(queryClient.getQueryState(queryKeys.accounts)?.isInvalidated).toBe(
      true,
    )
  })

  it('invalidates account-backed read models when account appearance changes', () => {
    const queryClient = createQueryClient()
    const account = accountOverview()
    queryClient.setQueryData(queryKeys.accounts, [account])
    queryClient.setQueryData(queryKeys.account(account.id), account)

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: EVENT_TOPICS.AccountUpdated,
        accountId: account.id,
        messageId: null,
        mailboxId: null,
        payload: {
          resources: [
            { kind: 'account', operation: 'updated', id: account.id },
          ],
        },
      }),
    )

    expect(queryClient.getQueryState(queryKeys.accounts)?.isInvalidated).toBe(
      true,
    )
    expect(
      queryClient.getQueryState(queryKeys.account(account.id))?.isInvalidated,
    ).toBe(true)
  })

  it('invalidates broad read models when config reloads', () => {
    const queryClient = createQueryClient()
    const smartMailbox = queryKeys.smartMailbox('sm-work')
    const messageDetailKey = mailKeys.message('primary', 'message-1')
    queryClient.setQueryData(queryKeys.settings, {
      defaultAccountId: 'primary',
    })
    queryClient.setQueryData(queryKeys.accounts, [accountOverview()])
    queryClient.setQueryData(queryKeys.mailNavigationRead, { results: {} })
    queryClient.setQueryData(queryKeys.smartMailboxes, [])
    queryClient.setQueryData(smartMailbox, { id: 'sm-work' })
    queryClient.setQueryData(messageDetailKey, { id: 'message-1' })

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: EVENT_TOPICS.ConfigReloaded,
        accountId: 'app',
        messageId: null,
        mailboxId: null,
        payload: { resources: [{ kind: 'config', operation: 'reloaded' }] },
      }),
    )

    expect(queryClient.getQueryState(queryKeys.settings)?.isInvalidated).toBe(
      true,
    )
    expect(queryClient.getQueryState(queryKeys.accounts)?.isInvalidated).toBe(
      true,
    )
    expect(
      queryClient.getQueryState(queryKeys.smartMailboxes)?.isInvalidated,
    ).toBe(true)
    expect(
      queryClient.getQueryState(queryKeys.mailNavigationRead)?.isInvalidated,
    ).toBe(true)
    expect(queryClient.getQueryState(messageDetailKey)?.isInvalidated).toBe(
      true,
    )
  })

  it('invalidates smart mailbox read models when smart mailbox config changes', () => {
    const queryClient = createQueryClient()
    const smartMailbox = queryKeys.smartMailbox('sm-work')
    const messageList = queryKeys.messages({
      kind: 'smart-mailbox',
      id: 'sm-work',
    })
    queryClient.setQueryData(queryKeys.smartMailboxes, [])
    queryClient.setQueryData(smartMailbox, { id: 'sm-work' })
    seedMessageList(queryClient, messageList, messageSummary())

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: EVENT_TOPICS.SmartMailboxUpdated,
        accountId: 'app',
        messageId: null,
        mailboxId: null,
        payload: { smartMailboxId: 'sm-work' },
      }),
    )

    expect(
      queryClient.getQueryState(queryKeys.smartMailboxes)?.isInvalidated,
    ).toBe(true)
    expect(queryClient.getQueryState(smartMailbox)?.isInvalidated).toBe(true)
    expect(queryClient.getQueryState(messageList)?.isInvalidated).toBe(true)
  })

  it('invalidates message details after full sync repair events', () => {
    const queryClient = createQueryClient()
    const messageDetailKey = mailKeys.message('primary', 'message-1')
    const conversationKey = mailKeys.conversation('conversation-1')
    queryClient.setQueryData(messageDetailKey, { id: 'message-1' })
    queryClient.setQueryData(conversationKey, { id: 'conversation-1' })

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: EVENT_TOPICS.SyncCompleted,
        messageId: null,
        mailboxId: null,
        payload: { mode: 'fullMetadata' },
      }),
    )

    expect(queryClient.getQueryState(messageDetailKey)?.isInvalidated).toBe(
      true,
    )
    expect(queryClient.getQueryState(conversationKey)?.isInvalidated).toBe(true)
  })

  it('invalidates target message detail when a body is cached', () => {
    const queryClient = createQueryClient()
    const messageDetailKey = mailKeys.message('primary', 'message-1')
    queryClient.setQueryData(messageDetailKey, { id: 'message-1' })

    applyDomainEvent(
      queryClient,
      domainEvent({ topic: EVENT_TOPICS.MessageBodyCached }),
    )

    expect(queryClient.getQueryState(messageDetailKey)?.isInvalidated).toBe(
      true,
    )
  })

  it('invalidates mailbox read models when a mailbox changes remotely', () => {
    const queryClient = createQueryClient()
    const mailboxList = queryKeys.mailboxes('primary')
    const messageList = queryKeys.messages({
      kind: 'source-mailbox',
      sourceId: 'primary',
      mailboxId: 'inbox',
    })
    queryClient.setQueryData(mailboxList, [])
    queryClient.setQueryData(queryKeys.smartMailboxes, [])
    seedMessageList(queryClient, messageList, messageSummary())

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: EVENT_TOPICS.MailboxUpdated,
        messageId: null,
        payload: { mailboxId: 'inbox' },
      }),
    )

    expect(queryClient.getQueryState(mailboxList)?.isInvalidated).toBe(true)
    expect(
      queryClient.getQueryState(queryKeys.smartMailboxes)?.isInvalidated,
    ).toBe(true)
    expect(queryClient.getQueryState(messageList)?.isInvalidated).toBe(true)
  })
})
