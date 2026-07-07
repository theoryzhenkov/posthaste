/**
 * SUGGESTION SOURCES for the type-directed condition value widgets — the data
 * half of the value-widget registry. A suggestion source is a capability of a
 * VALUE TYPE (address → the persistent address book; keyword → the live tag
 * list), so it composes with every operator arity: the scalar widget and the
 * `in` list-entry widget consume the same hook and can never drift.
 *
 * Freshness (the "autocomplete often doesn't work" staleness root cause): both
 * queries use `refetchOnMount: 'always'`. The settings SURFACE window runs
 * WITHOUT the live event bridge (`App.tsx` gates `DaemonEventBridge` off
 * standalone surfaces), so nothing event-invalidates these caches there — and
 * the address book is backfilled server-side by a deferred post-startup task,
 * so a query cached before the backfill would otherwise pin an EMPTY book for
 * the session. Refetching on mount costs one small REST call per editor open
 * and guarantees the pickers see the current book/tags in every window.
 */
import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'

import {
  buildAddressBookSuggestionOptions,
  type AddressSuggestionOption,
} from '@/composeAddressSuggestions'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'
import type { TagSummary } from '../../../api/types'

/**
 * The compose-shared address book (`senderAddresses` — every correspondent
 * harvested from ingest + send), as suggestion options for address fields
 * (`fromEmail` / `fromName` / `to`).
 */
export function useAddressBookSuggestions(): AddressSuggestionOption[] {
  const addressBook = useQuery({
    queryKey: queryKeys.senderAddresses,
    queryFn: runtimeViews.compose.senderAddresses,
    refetchOnMount: 'always',
  })
  return useMemo(
    () => buildAddressBookSuggestionOptions(addressBook.data ?? []),
    [addressBook.data],
  )
}

/**
 * The live tag names across enabled accounts, for `keyword` fields and the tag
 * action input. Fetched through the batch `/v1/read` surface (`Tag/list`) —
 * the main window normally hydrates `queryKeys.tags` from its navigation
 * bootstrap, but a settings surface window has no such bootstrap, so this
 * query must be able to fetch on its own.
 */
export function useKeywordSuggestions(): string[] {
  const tags = useQuery<TagSummary[]>({
    queryKey: queryKeys.tags,
    queryFn: async () => {
      const response = await runtimeViews.mail.read({
        calls: [{ id: 'tags', op: 'Tag/list' }],
      })
      const result = response.results.tags
      return result?.op === 'Tag/list' ? result.value.items : []
    },
    refetchOnMount: 'always',
  })
  return useMemo(() => (tags.data ?? []).map((tag) => tag.name), [tags.data])
}
