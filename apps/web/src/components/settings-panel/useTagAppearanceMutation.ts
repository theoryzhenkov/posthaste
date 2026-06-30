import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { AppSettings, TagAppearance } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'

/**
 * Per-tag appearance mutation. Takes the full next `tags` array (the caller
 * upserts/removes its entry) and PATCHes it to settings, applying the change to
 * the settings cache optimistically (instant chip recolor) and rolling back on
 * error. Presentation-only — no provider round-trip.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function useTagAppearanceMutation() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (tags: TagAppearance[]) =>
      runtimeMutations.settings.patch({ tags }),
    onMutate: async (tags) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.settings })
      const previous = queryClient.getQueryData<AppSettings>(queryKeys.settings)
      queryClient.setQueryData<AppSettings>(queryKeys.settings, (old) =>
        old ? { ...old, tags } : old,
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
