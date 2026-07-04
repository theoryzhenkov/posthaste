/**
 * This file was auto-generated from asyncapi.json by scripts/gen-event-topics.ts.
 * Do not make direct changes to the file.
 *
 * Regenerate: `bun run events:generate`. The committed copy is drift-checked
 * verbatim by `bun run events:check`, so a server-side topic addition fails the
 * client build until every consumer (notably the domain-cache handler registry)
 * accounts for it.
 */

/** Every event topic the server can emit on `GET /v1/events`. */
export type EventTopic =
  | 'sync.completed'
  | 'sync.failed'
  | 'settings.updated'
  | 'config.reloaded'
  | 'smart_mailbox.created'
  | 'smart_mailbox.updated'
  | 'smart_mailbox.deleted'
  | 'smart_mailbox.reset'
  | 'message.updated'
  | 'message.body_cached'
  | 'mailbox.updated'
  | 'account.updated'
  | 'account.created'
  | 'account.deleted'
  | 'account.status_changed'
  | 'push.connected'
  | 'push.disconnected'
  | 'operation.settled'
  | 'operation.dispatch_uncertain'
  | 'rule.fired'
  | 'rule.delivery.failed'

/**
 * PascalCase-named accessors for each {@link EventTopic}. Consumers reference
 * topics through these so a renamed/removed wire string is a compile error at the
 * use site, not a silent string typo.
 */
export const EVENT_TOPICS = {
  SyncCompleted: 'sync.completed',
  SyncFailed: 'sync.failed',
  SettingsUpdated: 'settings.updated',
  ConfigReloaded: 'config.reloaded',
  SmartMailboxCreated: 'smart_mailbox.created',
  SmartMailboxUpdated: 'smart_mailbox.updated',
  SmartMailboxDeleted: 'smart_mailbox.deleted',
  SmartMailboxReset: 'smart_mailbox.reset',
  MessageUpdated: 'message.updated',
  MessageBodyCached: 'message.body_cached',
  MailboxUpdated: 'mailbox.updated',
  AccountUpdated: 'account.updated',
  AccountCreated: 'account.created',
  AccountDeleted: 'account.deleted',
  AccountStatusChanged: 'account.status_changed',
  PushConnected: 'push.connected',
  PushDisconnected: 'push.disconnected',
  OperationSettled: 'operation.settled',
  OperationDispatchUncertain: 'operation.dispatch_uncertain',
  RuleFired: 'rule.fired',
  RuleDeliveryFailed: 'rule.delivery.failed',
} as const satisfies Record<string, EventTopic>

/** Every topic as an ordered tuple, mirroring the AsyncAPI enum order. */
export const ALL_EVENT_TOPICS = [
  'sync.completed',
  'sync.failed',
  'settings.updated',
  'config.reloaded',
  'smart_mailbox.created',
  'smart_mailbox.updated',
  'smart_mailbox.deleted',
  'smart_mailbox.reset',
  'message.updated',
  'message.body_cached',
  'mailbox.updated',
  'account.updated',
  'account.created',
  'account.deleted',
  'account.status_changed',
  'push.connected',
  'push.disconnected',
  'operation.settled',
  'operation.dispatch_uncertain',
  'rule.fired',
  'rule.delivery.failed',
] as const satisfies readonly EventTopic[]

/**
 * Payload of a domain event. The event contract documents `DomainEvent.payload`
 * as a free-form, topic-dependent object (not exhaustively typed per topic), so
 * every topic maps to this shape today.
 */
export type DomainEventPayload = Record<string, unknown>

/**
 * Per-topic payload map. This is the single seam to enrich when the AsyncAPI
 * contract grows per-topic payload schemas; `PayloadOf<T>` then narrows.
 */
export interface EventPayloadByTopic {
  'sync.completed': DomainEventPayload
  'sync.failed': DomainEventPayload
  'settings.updated': DomainEventPayload
  'config.reloaded': DomainEventPayload
  'smart_mailbox.created': DomainEventPayload
  'smart_mailbox.updated': DomainEventPayload
  'smart_mailbox.deleted': DomainEventPayload
  'smart_mailbox.reset': DomainEventPayload
  'message.updated': DomainEventPayload
  'message.body_cached': DomainEventPayload
  'mailbox.updated': DomainEventPayload
  'account.updated': DomainEventPayload
  'account.created': DomainEventPayload
  'account.deleted': DomainEventPayload
  'account.status_changed': DomainEventPayload
  'push.connected': DomainEventPayload
  'push.disconnected': DomainEventPayload
  'operation.settled': DomainEventPayload
  'operation.dispatch_uncertain': DomainEventPayload
  'rule.fired': DomainEventPayload
  'rule.delivery.failed': DomainEventPayload
}

/** The payload type carried by events on topic `T`. */
export type PayloadOf<T extends EventTopic> = EventPayloadByTopic[T]

const KNOWN_EVENT_TOPICS: ReadonlySet<string> = new Set(ALL_EVENT_TOPICS)

/** Runtime guard narrowing an arbitrary topic string to a known {@link EventTopic}. */
export function isEventTopic(topic: string): topic is EventTopic {
  return KNOWN_EVENT_TOPICS.has(topic)
}
