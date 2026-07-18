import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { TagAppearance } from '@/gen'
import { useMailClient } from '@/data/context'
import { ensureAppSettings } from '@/data/queries/queries'
import { runCommand } from '@/data/transport/commands'

/**
 * Per-tag appearance mutation. Takes the full next `tags` array (the caller
 * upserts/removes its entry) and writes the settings document whole through
 * the `updateSettings` command — acceptance invalidates every query, so
 * chips recolor when the answer catches up. Presentation-only — no provider
 * round-trip.
 */
export function useTagAppearanceMutation() {
  const client = useMailClient()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (tags: TagAppearance[]) => {
      const settings = await ensureAppSettings(client, queryClient)
      return runCommand(client, queryClient, {
        updateSettings: {
          settings: { ...settings, tags },
          forceBackfill: false,
        },
      })
    },
  })
}
