/**
 * SUGGESTION SOURCES for the type-directed condition value widgets — the data
 * half of the value-widget registry. A suggestion source is a capability of a
 * VALUE TYPE (address → the persistent address book; keyword → the live tag
 * list), so it composes with every operator arity: the scalar widget and the
 * `in` list-entry widget consume the same hook and can never drift.
 *
 * Both feed from ordinary query families (`senderAddresses`, `tags`), so the
 * pickers stay fresh through the same generation-advance invalidation as
 * every other read.
 */
import { useMemo } from 'react'

import {
  buildAddressBookSuggestionOptions,
  type AddressSuggestionOption,
} from '@/domain/addressSuggestions'
import { useSenderAddresses, useTags } from '@/data'

/**
 * The compose-shared address book (`senderAddresses` — every correspondent
 * harvested from ingest + send), as suggestion options for address fields
 * (`fromEmail` / `fromName` / `to`).
 */
export function useAddressBookSuggestions(): AddressSuggestionOption[] {
  const addressBook = useSenderAddresses()
  const rows = addressBook.data?.rows
  return useMemo(() => buildAddressBookSuggestionOptions(rows ?? []), [rows])
}

/**
 * The live tag names across enabled accounts, for `keyword` fields and the
 * tag action input.
 */
export function useKeywordSuggestions(): string[] {
  const tags = useTags()
  const rows = tags.data?.rows
  return useMemo(() => (rows ?? []).map((tag) => tag.name), [rows])
}
