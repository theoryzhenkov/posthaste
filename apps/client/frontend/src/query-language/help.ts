import {
  HELP_ENTRIES,
  PREFIX_BY_NAME,
  type QueryHelpEntry,
} from '../queryDefinitions'
import { findActivePrefix } from './completions'
import { normalize } from './helpers'

export function getQueryHelpEntries(input: string): QueryHelpEntry[] {
  const normalized = normalize(input)
  if (!normalized || normalized === '?' || normalized === 'help') {
    return HELP_ENTRIES.slice(0, 8)
  }

  const helpMode =
    normalized.includes('help') ||
    normalized.includes('query') ||
    normalized.includes('filter')

  if (!helpMode) {
    const activePrefix = findActivePrefix(input)
    if (!activePrefix) {
      return []
    }
    const definition = PREFIX_BY_NAME.get(activePrefix.name)
    return HELP_ENTRIES.filter((entry) =>
      definition
        ? entry.label === `${definition.primary}:`
        : entry.keywords.includes(activePrefix.name),
    )
  }

  const terms = normalized
    .split(/\s+/)
    .filter((term) => !['help', 'query', 'filter', 'search'].includes(term))
  if (terms.length === 0) {
    return HELP_ENTRIES
  }
  return HELP_ENTRIES.filter((entry) =>
    terms.every((term) => entry.keywords.includes(term)),
  )
}
