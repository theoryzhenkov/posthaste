import { Tag, TagsIcon } from 'lucide-react'

import type { MailboxNavigationReadModels } from '@/mailboxNavigationReadModels'

import { matchesQuery } from '../match'
import type { CommandPaletteEntry, SearchProvider } from '../types'
import { candidateFromEntry } from './shared'

function normalizeTagName(value: string): string | null {
  const normalized = value.trim().replace(/\s+/g, ' ')
  if (!normalized || normalized.startsWith('$') || normalized.includes('/')) {
    return null
  }
  return normalized
}

/**
 * Selection-scoped tag actions for the command palette: "Tag message with …"
 * over known tags (plus create-new from the query) and "Remove tag … from
 * message" over the selection's current tags. Emits nothing without a selected
 * message, so it respects `hasSelectedMessage` like the other selection actions.
 *
 * Candidates use the `command` vertical so they land in the Commands section
 * alongside the fixed command list — no parallel palette mechanism.
 */
export function createTagActionProvider(input: {
  readModels: Pick<MailboxNavigationReadModels, 'tags'>
  /** User tags currently on the selected message (system `$keywords` excluded). */
  selectedMessageTags: readonly string[]
}): SearchProvider {
  const provider: SearchProvider = {
    id: 'tag-actions',
    label: 'Tags',
    vertical: 'command',
    async search(req) {
      if (!req.context.app.hasSelectedMessage) {
        return { candidates: [], nextCursor: null }
      }

      const current = new Set(
        input.selectedMessageTags.map((tag) => tag.toLowerCase()),
      )
      const entries: CommandPaletteEntry[] = []

      // Remove: only tags actually on the selection.
      for (const tag of input.selectedMessageTags) {
        entries.push({
          id: `remove:${tag}`,
          kind: 'command',
          label: `Remove tag "${tag}" from message`,
          keywords: `remove tag ${tag} untag`,
          icon: (
            <Tag
              size={15}
              strokeWidth={1.7}
              className="text-muted-foreground"
            />
          ),
          action: { kind: 'remove-tag-from-message', tag },
        })
      }

      // Add: known tags not already on the selection.
      for (const tag of input.readModels.tags) {
        if (current.has(tag.name.toLowerCase())) continue
        entries.push({
          id: `add:${tag.name}`,
          kind: 'command',
          label: `Tag message with "${tag.name}"`,
          keywords: `tag message with ${tag.name} add label`,
          icon: (
            <Tag
              size={15}
              strokeWidth={1.7}
              className="text-muted-foreground"
            />
          ),
          action: { kind: 'add-tag-to-message', tag: tag.name },
        })
      }

      // Create-new: a typed name that is neither already applied nor a known tag.
      const typed = normalizeTagName(req.query)
      if (typed) {
        const typedKey = typed.toLowerCase()
        const known = input.readModels.tags.some(
          (tag) => tag.name.toLowerCase() === typedKey,
        )
        if (!known && !current.has(typedKey)) {
          entries.push({
            id: `create:${typed}`,
            kind: 'command',
            label: `Tag message with "${typed}" (new tag)`,
            keywords: `tag message with ${typed} add new create label`,
            icon: (
              <TagsIcon
                size={15}
                strokeWidth={1.7}
                className="text-muted-foreground"
              />
            ),
            action: { kind: 'add-tag-to-message', tag: typed },
          })
        }
      }

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
    },
  }
  return provider
}
