import { describe, expect, it } from 'bun:test'

import { buildPaletteRows, rankCandidates } from '../src/command-search/ranker'
import type {
  ProviderState,
  RankingContext,
  SearchCandidate,
  SearchVertical,
} from '../src/command-search/types'

function context(): RankingContext {
  const counter = { halfLifeMs: 1000, entries: {} }
  return {
    now: 1,
    app: { route: 'mailbox', hasSelectedMessage: false },
    session: { paletteOpenReason: 'keyboard' },
    user: {
      recentCommands: counter,
      recentEntities: counter,
      frequentCommands: counter,
      frequentMailboxes: counter,
      frequentContacts: counter,
      pinnedCommands: [],
      pinnedMailboxes: [],
    },
  }
}

function candidate(input: {
  id: string
  vertical: SearchVertical
  providerId?: string
  label: string
  matchKind?: 'exact' | 'prefix' | 'contains' | 'fuzzy'
  providerRank?: number
}): SearchCandidate {
  return {
    id: input.id,
    providerId: input.providerId ?? `${input.vertical}s`,
    vertical: input.vertical,
    entry: {
      id: input.id,
      kind: input.vertical,
      label: input.label,
      keywords: input.label,
      action: { kind: 'noop', label: input.label },
    },
    providerRank: input.providerRank ?? 0,
    match: {
      query: 'arc',
      fields: input.matchKind
        ? [{ field: 'label', kind: input.matchKind }]
        : [],
    },
    features: {},
  }
}

function state(candidates: SearchCandidate[]): ProviderState {
  return { status: 'done', candidates, nextCursor: null }
}

describe('command search ranker', () => {
  it('orders strong explicit matches ahead of contextual vertical priors', () => {
    const ranked = rankCandidates(
      [
        candidate({
          id: 'message:1',
          vertical: 'message',
          label: 'Quarterly report',
          matchKind: 'contains',
        }),
        candidate({
          id: 'command:archive',
          vertical: 'command',
          label: 'Archive selected',
          matchKind: 'prefix',
        }),
      ],
      'arc',
      context(),
    )

    expect(ranked.map((item) => item.id)).toEqual([
      'command:archive',
      'message:1',
    ])
  })

  it('deduplicates best matches from vertical sections', () => {
    const message = candidate({
      id: 'messages:1',
      vertical: 'message',
      providerId: 'messages',
      label: 'Architecture notes',
      matchKind: 'prefix',
    })
    const command = candidate({
      id: 'commands:archive',
      vertical: 'command',
      providerId: 'commands',
      label: 'Archive selected',
      matchKind: 'prefix',
    })

    const rows = buildPaletteRows({
      query: 'arc',
      context: context(),
      providerStates: new Map([
        ['messages', state([message])],
        ['commands', state([command])],
      ]),
      frozenBestMatchIds: null,
    }).rows

    const itemIds = rows
      .filter((row) => row.kind === 'item')
      .map((row) => row.candidate.id)

    expect(itemIds).toEqual(['commands:archive', 'messages:1'])
  })

  it('keeps frozen best matches stable when later candidates arrive', () => {
    const originalBest = candidate({
      id: 'commands:archive',
      vertical: 'command',
      providerId: 'commands',
      label: 'Archive selected',
      matchKind: 'prefix',
    })
    const lateBetterMatch = candidate({
      id: 'messages:archive',
      vertical: 'message',
      providerId: 'messages',
      label: 'Archive',
      matchKind: 'exact',
    })

    const rows = buildPaletteRows({
      query: 'arc',
      context: context(),
      providerStates: new Map([
        ['commands', state([originalBest])],
        ['messages', state([lateBetterMatch])],
      ]),
      frozenBestMatchIds: [originalBest.id],
    }).rows

    const bestSectionIndex = rows.findIndex(
      (row) => row.kind === 'section' && row.id === 'section:best',
    )
    const firstBestItem = rows[bestSectionIndex + 1]

    expect(firstBestItem).toMatchObject({
      kind: 'item',
      candidate: { id: originalBest.id },
    })
  })
})
