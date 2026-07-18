import type { ActionContext, ActionServices } from '@/commands'
import type { MessageSummary } from '@/data/transport/api'
import type { MailboxNavigationReadModels } from '@/data/models/mailboxNavigation'

import type { SearchProvider } from './types'
import { createActionProvider } from './providers/actions'
import { createMailboxProvider } from './providers/mailboxes'
import {
  createMessageProvider,
  type SearchMessagesFn,
} from './providers/messages'
import { createQueryCompletionProvider } from './providers/queryCompletions'
import { createTagProvider } from './providers/tags'

export function createCommandProviders(input: {
  readModels: Pick<
    MailboxNavigationReadModels,
    'smartMailboxes' | 'sources' | 'tags'
  >
  recentMessages: MessageSummary[]
  /** Evaluates the message provider's free-text window (`mailList` family). */
  searchMessages: SearchMessagesFn
  /** Live accessors for the palette's action context + bound services. Read
   *  through getters so the provider list stays referentially stable while the
   *  underlying app state (selection, view role) updates between renders. */
  getActionContext: () => ActionContext
  getActionServices: () => ActionServices
}): SearchProvider[] {
  return [
    createActionProvider({
      getContext: input.getActionContext,
      getServices: input.getActionServices,
    }),
    createQueryCompletionProvider(input),
    createMailboxProvider(input),
    createTagProvider(input),
    createMessageProvider(input),
  ]
}
