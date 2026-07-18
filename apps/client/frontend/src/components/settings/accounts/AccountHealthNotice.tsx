/**
 * Visible degraded/error state + recovery affordance for an account.
 *
 * Renders the classified health message (never a raw provider/library string)
 * and, when a recovery action applies, a Retry / Reconnect button that triggers
 * a fresh sync/connection attempt. The supervisor clears the error latch on the
 * next successful sync, so the notice disappears once recovery succeeds.
 */
import { AlertTriangle, RefreshCw, RotateCcw } from 'lucide-react'

import { accountHealthFor } from '../../../data/models/accountHealth'
import type { AccountRow } from '@/gen'
import { Button } from '../../ui/form/button'

const SEVERITY_CLASS = {
  error: 'border-rose-500/30 bg-rose-500/5 text-rose-700',
  warn: 'border-amber-500/30 bg-amber-500/5 text-amber-800',
} as const

export function AccountHealthNotice({
  account,
  onAction,
  isActionPending = false,
  compact = false,
}: {
  account: AccountRow
  /** Trigger the account's recovery action (a fresh sync/connection attempt). */
  onAction?: (account: AccountRow) => void
  isActionPending?: boolean
  compact?: boolean
}) {
  const health = accountHealthFor(account)
  if (!health.isUnhealthy || !health.message) {
    return null
  }

  const tone =
    health.severity === 'error' ? SEVERITY_CLASS.error : SEVERITY_CLASS.warn
  const showAction =
    onAction && health.action !== null && health.action !== 'edit'

  return (
    <div
      role="status"
      data-account-health={health.category ?? undefined}
      className={`flex ${compact ? 'items-center' : 'items-start'} gap-2 rounded-md border px-2.5 py-1.5 text-[12px] ${tone}`}
    >
      <AlertTriangle
        size={14}
        strokeWidth={1.8}
        className="mt-px shrink-0"
        aria-hidden
      />
      <span className="min-w-0 flex-1">{health.message}</span>
      {showAction && (
        <Button
          size="sm"
          variant="outline"
          type="button"
          className="h-6 shrink-0 gap-1 rounded-[5px] px-2 text-[11px]"
          disabled={isActionPending}
          onClick={(event) => {
            event.stopPropagation()
            onAction?.(account)
          }}
        >
          {health.action === 'reconnect' ? (
            <RotateCcw size={12} strokeWidth={1.8} />
          ) : (
            <RefreshCw size={12} strokeWidth={1.8} />
          )}
          {health.actionLabel}
        </Button>
      )}
    </div>
  )
}
