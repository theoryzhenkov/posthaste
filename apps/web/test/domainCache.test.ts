import { describe, expect, it } from 'bun:test'

import type { AccountOverview } from '../src/api/types'
import {
  applyAccountMutationResult,
  invalidateComposeSendReadModels,
  invalidateMessageMutationReadModels,
  invalidateMessageScopeReadModels,
  invalidateSmartMailboxMutationReadModels,
  invalidateSyncStartedReadModels,
} from '../src/domainCache'
import { mailKeys } from '../src/mailState'
import { queryKeys } from '../src/queryKeys'
import {
  accountOverview,
  createQueryClient,
  messageSummary,
  seedMessageList,
} from './domainCache.fixtures'

describe('frontend domain cache contracts', () => {
  // spec: docs/L0-testing#frontend-state-contracts
  it('centralizes mutation invalidation scopes by intent', async () => {
    const queryClient = createQueryClient()
    const target = { sourceId: 'primary', messageId: 'message-1' }
    const mailboxList = queryKeys.mailboxes('primary')
    const smartMailbox = queryKeys.smartMailbox('sm-work')
    const messageList = queryKeys.messages({
      kind: 'source-mailbox',
      sourceId: 'primary',
      mailboxId: 'inbox',
    })
    const messageDetail = mailKeys.message('primary', 'message-1')
    const conversation = mailKeys.conversation('conversation-1')
    const conversationSummary = mailKeys.conversationSummary('conversation-1')

    queryClient.setQueryData(mailboxList, [])
    queryClient.setQueryData(queryKeys.smartMailboxes, [])
    queryClient.setQueryData(queryKeys.tags, [])
    queryClient.setQueryData(queryKeys.mailNavigationRead, { results: {} })
    queryClient.setQueryData(queryKeys.senderAddresses, [])
    queryClient.setQueryData(smartMailbox, { id: 'sm-work' })
    seedMessageList(queryClient, messageList, messageSummary())
    queryClient.setQueryData(messageDetail, { id: 'message-1' })
    queryClient.setQueryData(conversation, { id: 'conversation-1' })
    queryClient.setQueryData(conversationSummary, { id: 'conversation-1' })

    await invalidateSyncStartedReadModels(queryClient, 'primary')
    await invalidateComposeSendReadModels(queryClient, 'primary')
    await invalidateMessageMutationReadModels(queryClient, target)
    await invalidateMessageScopeReadModels(
      queryClient,
      target,
      'conversation-1',
    )
    await invalidateSmartMailboxMutationReadModels(queryClient)

    expect(queryClient.getQueryState(mailboxList)?.isInvalidated).toBe(true)
    expect(
      queryClient.getQueryState(queryKeys.smartMailboxes)?.isInvalidated,
    ).toBe(true)
    expect(queryClient.getQueryState(queryKeys.tags)?.isInvalidated).toBe(true)
    expect(
      queryClient.getQueryState(queryKeys.mailNavigationRead)?.isInvalidated,
    ).toBe(true)
    expect(
      queryClient.getQueryState(queryKeys.senderAddresses)?.isInvalidated,
    ).toBe(true)
    expect(queryClient.getQueryState(messageList)?.isInvalidated).toBe(true)
    expect(queryClient.getQueryState(messageDetail)?.isInvalidated).toBe(true)
    expect(queryClient.getQueryState(conversation)?.isInvalidated).toBe(true)
    expect(queryClient.getQueryState(conversationSummary)?.isInvalidated).toBe(
      true,
    )
    expect(queryClient.getQueryState(smartMailbox)?.isInvalidated).toBe(true)
  })

  // spec: docs/L0-testing#frontend-state-contracts
  it('keeps account mutation responses from overwriting newer runtime readiness', () => {
    const queryClient = createQueryClient()
    // Current cache entry carries live runtime (ready/connected) from events.
    const current = accountOverview()
    // A config mutation result may carry stale runtime; config must apply while
    // the live runtime is preserved.
    const staleMutationResult = accountOverview({
      name: 'Renamed Primary',
      runtime: {
        status: 'syncing',
        push: 'reconnecting',
        lastSyncAt: null,
        lastSyncError: null,
        lastSyncErrorCode: null,
        syncProgress: {
          syncId: 'sync-1',
          trigger: 'startup',
          startedAt: '2026-04-28T12:01:00Z',
          stage: 'fetching',
          detail: 'Fetching messages',
          mailboxName: 'Inbox',
          mailboxIndex: 1,
          mailboxCount: 1,
          messageCount: 1,
          totalCount: 1,
        },
      },
    })
    queryClient.setQueryData(queryKeys.accounts, [current])
    queryClient.setQueryData(queryKeys.account(current.id), current)

    applyAccountMutationResult(queryClient, staleMutationResult)

    expect(
      queryClient.getQueryData<AccountOverview[]>(queryKeys.accounts),
    ).toEqual([{ ...staleMutationResult, runtime: current.runtime }])
    expect(
      queryClient.getQueryData<AccountOverview>(queryKeys.account(current.id)),
    ).toMatchObject({
      name: 'Renamed Primary',
      runtime: current.runtime,
    })
  })
})
