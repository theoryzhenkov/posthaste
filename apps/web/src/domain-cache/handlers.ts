import type { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../api/types'
import { EVENT_TOPICS, isDomainEventTopic } from '../domainVocabulary'
import type { DomainEventTopic } from '../domainVocabulary'
import { queryKeys } from '../queryKeys'
import { removeAccountOverview } from './accounts'
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
import { pushNotification } from '../notifications/store'
import { payloadString } from './payload'
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
  [EVENT_TOPICS.AccountStatusChanged]: () => {
    // Account status is served through the accountStatus view (queryKeys.accounts
    // re-served on every account event), so the renderer no longer patches it
    // here. Doing so per status delta would also storm refetches during a sync.
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
      // List and detail surfaces render from runtime view frames, which
      // recompute on keyword events; the renderer no longer patches message
      // caches here. Counts/sidebar (not view-backed) still invalidate.
      invalidateMailboxReadModels(queryClient, event.accountId)
      invalidateMailNavigationBootstrapReadModels(queryClient)
    }

    invalidateTargetMessageReadModels(queryClient, event)
  },
  [EVENT_TOPICS.PushConnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.PushDisconnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.OperationSettled]: (_queryClient, event) => {
    // Surface only failures; a successful flush settles silently.
    const outcome = payloadString(event.payload, 'outcome')
    if (outcome !== 'failed') {
      return
    }
    const id = payloadString(event.payload, 'id') ?? event.accountId
    const detail = payloadString(event.payload, 'error')
    pushNotification({
      severity: 'error',
      title: "Couldn't save a change to the server",
      message: detail ?? undefined,
      dedupeKey: `operation.settled:${id}`,
    })
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
