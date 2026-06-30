import { useQuery } from '@tanstack/react-query'
import { useCallback } from 'react'

import type { AppSettings, TagAppearance } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'

/**
 * Reactive lookup of per-tag appearance overrides (`settings.tags`). Returns the
 * {@link TagAppearance} for a tag name, or `undefined` when the tag uses its
 * name-derived defaults. Reactive to the settings query, so an optimistic PATCH
 * recolors every chip immediately.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function useTagAppearanceLookup(): (
  name: string,
) => TagAppearance | undefined {
  const { data } = useQuery<AppSettings>({
    queryKey: queryKeys.settings,
    queryFn: runtimeViews.settings.current,
  })
  const tags = data?.tags
  return useCallback(
    (name: string) => tags?.find((entry) => entry.name === name),
    [tags],
  )
}
