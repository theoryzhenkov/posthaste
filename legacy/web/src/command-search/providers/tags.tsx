import { Tag } from 'lucide-react'

import type { MailboxNavigationReadModels } from '@/mailboxNavigationReadModels'

import { matchesQuery } from '../match'
import type { CommandPaletteEntry, SearchProvider } from '../types'
import { candidateFromEntry } from './shared'

export function createTagProvider(input: {
  readModels: Pick<MailboxNavigationReadModels, 'tags'>
}): SearchProvider {
  const tagProvider: SearchProvider = {
    id: 'tags',
    label: 'Tags',
    vertical: 'tag',
    async search(req) {
      const entries = input.readModels.tags
        .filter((tag) => matchesQuery(req.query, tag.name))
        .map<CommandPaletteEntry>((tag) => ({
          id: tag.name,
          kind: 'tag',
          label: tag.name,
          subtitle: `${tag.totalMessages} messages`,
          keywords: tag.name,
          icon: (
            <Tag
              size={15}
              strokeWidth={1.7}
              className="text-muted-foreground"
            />
          ),
          action: { kind: 'apply-query', query: `tag:${tag.name}` },
        }))
      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(tagProvider, entry, req.query, index),
          ),
        nextCursor: null,
      }
    },
  }
  return tagProvider
}
