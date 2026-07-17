import type { MailboxNavigationReadModels } from '@/mailboxNavigationReadModels'
import { renderMailboxRoleIcon, renderSmartMailboxIcon } from '@/mailboxRoles'

import { matchesQuery } from '../match'
import type { CommandPaletteEntry, SearchProvider } from '../types'
import { candidateFromEntry } from './shared'

export function createMailboxProvider(input: {
  readModels: Pick<MailboxNavigationReadModels, 'smartMailboxes' | 'sources'>
}): SearchProvider {
  const mailboxProvider: SearchProvider = {
    id: 'mailboxes',
    label: 'Mailboxes',
    vertical: 'mailbox',
    async search(req) {
      const entries: CommandPaletteEntry[] = []
      for (const smartMailbox of input.readModels.smartMailboxes) {
        if (!matchesQuery(req.query, smartMailbox.name)) continue
        entries.push({
          id: `smart:${smartMailbox.id}`,
          kind: 'mailbox',
          label: smartMailbox.name,
          subtitle: 'Smart mailbox',
          keywords: smartMailbox.name,
          icon: renderSmartMailboxIcon(
            smartMailbox.role,
            smartMailbox.defaultKey,
            15,
          ),
          action: {
            kind: 'open-smart-mailbox',
            smartMailboxId: smartMailbox.id,
            name: smartMailbox.name,
          },
        })
      }
      for (const source of input.readModels.sources) {
        for (const mailbox of source.mailboxes) {
          const haystack = `${mailbox.name} ${source.name} ${mailbox.role ?? ''}`
          if (!matchesQuery(req.query, mailbox.name, haystack)) continue
          entries.push({
            id: `${source.id}:${mailbox.id}`,
            kind: 'mailbox',
            label: mailbox.name,
            subtitle: source.name,
            keywords: haystack,
            icon: renderMailboxRoleIcon(mailbox.role, 15),
            action: {
              kind: 'open-source-mailbox',
              sourceId: source.id,
              mailboxId: mailbox.id,
              name: `${source.name} / ${mailbox.name}`,
            },
          })
        }
      }
      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(mailboxProvider, entry, req.query, index),
          ),
        nextCursor: null,
      }
    },
  }
  return mailboxProvider
}
