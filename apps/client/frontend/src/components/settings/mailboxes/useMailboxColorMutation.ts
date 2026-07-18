import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { MailboxColor } from '@/gen'
import { useMailClient } from '@/data/context'
import { ensureAppSettings } from '@/data/queries/queries'
import { runCommand } from '@/data/transport/commands'

/**
 * Per-mailbox sidebar color mutation. Takes the full next `mailboxColors`
 * array (the caller upserts/removes its entry) and writes the settings
 * document whole through the `updateSettings` command — acceptance
 * invalidates every query, so the sidebar recolors when the answer catches
 * up. Presentation-only — no provider round-trip.
 */
export function useMailboxColorMutation() {
  const client = useMailClient()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (mailboxColors: MailboxColor[]) => {
      const settings = await ensureAppSettings(client, queryClient)
      return runCommand(client, queryClient, {
        updateSettings: {
          settings: { ...settings, mailboxColors },
          forceBackfill: false,
        },
      })
    },
  })
}
