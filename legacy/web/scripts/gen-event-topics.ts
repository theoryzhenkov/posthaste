/**
 * Generator for `src/api/events.gen.ts` — the client mirror of the AsyncAPI
 * event boundary. Reads the `EventTopic` enum from the committed `asyncapi.json`
 * (itself drift-checked against `posthaste_domain::ALL_EVENT_TOPICS`) and emits:
 *   - the `EventTopic` string union,
 *   - `EVENT_TOPICS` (PascalCase-named accessors) + `ALL_EVENT_TOPICS` tuple,
 *   - `PayloadOf<T>` (free-form today; the single enrichment seam),
 *   - the `isEventTopic` runtime guard.
 *
 * This is the event-side analogue of `api:generate` (openapi-typescript). Run it
 * with `bun run events:generate`; `bun run events:check` fails the build when the
 * committed output drifts from what this generator would produce.
 *
 * @spec docs/L1-api#sse-event-stream
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const asyncapiPath = new URL('../../../asyncapi.json', import.meta.url)
const outPath = new URL('../src/api/events.gen.ts', import.meta.url)

/** Extract the ordered `EventTopic` enum from the AsyncAPI document. */
export function readEventTopics(asyncapiJson: string): string[] {
  const doc = JSON.parse(asyncapiJson) as {
    components?: {
      schemas?: { EventTopic?: { enum?: unknown } }
    }
  }
  const topics = doc.components?.schemas?.EventTopic?.enum
  if (!Array.isArray(topics) || topics.some((t) => typeof t !== 'string')) {
    throw new Error(
      'asyncapi.json: components.schemas.EventTopic.enum must be a string[]',
    )
  }
  return topics as string[]
}

/** `sync.completed` -> `SyncCompleted`, `smart_mailbox.created` -> `SmartMailboxCreated`. */
export function pascalName(topic: string): string {
  return topic
    .split(/[._]/)
    .map((seg) => seg.charAt(0).toUpperCase() + seg.slice(1))
    .join('')
}

/** Render the full `events.gen.ts` module source from an AsyncAPI document. */
export function renderEventTopicsModule(asyncapiJson: string): string {
  const topics = readEventTopics(asyncapiJson)
  const seen = new Set<string>()
  for (const name of topics.map(pascalName)) {
    if (seen.has(name)) {
      throw new Error(`event topic name collision after PascalCase: ${name}`)
    }
    seen.add(name)
  }

  const union = topics.map((t) => `  | '${t}'`).join('\n')
  const named = topics.map((t) => `  ${pascalName(t)}: '${t}',`).join('\n')
  const all = topics.map((t) => `  '${t}',`).join('\n')
  const payloadMap = topics
    .map((t) => `  '${t}': DomainEventPayload`)
    .join('\n')

  return `/**
 * This file was auto-generated from asyncapi.json by scripts/gen-event-topics.ts.
 * Do not make direct changes to the file.
 *
 * Regenerate: \`bun run events:generate\`. The committed copy is drift-checked
 * verbatim by \`bun run events:check\`, so a server-side topic addition fails the
 * client build until every consumer (notably the domain-cache handler registry)
 * accounts for it.
 */

/** Every event topic the server can emit on \`GET /v1/events\`. */
export type EventTopic =
${union}

/**
 * PascalCase-named accessors for each {@link EventTopic}. Consumers reference
 * topics through these so a renamed/removed wire string is a compile error at the
 * use site, not a silent string typo.
 */
export const EVENT_TOPICS = {
${named}
} as const satisfies Record<string, EventTopic>

/** Every topic as an ordered tuple, mirroring the AsyncAPI enum order. */
export const ALL_EVENT_TOPICS = [
${all}
] as const satisfies readonly EventTopic[]

/**
 * Payload of a domain event. The event contract documents \`DomainEvent.payload\`
 * as a free-form, topic-dependent object (not exhaustively typed per topic), so
 * every topic maps to this shape today.
 */
export type DomainEventPayload = Record<string, unknown>

/**
 * Per-topic payload map. This is the single seam to enrich when the AsyncAPI
 * contract grows per-topic payload schemas; \`PayloadOf<T>\` then narrows.
 */
export interface EventPayloadByTopic {
${payloadMap}
}

/** The payload type carried by events on topic \`T\`. */
export type PayloadOf<T extends EventTopic> = EventPayloadByTopic[T]

const KNOWN_EVENT_TOPICS: ReadonlySet<string> = new Set(ALL_EVENT_TOPICS)

/** Runtime guard narrowing an arbitrary topic string to a known {@link EventTopic}. */
export function isEventTopic(topic: string): topic is EventTopic {
  return KNOWN_EVENT_TOPICS.has(topic)
}
`
}

if (import.meta.main) {
  const asyncapiJson = readFileSync(fileURLToPath(asyncapiPath), 'utf8')
  writeFileSync(fileURLToPath(outPath), renderEventTopicsModule(asyncapiJson))
  console.log(`Wrote ${fileURLToPath(outPath).slice(root.length)}`)
}
