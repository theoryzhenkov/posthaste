import type { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../api/types'
import { EVENT_TOPICS, isDomainEventTopic } from '../domainVocabulary'
import type { DomainEventTopic } from '../domainVocabulary'
import { applyKeywordEventPatch } from '../mailState'
import { queryKeys } from '../queryKeys'
import { applyAccountStatusPatch, removeAccountOverview } from './accounts'
import {
  invalidateAccountReadModels,
  invalidateAccountRuntimeReadModels,
  invalidateMailboxReadModels,
  invalidateMailNavigationBootstrapReadModels,
  invalidateMessageDetailReadModels,
  invalidateMessageListReadModels,
  invalidateSmartMailboxReadModels,
  invalidateTargetMessageReadModels,
} from './invalidations'
import { eventTarget, isStringArray, payloadString } from './payload'
import {
  applyResourceInvalidationsOrFallback,
  type EventHandler,
} from './resources'

const eventHandlers = {
  [EVENT_TOPICS.SettingsUpdated]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateAccountReadModels(client)
      invalidateSmartMailboxReadModels(client)
    })
  },
  [EVENT_TOPICS.ConfigReloaded]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateAccountReadModels(client)
      invalidateSmartMailboxReadModels(client)
      invalidateMailNavigationBootstrapReadModels(client)
      invalidateMessageDetailReadModels(client)
    })
  },
  [EVENT_TOPICS.SmartMailboxCreated]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateSmartMailboxReadModels(
        client,
        payloadString(event.payload, 'smartMailboxId'),
      )
    })
  },
  [EVENT_TOPICS.SmartMailboxUpdated]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateSmartMailboxReadModels(
        client,
        payloadString(event.payload, 'smartMailboxId'),
      )
    })
  },
  [EVENT_TOPICS.SmartMailboxDeleted]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      const smartMailboxId = payloadString(event.payload, 'smartMailboxId')
      if (smartMailboxId) {
        client.removeQueries({
          queryKey: queryKeys.smartMailbox(smartMailboxId),
          exact: true,
        })
      }
      invalidateSmartMailboxReadModels(client, smartMailboxId)
    })
  },
  [EVENT_TOPICS.SmartMailboxReset]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateSmartMailboxReadModels(client)
    })
  },
  [EVENT_TOPICS.SyncCompleted]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateAccountReadModels(client, event.accountId)
      invalidateSmartMailboxReadModels(client)
      invalidateMailNavigationBootstrapReadModels(client)
      invalidateMessageDetailReadModels(client)
    })
  },
  [EVENT_TOPICS.SyncFailed]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.AccountCreated]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateAccountReadModels(client, event.accountId)
    })
  },
  [EVENT_TOPICS.AccountStatusChanged]: (queryClient, event) => {
    if (!applyAccountStatusPatch(queryClient, event.accountId, event.payload)) {
      invalidateAccountRuntimeReadModels(queryClient, event.accountId)
    }
  },
  [EVENT_TOPICS.AccountUpdated]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      invalidateAccountReadModels(client, event.accountId)
    })
  },
  [EVENT_TOPICS.AccountDeleted]: (queryClient, event) => {
    applyResourceInvalidationsOrFallback(queryClient, event, (client) => {
      removeAccountOverview(client, event.accountId)
      invalidateAccountReadModels(client)
      invalidateSmartMailboxReadModels(client)
      invalidateMessageDetailReadModels(client)
    })
  },
  [EVENT_TOPICS.MailboxUpdated]: (queryClient, event) => {
    invalidateMailboxReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.MessageBodyCached]: (queryClient, event) => {
    invalidateTargetMessageReadModels(queryClient, event)
  },
  [EVENT_TOPICS.MessageUpdated]: (queryClient, event) => {
    invalidateMessageListReadModels(queryClient)

    if (payloadChangeFlag(event, 'arrived')) {
      invalidateMailboxReadModels(queryClient, event.accountId)
      invalidateMailNavigationBootstrapReadModels(queryClient)
    }

    if (payloadChangeFlag(event, 'mailboxes')) {
      invalidateMailboxReadModels(queryClient, event.accountId)
      invalidateMailNavigationBootstrapReadModels(queryClient)
    }

    if (payloadChangeFlag(event, 'keywords')) {
      invalidateMailboxReadModels(queryClient, event.accountId)
      invalidateMailNavigationBootstrapReadModels(queryClient)

      const target = eventTarget(event)
      const keywords = event.payload.keywords
      const patched =
        target && isStringArray(keywords)
          ? applyKeywordEventPatch(queryClient, target, keywords)
          : false
      if (patched) {
        return
      }
    }

    invalidateTargetMessageReadModels(queryClient, event)
  },
  [EVENT_TOPICS.PushConnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.PushDisconnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
} satisfies Record<DomainEventTopic, EventHandler>

function payloadChangeFlag(event: DomainEvent, key: string): boolean {
  const changes = event.payload.changes
  return (
    typeof changes === 'object' &&
    changes !== null &&
    key in changes &&
    (changes as Record<string, unknown>)[key] === true
  )
}

export function applyDomainEvent(queryClient: QueryClient, event: DomainEvent) {
  if (!isDomainEventTopic(event.topic)) {
    invalidateAccountReadModels(queryClient)
    return
  }
  eventHandlers[event.topic](queryClient, event)
}
