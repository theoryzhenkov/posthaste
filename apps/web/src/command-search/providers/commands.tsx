import {
  Archive,
  Clock3,
  Keyboard,
  PenSquare,
  Reply,
  Settings,
  SlidersHorizontal,
  Tag,
  UserPlus,
} from 'lucide-react'
import type { ReactNode } from 'react'

import { matchesQuery } from '../match'
import type {
  CommandActionId,
  CommandPaletteEntry,
  SearchProvider,
} from '../types'
import { candidateFromEntry } from './shared'

function commandIcon(id: CommandActionId): ReactNode {
  switch (id) {
    case 'compose':
      return (
        <PenSquare
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'reply':
      return (
        <Reply size={15} strokeWidth={1.7} className="text-muted-foreground" />
      )
    case 'archive':
      return (
        <Archive
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'flag':
      return (
        <Tag size={15} strokeWidth={1.7} className="text-muted-foreground" />
      )
    case 'snooze':
      return (
        <Clock3 size={15} strokeWidth={1.7} className="text-muted-foreground" />
      )
    case 'newSmart':
    case 'newRule':
      return (
        <SlidersHorizontal
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'settings':
      return (
        <Settings
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'shortcuts':
      return (
        <Keyboard
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'account':
      return (
        <UserPlus
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
  }
}

export function createCommandProvider(): SearchProvider {
  const commandProvider: SearchProvider = {
    id: 'commands',
    label: 'Commands',
    vertical: 'command',
    async search(req) {
      const commandEntries: CommandPaletteEntry[] = [
        {
          id: 'compose',
          kind: 'command',
          label: 'Compose new message',
          keywords: 'compose new message draft',
          icon: commandIcon('compose'),
          action: { kind: 'command', commandId: 'compose' },
        },
        {
          id: 'reply',
          kind: 'command',
          label: 'Reply',
          keywords: 'reply respond answer',
          icon: commandIcon('reply'),
          action: { kind: 'command', commandId: 'reply' },
        },
        {
          id: 'archive',
          kind: 'command',
          label: 'Archive selected',
          keywords: 'archive selected',
          icon: commandIcon('archive'),
          action: { kind: 'command', commandId: 'archive' },
        },
        {
          id: 'flag',
          kind: 'command',
          label: 'Flag message',
          keywords: 'flag star selected',
          icon: commandIcon('flag'),
          action: { kind: 'command', commandId: 'flag' },
        },
        {
          id: 'snooze',
          kind: 'command',
          label: 'Snooze…',
          keywords: 'snooze later remind',
          icon: commandIcon('snooze'),
          action: { kind: 'noop', label: 'Snooze' },
        },
        {
          id: 'newSmart',
          kind: 'command',
          label: 'New smart mailbox…',
          keywords: 'new smart mailbox create filter',
          icon: commandIcon('newSmart'),
          action: { kind: 'open-settings', category: 'mailboxes' },
        },
        {
          id: 'newRule',
          kind: 'command',
          label: 'New rule for mailbox…',
          keywords: 'rule mailbox saved search',
          icon: commandIcon('newRule'),
          action: { kind: 'open-settings', category: 'mailboxes' },
        },
        {
          id: 'settings',
          kind: 'command',
          label: 'Open Settings',
          keywords: 'settings preferences',
          icon: commandIcon('settings'),
          action: { kind: 'open-settings' },
        },
        {
          id: 'shortcuts',
          kind: 'command',
          label: 'Keyboard shortcuts',
          keywords: 'keyboard shortcuts help',
          icon: commandIcon('shortcuts'),
          action: { kind: 'command', commandId: 'shortcuts' },
        },
        {
          id: 'account',
          kind: 'command',
          label: 'Add account…',
          keywords: 'account add source login',
          icon: commandIcon('account'),
          action: { kind: 'open-settings', category: 'accounts' },
        },
      ]
      const entries = commandEntries.filter(
        (entry) =>
          matchesQuery(req.query, entry.label, entry.keywords) &&
          (req.context.app.hasSelectedMessage ||
            !['archive', 'flag', 'reply'].includes(entry.id)),
      )

      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(commandProvider, entry, req.query, index),
          ),
        nextCursor: null,
      }
    },
  }
  return commandProvider
}
