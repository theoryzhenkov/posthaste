import type { SiteMessage } from '../content/types'

export interface Mailbox {
  id: 'inbox' | 'archive'
  label: string
  count: string
  active?: boolean
}

export type MailboxView = Mailbox['id']
export type MockTheme = 'baseline' | 'glass' | 'typewriter' | 'pigeon'

export interface PersistedMockState {
  selectedMailbox?: MailboxView
  selectedMessageId?: string | null
  readMessageIds?: string[]
  archivedMessageIds?: string[]
  flaggedMessageIds?: string[]
  mockTheme?: MockTheme
  hasUnlockedSecretTheme?: boolean
}

export interface MessagePreview extends SiteMessage {
  archived: boolean
  flagged: boolean
}

export function mailboxCounts(messages: MessagePreview[]) {
  return {
    inbox: messages.filter((message) => !message.archived && message.unread)
      .length,
    archive: messages.filter((message) => message.archived && message.unread)
      .length,
  }
}
