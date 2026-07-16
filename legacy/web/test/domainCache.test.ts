import { describe, expect, it } from 'bun:test'

import type { AccountOverview } from '../src/api/types'
import { applyAccountMutationResult } from '../src/domainCache'
import { queryKeys } from '../src/queryKeys'
import { accountOverview, createQueryClient } from './domainCache.fixtures'

describe('frontend domain cache contracts', () => {
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
