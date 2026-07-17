import { useCallback } from 'react'

import { useAppSettings } from '@/data'

/**
 * A reactive lookup of the per-mailbox sidebar color overrides
 * (`settings.mailboxColors` on the `appSettings` family). Returns the
 * override hue for a `(sourceId, mailboxId)` pair, or `undefined` when the
 * mailbox uses its default (role-derived) color. Reactive to the settings
 * query, so a saved settings change re-colors the sidebar when the answer
 * catches up.
 */
export function useMailboxColorLookup(): (
  sourceId: string,
  mailboxId: string,
) => number | undefined {
  const { data } = useAppSettings()
  const colors = data?.settings.mailboxColors
  return useCallback(
    (sourceId: string, mailboxId: string) =>
      colors?.find(
        (entry) => entry.sourceId === sourceId && entry.mailboxId === mailboxId,
      )?.hue,
    [colors],
  )
}
