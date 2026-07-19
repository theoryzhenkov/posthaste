/**
 * Outbox: pending and failed local-first operations across accounts.
 *
 * Reads the `pendingOperations` family (all accounts when unscoped).
 * Successful operations settle and disappear; what remains here is
 * in-flight, retrying, or failed work. Retry and discard are the
 * `retryOperation` / `cancelOperation` commands.
 */
import type { OperationKind, OperationState, PendingOperationRow } from '@/gen'
import { useCommands, usePendingOperations } from '@/data'
import { notifyFromError } from '@/data/notifications/notifyFromError'
import { now as currentTime } from '@/lib/ambient/time'

import { SettingsPage, SettingsPageHeader } from '../panel/shared'

/**
 * Format a held send's `sendAt` for the outbox copy (e.g. "Jan 4, 9:00 AM").
 * Falls back to the raw string if unparseable.
 */
function formatScheduledTime(sendAt: string): string {
  const parsed = new Date(sendAt)
  if (Number.isNaN(parsed.getTime())) {
    return sendAt
  }
  const sameYear = parsed.getFullYear() === currentTime().getFullYear()
  return parsed.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    ...(sameYear ? {} : { year: 'numeric' }),
  })
}

/**
 * A queued send that is being HELD for a future `sendAt` (the undo-send
 * hold): shown honestly as scheduled, with the local-first caveat, and
 * cancelable (the ✕ discard) while still queued.
 */
function scheduledFor(operation: PendingOperationRow): string | null {
  if (
    operation.kind !== 'send' ||
    operation.state !== 'pending' ||
    !operation.sendAt
  ) {
    return null
  }
  return formatScheduledTime(operation.sendAt)
}

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
  dispatchUncertain: 'text-orange-700 border-orange-500/40 bg-orange-500/10',
}

const STATE_LABELS: Record<OperationState, string> = {
  pending: 'Queued',
  inflight: 'Sending',
  applied: 'Done',
  failed: 'Failed',
  dispatchUncertain: 'May not have sent',
}

/** Operation states the user can explicitly retry (re-dispatch). */
function isRetryable(state: OperationState): boolean {
  return state === 'failed' || state === 'dispatchUncertain'
}

/** Operation states the user can discard (yank from the outbox). */
function isDiscardable(state: OperationState): boolean {
  return (
    state === 'failed' || state === 'pending' || state === 'dispatchUncertain'
  )
}

export function OutboxPane() {
  const operationsQuery = usePendingOperations()
  const commands = useCommands()
  const entries = operationsQuery.data?.rows ?? []

  const discard = (accountId: string, operationId: string) => {
    void commands
      .run({ cancelOperation: { accountId, operationId } })
      .catch((error: unknown) =>
        notifyFromError(error, "Couldn't discard the operation"),
      )
  }
  const retry = (accountId: string, operationId: string) => {
    void commands
      .run({ retryOperation: { accountId, operationId } })
      .catch((error: unknown) =>
        notifyFromError(error, "Couldn't retry the operation"),
      )
  }

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Outbox"
        description="Changes made while offline are queued here and sent when your accounts reconnect."
      />
      {entries.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">Nothing queued.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {entries.map((operation) => {
            const parked = operation.state === 'dispatchUncertain'
            const scheduled = scheduledFor(operation)
            return (
              <li
                key={operation.id}
                className={`flex items-start justify-between gap-3 rounded-md border px-3 py-2 ${
                  parked
                    ? 'border-orange-500/40 bg-orange-500/5'
                    : 'border-border/70'
                }`}
              >
                <div className="min-w-0">
                  <p className="text-[13px] font-medium text-foreground">
                    {KIND_LABELS[operation.kind]}
                  </p>
                  {parked ? (
                    <p className="mt-0.5 text-[12px] text-orange-700">
                      This message may or may not have been delivered. Retry to
                      re-send it (duplicates are prevented where the provider
                      supports it), or discard it.
                    </p>
                  ) : null}
                  {scheduled ? (
                    <p className="mt-0.5 text-[12px] text-muted-foreground">
                      Scheduled for {scheduled} — sends when Posthaste is open.
                      Discard to cancel.
                    </p>
                  ) : null}
                  {operation.lastError ? (
                    <p className="mt-0.5 truncate text-[12px] text-muted-foreground">
                      {operation.lastError}
                    </p>
                  ) : null}
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {isRetryable(operation.state) ? (
                    <button
                      type="button"
                      className="rounded-md border border-border px-2 py-0.5 text-[11px] font-medium text-foreground hover:bg-muted"
                      onClick={() => retry(operation.accountId, operation.id)}
                    >
                      Retry
                    </button>
                  ) : null}
                  {isDiscardable(operation.state) ? (
                    <button
                      type="button"
                      aria-label="Discard operation"
                      title="Discard"
                      className="rounded-md border border-border px-2 py-0.5 text-[11px] font-medium text-muted-foreground hover:bg-muted"
                      onClick={() => discard(operation.accountId, operation.id)}
                    >
                      ✕
                    </button>
                  ) : null}
                  <span
                    className={`rounded-full border px-2 py-0.5 text-[11px] font-medium ${STATE_TONE[operation.state]}`}
                  >
                    {STATE_LABELS[operation.state]}
                  </span>
                </div>
              </li>
            )
          })}
        </ul>
      )}
    </SettingsPage>
  )
}
