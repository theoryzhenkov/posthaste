export type { QueryHelpEntry, QueryPrefixDefinition } from './queryDefinitions'
export { getQueryCompletions } from './query-language/completions'
export { getQueryHelpEntries } from './query-language/help'
export { validateSearchQuery } from './query-language/validation'
export type {
  QueryCompletion,
  QueryCompletionContext,
  QueryCompletionSource,
  QueryValidation,
} from './query-language/types'
