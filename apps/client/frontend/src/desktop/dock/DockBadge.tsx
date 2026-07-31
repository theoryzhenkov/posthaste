/**
 * App-root Dock/taskbar unread-badge driver. Sums inbox unread across enabled
 * accounts from the `accounts` + `mailboxCounts` families and pushes it to
 * the app icon (see useDockBadge.ts for the count selection + Tauri sink).
 * Renders nothing; mount in the one window holding the shared-OS-surface
 * claim (lib/platform/sharedOsSurfaces.ts) — the badge is a single app-wide
 * counter, so a second driver would fight the first.
 */
import { useMemo, type ReactNode } from 'react'

import { useAccounts, useMailboxCounts } from '@/data'

import { inboxUnreadTotal, useDockBadge } from './useDockBadge'

/**
 * App-root badge driver. Both queries are kept fresh by the global
 * generation-advance invalidation, so the badge tracks the same numbers the
 * sidebar shows.
 */
export function DockBadge(): ReactNode {
  const accountsQuery = useAccounts()
  const countsQuery = useMailboxCounts()

  const accountRows = accountsQuery.data?.rows
  const enabledAccountIds = useMemo(
    () =>
      new Set(
        (accountRows ?? [])
          .filter((account) => account.enabled)
          .map((account) => account.id),
      ),
    [accountRows],
  )

  const countRows = countsQuery.data?.rows
  const total = useMemo(
    () => inboxUnreadTotal(countRows ?? [], enabledAccountIds),
    [countRows, enabledAccountIds],
  )
  useDockBadge(total)

  return null
}
