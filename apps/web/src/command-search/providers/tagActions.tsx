import { Tag } from 'lucide-react'

import { matchesQuery } from '../match'
import type { CommandPaletteEntry, SearchProvider } from '../types'
import { candidateFromEntry } from './shared'

/**
 * Selection-scoped tag action for the command palette: a single "Tag" command
 * that opens the tag editor (the same panel the `t` key opens) for the
 * selected message. Replaces the previous per-tag "Tag message with …" /
 * "Remove tag … from message" entries, which were spammy once a mailbox had
 * more than a couple of tags — the tag editor itself is the scrollable,
 * colored place to add/remove tags now. Emits nothing without a selected
 * message, so it respects `hasSelectedMessage` like the other selection
 * actions.
 *
 * Candidates use the `command` vertical so they land in the Commands section
 * alongside the fixed command list — no parallel palette mechanism.
 */
export function createTagActionProvider(): SearchProvider {
  const provider: SearchProvider = {
    id: 'tag-actions',
    label: 'Tags',
    vertical: 'command',
    async search(req) {
      if (!req.context.app.hasSelectedMessage) {
        return { candidates: [], nextCursor: null }
      }

      const entry: CommandPaletteEntry = {
        id: 'open-tag-editor',
        kind: 'command',
        label: 'Tag',
        keywords: 'tag add remove label message',
        icon: (
          <Tag size={15} strokeWidth={1.7} className="text-muted-foreground" />
        ),
        action: { kind: 'open-tag-editor' },
      }

      if (!matchesQuery(req.query, entry.label, entry.keywords)) {
        return { candidates: [], nextCursor: null }
      }

      return {
        candidates: [candidateFromEntry(provider, entry, req.query, 0)],
        nextCursor: null,
      }
    },
  }
  return provider
}
