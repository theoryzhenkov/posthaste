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
    // The entity store owns the mail-list rows (synthesized view frames) + the
    // mailbox counts (`setQueryData`), so the store-owned invalidations are
    // skipped to avoid a redundant REST refetch. Surfaces the store does not own
    // (conversations, smart-mailboxes, tags, mail-navigation, message detail)
    // still invalidate.
    const skipStoreOwned = true
    invalidateMessageListReadModels(queryClient, { skipStoreOwned })

    if (payloadChangeFlag(event, 'arrived')) {
      invalidateMailboxReadModels(queryClient, event.accountId, {
        skipStoreOwned,
      })
      invalidateMailNavigationBootstrapReadModels(queryClient)
    }

    if (payloadChangeFlag(event, 'mailboxes')) {
      invalidateMailboxReadModels(queryClient, event.accountId, {
        skipStoreOwned,
      })
      invalidateMailNavigationBootstrapReadModels(queryClient)
    }

    if (payloadChangeFlag(event, 'keywords')) {
      // List/detail surfaces render from runtime view frames, which recompute
      // on keyword events. When the entity store is active it owns the counts
      // too (`setQueryData`); otherwise counts/sidebar invalidate here.
      invalidateMailboxReadModels(queryClient, event.accountId, {
        skipStoreOwned,
      })
      invalidateMailNavigationBootstrapReadModels(queryClient)
    }

    if (event.payload.deleted === true) {
      // A destroy/expunge carries no `changes` object, so the membership
      // branches above don't fire. Counts/sidebar are not view-backed and would
      // otherwise lag until the next sync, so invalidate them on deletion too
      // (unless the entity store owns them).
      invalidateMailboxReadModels(queryClient, event.accountId, {
        skipStoreOwned,
      })
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
  [EVENT_TOPICS.OperationDispatchUncertain]: (queryClient, event) => {
    // A send may or may not have reached the recipient — never silently retried
    // (RFC-L2 D86/O1). Refresh the outbox so the parked send surfaces there, and
    // raise a needs-attention notification pointing the user at it.
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
    const id = payloadString(event.payload, 'id') ?? event.accountId
    const reason = payloadString(event.payload, 'reason')
    pushNotification({
      severity: 'warning',
      title: 'A message may not have been sent',
      message: reason
        ? `${reason} — open the Outbox to retry or discard it.`
        : 'Open the Outbox to retry or discard it.',
      dedupeKey: `operation.dispatch_uncertain:${id}`,
    })
  },
  [EVENT_TOPICS.RuleFired]: () => {
    // An automation rule fired at the authority server. Its Level-0 effects
    // (tag/move) reach the web through the message.updated fact they emit, so
    // this audit fact needs no additional cache reaction.
  },
  [EVENT_TOPICS.RuleDeliveryFailed]: () => {
    // A rule's webhook/exec delivery was abandoned (dead-letter). Audit-only on
    // the web side; no cache reaction.
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
