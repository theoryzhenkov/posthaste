import { useCallback } from 'react'

import type { TagAppearance } from '@/gen'
import { useAppSettings } from '@/data'

/**
 * Reactive lookup of per-tag appearance overrides (`settings.tags` on the
 * `appSettings` family). Returns the {@link TagAppearance} for a tag name, or
 * `undefined` when the tag uses its name-derived defaults. Reactive to the
 * settings query, so a saved settings change recolors every chip when the
 * answer catches up.
 */
export function useTagAppearanceLookup(): (
  name: string,
) => TagAppearance | undefined {
  const { data } = useAppSettings()
  const tags = data?.settings.tags
  return useCallback(
    (name: string) => tags?.find((entry) => entry.name === name),
    [tags],
  )
}
