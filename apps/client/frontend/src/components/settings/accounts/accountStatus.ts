import type { AccountStatus } from '@/gen'

const STATUS_LABELS: Record<AccountStatus, string> = {
  ready: 'Ready',
  syncing: 'Syncing',
  degraded: 'Degraded',
  authError: 'Authentication error',
  offline: 'Offline',
  disabled: 'Disabled',
}

/** Human-readable label for an account status (avoids showing raw enum text). */
export function statusLabel(status: AccountStatus): string {
  return STATUS_LABELS[status]
}
