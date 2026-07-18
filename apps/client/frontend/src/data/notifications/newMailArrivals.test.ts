import { describe, expect, test } from 'bun:test'

import type { DomainEventPayload } from '@/gen'
import {
  createNewMailArrivalCoordinator,
  type NewMailArrivalDeps,
  type NewMailBanner,
} from './newMailArrivals'

function arrivalEvent(
  overrides: Partial<DomainEventPayload> = {},
  payload: Record<string, unknown> = {},
): DomainEventPayload {
  return {
    kind: 'message.updated',
    accountId: 'acct-1',
    payload: {
      created: true,
      changes: { arrived: true },
      keywords: [],
      projection: { fromName: 'Ada', subject: 'Hello' },
      ...payload,
    },
    ...overrides,
  }
}

function harness(overrides: Partial<NewMailArrivalDeps> = {}) {
  const posted: NewMailBanner[] = []
  let now = 1_000
  const deps: NewMailArrivalDeps = {
    post: (banner) => posted.push(banner),
    getPreferences: () => ({ newMail: true, sound: true }),
    isAppFocused: () => false,
    isAccountInInitialSync: () => false,
    // Window 0 flushes synchronously via the max-coalesce cap on the second
    // event; single-event tests advance time past the cap instead.
    windowMs: 5,
    maxCoalesceMs: 50,
    now: () => now,
    ...overrides,
  }
  const coordinator = createNewMailArrivalCoordinator(deps)
  return {
    coordinator,
    posted,
    advance: (ms: number) => {
      now += ms
    },
  }
}

async function settle(ms = 20): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms))
}

describe('new-mail arrival gate', () => {
  test('a created+arrived event banners with sender and subject', async () => {
    const { coordinator, posted } = harness()
    coordinator.onMessageUpdated(arrivalEvent())
    await settle()
    expect(posted).toHaveLength(1)
    expect(posted[0]).toMatchObject({ title: 'Ada', body: 'Hello' })
    coordinator.dispose()
  })

  test('mutation echoes and moves of existing messages never banner', async () => {
    const { coordinator, posted } = harness()
    // Keyword flip: no arrived flag.
    coordinator.onMessageUpdated(arrivalEvent({}, { changes: {} }))
    // Move of an existing message: arrived but not created.
    coordinator.onMessageUpdated(arrivalEvent({}, { created: false }))
    // Payload absent entirely.
    coordinator.onMessageUpdated({
      kind: 'message.updated',
      accountId: 'acct-1',
    })
    await settle()
    expect(posted).toHaveLength(0)
    coordinator.dispose()
  })

  test('self-sent ($seen) and draft ($draft) arrivals are skipped', async () => {
    const { coordinator, posted } = harness()
    coordinator.onMessageUpdated(arrivalEvent({}, { keywords: ['$seen'] }))
    coordinator.onMessageUpdated(arrivalEvent({}, { keywords: ['$draft'] }))
    await settle()
    expect(posted).toHaveLength(0)
    coordinator.dispose()
  })

  test('initial-sync accounts are suppressed', async () => {
    const { coordinator, posted } = harness({
      isAccountInInitialSync: (accountId) => accountId === 'acct-1',
    })
    coordinator.onMessageUpdated(arrivalEvent())
    await settle()
    expect(posted).toHaveLength(0)
    coordinator.dispose()
  })

  test('a burst coalesces into one summary banner', async () => {
    const { coordinator, posted } = harness()
    coordinator.onMessageUpdated(
      arrivalEvent({}, { projection: { fromName: 'A', subject: 'one' } }),
    )
    coordinator.onMessageUpdated(
      arrivalEvent({}, { projection: { fromName: 'B', subject: 'two' } }),
    )
    await settle()
    expect(posted).toHaveLength(1)
    expect(posted[0]?.title).toBe('2 new messages')
    coordinator.dispose()
  })

  test('a backfill storm is dropped entirely', async () => {
    const { coordinator, posted } = harness({ backfillThreshold: 2 })
    for (let i = 0; i < 4; i++) {
      coordinator.onMessageUpdated(arrivalEvent())
    }
    await settle()
    expect(posted).toHaveLength(0)
    coordinator.dispose()
  })

  test('the focused window never banners', async () => {
    const { coordinator, posted } = harness({ isAppFocused: () => true })
    coordinator.onMessageUpdated(arrivalEvent())
    await settle()
    expect(posted).toHaveLength(0)
    coordinator.dispose()
  })

  test('the newMail toggle gates delivery', async () => {
    const { coordinator, posted } = harness({
      getPreferences: () => ({ newMail: false, sound: true }),
    })
    coordinator.onMessageUpdated(arrivalEvent())
    await settle()
    expect(posted).toHaveLength(0)
    coordinator.dispose()
  })
})
