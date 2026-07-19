import { matchEvidence, matchesQuery, textMatch } from '../match'
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

/** The providers' shared tail: filter entries against the query, cap to the
 *  request's limit, and wrap each survivor as a ranked candidate page. */
export function matchedCandidatePage(
  provider: SearchProvider,
  entries: CommandPaletteEntry[],
  req: { query: string; limit: number },
): { candidates: SearchCandidate[]; nextCursor: null } {
  const matched = entries.filter((entry) =>
    matchesQuery(req.query, entry.label, entry.keywords),
  )
  return {
    candidates: matched
      .slice(0, req.limit)
      .map((entry, index) =>
        candidateFromEntry(provider, entry, req.query, index),
      ),
    nextCursor: null,
  }
}
