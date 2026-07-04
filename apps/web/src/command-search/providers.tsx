import type { MessageSummary } from '@/api/types'
import type { MailboxNavigationReadModels } from '@/mailboxNavigationReadModels'

import type { SearchProvider } from './types'
import { createCommandProvider } from './providers/commands'
import { createMailboxProvider } from './providers/mailboxes'
import { createMessageProvider } from './providers/messages'
import { createQueryCompletionProvider } from './providers/queryCompletions'
import { createTagProvider } from './providers/tags'
import { createTagActionProvider } from './providers/tagActions'

export function createCommandProviders(input: {
  readModels: Pick<
    MailboxNavigationReadModels,
    'smartMailboxes' | 'sources' | 'tags'
  >
  recentMessages: MessageSummary[]
  /** User tags on the selected message; drives the selection-scoped tag actions. */
  selectedMessageTags: readonly string[]
}): SearchProvider[] {
  return [
    createCommandProvider(),
    createQueryCompletionProvider(input),
    createMailboxProvider(input),
    createTagProvider(input),
    createTagActionProvider(input),
    createMessageProvider(input),
  ]
}
