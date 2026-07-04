import { describe, expect, it } from 'bun:test'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import {
  ALL_EVENT_TOPICS,
  EVENT_TOPICS,
  isEventTopic,
} from '../src/api/events.gen'
import { registeredEventTopics } from '../src/domain-cache/handlers'

/**
 * Guards the M47 event boundary: the generated topic list is the single source
 * of truth, and the domain-cache handler registry must enumerate every topic.
 * Compile-time exhaustiveness is enforced by `satisfies DomainEventHandlers`;
 * these tests assert the runtime shape matches the generated contract.
 */

const asyncapiTopics = (() => {
  const path = fileURLToPath(new URL('../../../asyncapi.json', import.meta.url))
  const doc = JSON.parse(readFileSync(path, 'utf8')) as {
    components: { schemas: { EventTopic: { enum: string[] } } }
  }
  return doc.components.schemas.EventTopic.enum
})()

describe('event boundary codegen (M47/D118)', () => {
  it('generated ALL_EVENT_TOPICS mirrors the AsyncAPI EventTopic enum', () => {
    expect([...ALL_EVENT_TOPICS]).toEqual(asyncapiTopics)
  })

  it('EVENT_TOPICS named accessors cover exactly the topic set', () => {
    expect(Object.values(EVENT_TOPICS).sort()).toEqual(
      [...ALL_EVENT_TOPICS].sort(),
    )
  })

  it('isEventTopic accepts known topics and rejects unknown ones', () => {
    for (const topic of ALL_EVENT_TOPICS) {
      expect(isEventTopic(topic)).toBe(true)
    }
    expect(isEventTopic('nope.invented')).toBe(false)
    expect(isEventTopic('')).toBe(false)
  })

  it('domain-cache registry enumerates every generated topic (no holes)', () => {
    expect(registeredEventTopics.slice().sort()).toEqual(
      [...ALL_EVENT_TOPICS].sort(),
    )
  })
})
