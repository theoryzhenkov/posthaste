import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { AppSettings, MailboxColor } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'

/**
 * Per-mailbox sidebar color mutation. Takes the full next `mailboxColors` array
 * (the caller upserts/removes its entry) and PATCHes it to settings, applying
 * the change to the settings cache optimistically (instant sidebar recolor) +
 * rolling back on error. Presentation-only — no provider round-trip.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function useMailboxColorMutation() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (mailboxColors: MailboxColor[]) =>
      runtimeMutations.settings.patch({ mailboxColors }),
    onMutate: async (mailboxColors) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.settings })
      const previous = queryClient.getQueryData<AppSettings>(queryKeys.settings)
      queryClient.setQueryData<AppSettings>(queryKeys.settings, (old) =>
        old ? { ...old, mailboxColors } : old,
      )
      return { previous }
    },
    onError: (_error, _next, context) => {
      if (context?.previous) {
        queryClient.setQueryData(queryKeys.settings, context.previous)
      }
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(queryKeys.settings, saved)
    },
  })
}
