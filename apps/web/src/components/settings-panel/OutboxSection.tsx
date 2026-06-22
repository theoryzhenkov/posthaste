/**
 * Outbox: pending and failed local-first operations across accounts.
 *
 * Reads each account's non-terminal outbox operations (queued drafts and, in
 * future, other offline mutations). Successful operations settle and disappear;
 * what remains here is in-flight, retrying, or failed work.
 *
 * @spec docs/L1-outbox#operation-model
 */
import { useQueries, useQuery } from '@tanstack/react-query'

import type { Operation, OperationKind, OperationState } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'
import { runtimeViews } from '@/runtime/views'

import { SettingsSection } from './shared'

const KIND_LABELS: Record<OperationKind, string> = {
  setKeywords: 'Flag change',
  replaceMailboxes: 'Move',
  destroy: 'Delete',
  draftCreate: 'Draft',
  draftUpdate: 'Draft edit',
  draftDelete: 'Draft deletion',
  send: 'Send',
}

const STATE_TONE: Record<OperationState, string> = {
  pending: 'text-amber-700 border-amber-500/30 bg-amber-500/10',
  inflight: 'text-blue-700 border-blue-500/30 bg-blue-500/10',
  applied: 'text-emerald-700 border-emerald-500/30 bg-emerald-500/10',
  failed: 'text-rose-700 border-rose-500/30 bg-rose-500/10',
}

const STATE_LABELS: Record<OperationState, string> = {
  pending: 'Queued',
  inflight: 'Sending',
  applied: 'Done',
  failed: 'Failed',
}

export function OutboxSection() {
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: runtimeViews.accounts.list,
  })
  const accountIds = (accountsQuery.data ?? []).map((account) => account.id)

  const operationQueries = useQueries({
    queries: accountIds.map((accountId) => ({
      queryKey: queryKeys.pendingOperations(accountId),
      queryFn: () => runtimeMutations.messages.listPendingOperations(accountId),
      // Pending work changes as it flushes; keep this lightly fresh while open.
      refetchInterval: 5000,
    })),
  })

  const operations: Operation[] = operationQueries.flatMap(
    (query) => query.data ?? [],
  )

  return (
    <SettingsSection title="Outbox">
      <p className="text-[12px] leading-5 text-muted-foreground">
        Changes made while offline are queued here and sent when your accounts
        reconnect.
      </p>
      {operations.length === 0 ? (
        <p className="mt-3 text-[13px] text-muted-foreground">
          Nothing queued.
        </p>
      ) : (
        <ul className="mt-3 flex flex-col gap-2">
          {operations.map((operation) => (
            <li
              key={operation.id}
              className="flex items-start justify-between gap-3 rounded-md border border-border/70 px-3 py-2"
            >
              <div className="min-w-0">
                <p className="text-[13px] font-medium text-foreground">
                  {KIND_LABELS[operation.kind]}
                </p>
                {operation.lastError ? (
                  <p className="mt-0.5 truncate text-[12px] text-muted-foreground">
                    {operation.lastError}
                  </p>
                ) : null}
              </div>
              <span
                className={`shrink-0 rounded-full border px-2 py-0.5 text-[11px] font-medium ${STATE_TONE[operation.state]}`}
              >
                {STATE_LABELS[operation.state]}
              </span>
            </li>
          ))}
        </ul>
      )}
    </SettingsSection>
  )
}
