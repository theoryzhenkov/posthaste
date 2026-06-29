import { useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'

import type { AppSettings } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'

/**
 * A reactive lookup of the per-mailbox sidebar color overrides
 * (`settings.mailboxColors`). Returns the override hue for a
 * `(sourceId, mailboxId)` pair, or `undefined` when the mailbox uses its
 * default (role-derived) color. Reactive to the settings query, so an
 * optimistic PATCH re-colors the sidebar immediately.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function useMailboxColorLookup(): (
  sourceId: string,
  mailboxId: string,
) => number | undefined {
  const { data } = useQuery<AppSettings>({
    queryKey: queryKeys.settings,
    queryFn: runtimeViews.settings.current,
  })
  const colors = data?.mailboxColors
  return useCallback(
    (sourceId: string, mailboxId: string) =>
      colors?.find(
        (entry) => entry.sourceId === sourceId && entry.mailboxId === mailboxId,
      )?.hue,
    [colors],
  )
}
