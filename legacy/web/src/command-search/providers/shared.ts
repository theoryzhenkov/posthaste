import { matchEvidence, textMatch } from '../match'
import type {
  CommandPaletteEntry,
  SearchCandidate,
  SearchProvider,
} from '../types'

export function candidateFromEntry(
  provider: SearchProvider,
  entry: CommandPaletteEntry,
  query: string,
  providerRank: number,
): SearchCandidate {
  const match = textMatch(
    query,
    entry.label,
    `${entry.subtitle ?? ''} ${entry.keywords}`,
  )
  return {
    id: `${provider.id}:${entry.id}`,
    providerId: provider.id,
    vertical: provider.vertical,
    entry,
    providerRank,
    match: matchEvidence(query, 'label', match),
    features: {
      matchKind: match.kind,
      matchScore: match.score,
      vertical: provider.vertical,
    },
  }
}
