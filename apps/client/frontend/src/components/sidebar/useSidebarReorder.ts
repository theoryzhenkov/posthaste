/**
 * Drag-to-reorder persistence for the sidebar (and settings) lists. The new
 * order is written to `AppSettings` (`smartMailboxOrder` / `accountOrder`)
 * through the `updateSettings` command — the single source of truth the
 * backend re-resolves list order from. Acceptance invalidates every query,
 * so the reordered lists re-render from the backend's answer.
 *
 * @spec docs/L1-accounts#sidebar-ordering
 */
import { useQueryClient } from '@tanstack/react-query'
import { useCallback } from 'react'
import { toast } from 'sonner'

import { useMailClient } from '@/data/context'
import { runCommand } from '@/data/commands'
import { ensureAppSettings } from '@/data/queries'
import type { AppSettings } from '@/gen'

export function useSidebarReorder() {
  const client = useMailClient()
  const queryClient = useQueryClient()

  const persist = useCallback(
    (change: Partial<Pick<AppSettings, 'smartMailboxOrder' | 'accountOrder'>>) => {
      void (async () => {
        const settings = await ensureAppSettings(client, queryClient)
        await runCommand(client, queryClient, {
          updateSettings: {
            settings: { ...settings, ...change },
            forceBackfill: false,
          },
        })
      })().catch(() => {
        toast.error("Couldn't save the new order. Please try again.")
      })
    },
    [client, queryClient],
  )

  const reorderSmartMailboxes = useCallback(
    (orderedIds: string[]) => persist({ smartMailboxOrder: orderedIds }),
    [persist],
  )

  const reorderAccounts = useCallback(
    (orderedIds: string[]) => persist({ accountOrder: orderedIds }),
    [persist],
  )

  return { reorderSmartMailboxes, reorderAccounts }
}
