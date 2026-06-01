import type {
  PaletteRow,
  ProviderState,
  RankingContext,
  SearchCandidate,
  SearchVertical,
} from './types'

type RankTier = 1 | 2 | 3

interface RankedCandidate {
  candidate: SearchCandidate
  tier: RankTier
  matchScore: number
  contextScore: number
  verticalScore: number
}

const VERTICAL_PRIOR: Record<SearchVertical, number> = {
  command: 80,
  'query-completion': 78,
  message: 76,
  mailbox: 70,
  tag: 62,
  contact: 58,
}

const SECTION_ORDER: Array<{ vertical: SearchVertical; label: string }> = [
  { vertical: 'message', label: 'Messages' },
  { vertical: 'contact', label: 'Contacts' },
  { vertical: 'mailbox', label: 'Mailboxes' },
  { vertical: 'tag', label: 'Tags' },
  { vertical: 'command', label: 'Commands' },
  { vertical: 'query-completion', label: 'Query Language' },
]

function primaryMatchKind(candidate: SearchCandidate): string | null {
  return candidate.match.fields[0]?.kind ?? null
}

function tierFor(candidate: SearchCandidate, query: string): RankTier {
  const matchKind = primaryMatchKind(candidate)
  if (matchKind === 'exact' || matchKind === 'prefix') {
    return 1
  }
  if (!query.trim() || matchKind === null) {
    return 2
  }
  if (
    matchKind === 'acronym' ||
    matchKind === 'contains' ||
    matchKind === 'fuzzy'
  ) {
    return 3
  }
  return 2
}

function matchScore(candidate: SearchCandidate): number {
  switch (primaryMatchKind(candidate)) {
    case 'exact':
      return 100
    case 'prefix':
      return 90
    case 'acronym':
      return 75
    case 'contains':
      return 60
    case 'fuzzy':
      return 40
    case 'fts':
      return 65
    default:
      return 0
  }
}

function counterValue(
  entries: Record<string, { value: number; updatedAt: number }>,
  id: string,
): number {
  return entries[id]?.value ?? 0
}

function contextScore(
  candidate: SearchCandidate,
  context: RankingContext,
): number {
  let score = 0
  if (candidate.vertical === 'command') {
    score +=
      counterValue(context.user.recentCommands.entries, candidate.entry.id) * 8
    score +=
      counterValue(context.user.frequentCommands.entries, candidate.entry.id) *
      4
    if (context.user.pinnedCommands.includes(candidate.entry.id)) {
      score += 25
    }
    if (
      context.app.hasSelectedMessage &&
      ['reply', 'archive', 'flag'].includes(candidate.entry.id)
    ) {
      score += 16
    }
  }
  if (candidate.vertical === 'mailbox') {
    score +=
      counterValue(context.user.frequentMailboxes.entries, candidate.entry.id) *
      5
    if (context.user.pinnedMailboxes.includes(candidate.entry.id)) {
      score += 20
    }
    if (candidate.entry.action.kind === 'open-source-mailbox') {
      if (candidate.entry.action.sourceId === context.app.accountId) {
        score += 6
      }
      if (candidate.entry.action.mailboxId === context.app.mailboxId) {
        score += 10
      }
    }
  }
  if (
    candidate.vertical === 'message' &&
    candidate.entry.action.kind === 'open-message'
  ) {
    if (candidate.entry.action.messageId === context.app.selectedMessageId) {
      score += 18
    }
  }
  score +=
    counterValue(context.user.recentEntities.entries, candidate.entry.id) * 4
  return score
}

function rankCandidate(
  candidate: SearchCandidate,
  query: string,
  context: RankingContext,
): RankedCandidate {
  return {
    candidate,
    tier: tierFor(candidate, query),
    matchScore: matchScore(candidate),
    contextScore: contextScore(candidate, context),
    verticalScore: VERTICAL_PRIOR[candidate.vertical],
  }
}

export function rankCandidates(
  candidates: SearchCandidate[],
  query: string,
  context: RankingContext,
): SearchCandidate[] {
  return candidates
    .map((candidate) => rankCandidate(candidate, query, context))
    .sort((left, right) => {
      if (left.tier !== right.tier) return left.tier - right.tier
      if (left.matchScore !== right.matchScore)
        return right.matchScore - left.matchScore
      if (left.tier === 2 && left.verticalScore !== right.verticalScore) {
        return right.verticalScore - left.verticalScore
      }
      if (left.contextScore !== right.contextScore)
        return right.contextScore - left.contextScore
      if (left.verticalScore !== right.verticalScore)
        return right.verticalScore - left.verticalScore
      if (left.candidate.providerRank !== right.candidate.providerRank) {
        return left.candidate.providerRank - right.candidate.providerRank
      }
      return left.candidate.id.localeCompare(right.candidate.id)
    })
    .map((ranked) => ranked.candidate)
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'Provider failed'
}

function verticalForProviderId(providerId: string): SearchVertical | null {
  switch (providerId) {
    case 'commands':
      return 'command'
    case 'query-completions':
      return 'query-completion'
    case 'mailboxes':
      return 'mailbox'
    case 'tags':
      return 'tag'
    case 'contacts':
      return 'contact'
    case 'messages':
      return 'message'
    default:
      return null
  }
}

export function buildPaletteRows(input: {
  query: string
  context: RankingContext
  providerStates: Map<string, ProviderState>
  frozenBestMatchIds: string[] | null
}): { rows: PaletteRow[]; bestMatchIds: string[] } {
  const candidates = [...input.providerStates.values()].flatMap(
    (state) => state.candidates,
  )
  const ranked = rankCandidates(candidates, input.query, input.context)
  const computedBestMatchIds = ranked
    .slice(0, 8)
    .map((candidate) => candidate.id)
  const bestMatchIds = input.frozenBestMatchIds ?? computedBestMatchIds
  const candidateById = new Map(
    candidates.map((candidate) => [candidate.id, candidate]),
  )
  const dedupedIds = new Set(bestMatchIds)
  const rows: PaletteRow[] = []

  const bestMatches = bestMatchIds
    .map((id) => candidateById.get(id))
    .filter((candidate): candidate is SearchCandidate => Boolean(candidate))
  if (bestMatches.length > 0) {
    rows.push({ kind: 'section', id: 'section:best', label: 'Best matches' })
    for (const candidate of bestMatches) {
      rows.push({ kind: 'item', id: `best:${candidate.id}`, candidate })
    }
  }

  for (const section of SECTION_ORDER) {
    const sectionCandidates = ranked.filter(
      (candidate) =>
        candidate.vertical === section.vertical &&
        !dedupedIds.has(candidate.id),
    )
    const providerStates = [...input.providerStates.entries()].filter(
      ([providerId, state]) =>
        verticalForProviderId(providerId) === section.vertical ||
        state.candidates.some(
          (candidate) => candidate.vertical === section.vertical,
        ),
    )
    const hasLoading = providerStates.some(
      ([, state]) => state.status === 'loading',
    )
    const errors = providerStates.filter(
      ([, state]) => state.status === 'error',
    )

    if (sectionCandidates.length === 0 && !hasLoading && errors.length === 0) {
      continue
    }

    rows.push({
      kind: 'section',
      id: `section:${section.vertical}`,
      label: section.label,
    })
    for (const candidate of sectionCandidates) {
      rows.push({ kind: 'item', id: `item:${candidate.id}`, candidate })
    }
    for (const [providerId, state] of errors) {
      rows.push({
        kind: 'error',
        id: `error:${providerId}`,
        providerId,
        label: section.label,
        message: errorMessage(state.error),
      })
    }
    if (hasLoading) {
      const providerId = providerStates.find(
        ([, state]) => state.status === 'loading',
      )?.[0]
      if (providerId) {
        rows.push({
          kind: 'loading',
          id: `loading:${providerId}`,
          providerId,
          label: `Loading ${section.label.toLowerCase()}…`,
        })
      }
    }
  }

  return { rows, bestMatchIds: computedBestMatchIds }
}
