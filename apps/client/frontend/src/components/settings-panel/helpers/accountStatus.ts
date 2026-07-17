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

/** Map account status to Tailwind color classes for the status badge. */
export function statusTone(status: AccountStatus): string {
  switch (status) {
    case 'ready':
      return 'text-emerald-700 border-emerald-500/30 bg-emerald-500/10'
    case 'syncing':
      return 'text-blue-700 border-blue-500/30 bg-blue-500/10'
    case 'degraded':
      return 'text-amber-700 border-amber-500/30 bg-amber-500/10'
    case 'authError':
      return 'text-rose-700 border-rose-500/30 bg-rose-500/10'
    case 'offline':
      return 'text-orange-700 border-orange-500/30 bg-orange-500/10'
    case 'disabled':
      return 'text-zinc-600 border-zinc-500/30 bg-zinc-500/10'
  }
}
