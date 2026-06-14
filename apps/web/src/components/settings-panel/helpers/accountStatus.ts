import type { AccountOverview } from '../../../api/types'

/** Map account status to Tailwind color classes for the status badge. */
export function statusTone(status: AccountOverview['status']): string {
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

export function syncProgressLabel(account: AccountOverview): string | null {
  const progress = account.syncProgress
  if (account.status !== 'syncing' || !progress) {
    return null
  }

  const parts = [progress.detail]
  if (progress.mailboxName) {
    parts.push(progress.mailboxName)
  }
  if (progress.mailboxIndex && progress.mailboxCount) {
    parts.push(`${progress.mailboxIndex}/${progress.mailboxCount}`)
  }
  if (progress.messageCount !== null) {
    parts.push(`${progress.messageCount} messages`)
  }

  return parts.join(' · ')
}

export function syncProgressPercent(account: AccountOverview): number | null {
  const progress = account.syncProgress
  if (!progress?.mailboxIndex || !progress.mailboxCount) {
    return null
  }

  return Math.min(
    100,
    Math.max(
      0,
      Math.round((progress.mailboxIndex / progress.mailboxCount) * 100),
    ),
  )
}
