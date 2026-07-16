import type { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../api/types'
import { EVENT_TOPICS, isEventTopic } from '../api/events.gen'
import type { EventTopic, PayloadOf } from '../api/events.gen'
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
import { invalidateMailboxCountsDebounced } from './mailboxCounts'
import { notifyNewMailFromEvent } from '../notifications/newMailArrivals'
import { pushNotification } from '../notifications/store'
import { payloadString } from './payload'
import { applyResourceInvalidationsOrFallback, noop } from './resources'

/**
 * A cache reaction to one topic's event. Payloads are free-form in the event
 * contract today, so `Handler<PayloadOf<T>>` collapses to a uniform handler; the
 * generic keeps the registry reading as the RFC's `Handler<PayloadOf<T>>` and
 * makes per-topic payload narrowing a one-line change once the contract enriches.
 */
type Handler<P> = (
  queryClient: QueryClient,
  event: DomainEvent & { payload: P },
) => void

/**
 * The exhaustive topic -> handler registry. Because this is a mapped type over
 * the generated `EventTopic` union, the object below must supply an entry for
 * EVERY topic or `tsc` fails — the audit's coverage matrix is now enforced by the
 * type system, not by hand.
 */
export type DomainEventHandlers = { [T in EventTopic]: Handler<PayloadOf<T>> }

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
  // Account status is served through the accountStatus view (queryKeys.accounts
  // re-served on every account event), so the renderer no longer patches it here.
  // Doing so per status delta would also storm refetches during a sync.
  [EVENT_TOPICS.AccountStatusChanged]: noop(
    'account.status_changed — served via the accountStatus view, no cache patch',
  ),
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
    // The entity store owns the mail-list ROWS (synthesized view frames), so
    // the store-owned row invalidation is skipped. Mailbox COUNTS are
    // react-query state (RFC-L2-count-unification): every count-affecting
    // change — keyword flips, membership moves, arrivals, deletions — fires
    // the count invalidation, and react-query refetches the runtime's
    // canonical counts. This one trigger point serves every topology: the
    // bundled echo, the sync re-emit, and the split runtime's down-channel
    // republish all arrive here as `message.updated` (the class of all three
    // countDelta bugs). Debounced per account so a sync burst coalesces into
    // ~one refetch per window instead of a stampede.
    invalidateMessageListReadModels(queryClient, { skipStoreOwned: true })

    const countAffecting =
      payloadChangeFlag(event, 'arrived') ||
      payloadChangeFlag(event, 'mailboxes') ||
      payloadChangeFlag(event, 'keywords') ||
      event.payload.deleted === true
    if (countAffecting) {
      invalidateMailboxCountsDebounced(queryClient, event.accountId)
    }

    // New-mail OS banner: keys on the SAME `arrived` wire flag as the count
    // invalidation above. The arrival gate (burst coalescing, initial-sync and
    // backfill suppression, focus check, pane toggles) lives in
    // notifications/newMailArrivals.
    notifyNewMailFromEvent(queryClient, event)

    invalidateTargetMessageReadModels(queryClient, event)
  },
  [EVENT_TOPICS.PushConnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.PushDisconnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.OperationSettled]: (_queryClient, event) => {
    const outcome = payloadString(event.payload, 'outcome')
    const id = payloadString(event.payload, 'id') ?? event.accountId
    // S-VERD-3 (D154): a delivered send whose Sent copy is NOT confirmed
    // filed gets a truthful verdict instead of a silent Drafts ghost — the
    // message went out; only the Sent-folder copy is still reconciling.
    if (
      outcome === 'applied' &&
      payloadString(event.payload, 'sendFiling') === 'pendingFiling'
    ) {
      pushNotification({
        severity: 'warning',
        title: 'Sent — still filing the Sent-folder copy',
        message:
          'The message was delivered, but the server has not confirmed the Sent copy yet. It reconciles on a later sync.',
        dedupeKey: `operation.settled:${id}`,
      })
      return
    }
    // Otherwise surface only failures; a successful flush settles silently.
    if (outcome !== 'failed') {
      return
    }
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
  // An automation rule fired at the authority server. Its Level-0 effects
  // (tag/move) reach the web through the message.updated fact they emit, so this
  // audit fact needs no additional cache reaction.
  [EVENT_TOPICS.RuleFired]: noop(
    'rule.fired — tap-only; effects arrive via the message.updated fact',
  ),
  // A rule's webhook/exec delivery was abandoned (dead-letter). Audit-only on the
  // web side; no cache reaction.
  [EVENT_TOPICS.RuleDeliveryFailed]: noop(
    'rule.delivery.failed — audit-only dead-letter, no cache reaction',
  ),
} satisfies DomainEventHandlers

function payloadChangeFlag(event: DomainEvent, key: string): boolean {
  const changes = event.payload.changes
  return (
    typeof changes === 'object' &&
    changes !== null &&
    key in changes &&
    (changes as Record<string, unknown>)[key] === true
  )
}

/**
 * The topics the registry actually wires, for the runtime exhaustiveness test
 * (asserting these keys equal the generated `ALL_EVENT_TOPICS`). Compile-time
 * exhaustiveness is enforced by `satisfies DomainEventHandlers` above.
 */
export const registeredEventTopics = Object.keys(eventHandlers) as EventTopic[]

export function applyDomainEvent(queryClient: QueryClient, event: DomainEvent) {
  if (!isEventTopic(event.topic)) {
    invalidateAccountReadModels(queryClient)
    return
  }
  eventHandlers[event.topic](queryClient, event)
}
