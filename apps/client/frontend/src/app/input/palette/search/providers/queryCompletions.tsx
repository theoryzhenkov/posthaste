import { CircleHelp, ListFilter } from 'lucide-react'

import type { MailboxNavigationReadModels } from '@/data/models/mailboxNavigation'
import { getQueryCompletions, getQueryHelpEntries } from '@/domain/search'

import type { CommandPaletteEntry, SearchProvider } from '../types'
import { candidateFromEntry } from './shared'

export function createQueryCompletionProvider(input: {
  readModels: Pick<MailboxNavigationReadModels, 'sources' | 'tags'>
}): SearchProvider {
  const queryCompletionProvider: SearchProvider = {
    id: 'query-completions',
    label: 'Query Language',
    vertical: 'query-completion',
    async search(req) {
      const completions = getQueryCompletions(req.query, {
        messages: [],
        sources: input.readModels.sources,
        tags: input.readModels.tags,
      }).map<CommandPaletteEntry>((completion) => ({
        id: completion.id,
        kind: 'query-completion',
        label: completion.label,
        subtitle: completion.detail,
        keywords: `${completion.label} ${completion.detail}`,
        icon: (
          <ListFilter
            size={15}
            strokeWidth={1.7}
            className="text-muted-foreground"
          />
        ),
        action: { kind: 'replace-query', query: completion.replacement },
        closeOnSelect: false,
      }))
      const help = getQueryHelpEntries(req.query).map<CommandPaletteEntry>(
        (entry) => ({
          id: entry.id,
          kind: 'query-completion',
          label: entry.label,
          subtitle: entry.detail,
          keywords: entry.keywords,
          icon: (
            <CircleHelp
              size={15}
              strokeWidth={1.7}
              className="text-muted-foreground"
            />
          ),
          action: { kind: 'replace-query', query: entry.replacement },
          closeOnSelect: false,
        }),
      )
      const entries = [...completions, ...help]
      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(
              queryCompletionProvider,
              entry,
              req.query,
              index,
            ),
          ),
        nextCursor: null,
      }
    },
  }
  return queryCompletionProvider
}
