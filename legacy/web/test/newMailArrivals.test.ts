import { describe, expect, it } from 'bun:test'

import type { DomainEvent, Notifications } from '../src/api/types'
import { EVENT_TOPICS } from '../src/api/events.gen'
import {
  createNewMailArrivalCoordinator,
  type NewMailArrivalDeps,
  type NewMailBanner,
} from '../src/notifications/newMailArrivals'

const BURST_WINDOW_MS = 5

function arrivalEvent(overrides: {
  seq?: number
  accountId?: string
  created?: boolean
  arrived?: boolean
  keywords?: string[]
  fromName?: string | null
  fromEmail?: string | null
  subject?: string | null
}): DomainEvent {
  return {
    seq: overrides.seq ?? 1,
    accountId: overrides.accountId ?? 'primary',
    topic: EVENT_TOPICS.MessageUpdated,
    occurredAt: '2026-07-07T00:00:00Z',
    mailboxId: 'inbox',
    messageId: `message-${overrides.seq ?? 1}`,
    payload: {
      messageId: `message-${overrides.seq ?? 1}`,
      created: overrides.created ?? true,
      changes: {
        keywords: false,
        mailboxes: true,
        arrived: overrides.arrived ?? true,
      },
      keywords: overrides.keywords ?? [],
      projection: {
        fromName: overrides.fromName === undefined ? 'Ada' : overrides.fromName,
        fromEmail:
          overrides.fromEmail === undefined
            ? 'ada@example.test'
            : overrides.fromEmail,
        subject:
          overrides.subject === undefined ? 'Hello there' : overrides.subject,
        isRead: (overrides.keywords ?? []).includes('$seen'),
      },
    },
  }
}

interface Harness {
  banners: NewMailBanner[]
  prefs: { current: Notifications | null | undefined }
  focused: { current: boolean }
  initialSyncAccounts: Set<string>
  coordinator: ReturnType<typeof createNewMailArrivalCoordinator>
  settle: () => Promise<void>
}

function createHarness(deps: Partial<NewMailArrivalDeps> = {}): Harness {
  const banners: NewMailBanner[] = []
  const prefs = {
    current: undefined as Notifications | null | undefined,
  }
  const focused = { current: false }
  const initialSyncAccounts = new Set<string>()
  const coordinator = createNewMailArrivalCoordinator({
    post: (banner) => banners.push(banner),
    getPreferences: () => prefs.current,
    isAppFocused: () => focused.current,
    isAccountInInitialSync: (accountId) => initialSyncAccounts.has(accountId),
    windowMs: BURST_WINDOW_MS,
    ...deps,
  })
  return {
    banners,
    prefs,
    focused,
    initialSyncAccounts,
    coordinator,
    settle: () =>
      new Promise((resolve) => setTimeout(resolve, BURST_WINDOW_MS * 8)),
  }
}

describe('new-mail arrival gate', () => {
  // spec: the `arrived` change flag is the new-arrival wire contract (the same
  // flag the count invalidation keys on).
  it('posts one banner with sender and subject for a new arrival', async () => {
    const harness = createHarness()
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    await harness.settle()

    expect(harness.banners).toHaveLength(1)
    expect(harness.banners[0].title).toBe('Ada')
    expect(harness.banners[0].body).toBe('Hello there')
    expect(harness.banners[0].sound).toBe(true)
  })

  it('falls back to the sender email and a no-subject placeholder', async () => {
    const harness = createHarness()
    harness.coordinator.onMessageUpdated(
      arrivalEvent({ fromName: null, subject: null }),
    )
    await harness.settle()

    expect(harness.banners).toHaveLength(1)
    expect(harness.banners[0].title).toBe('ada@example.test')
    expect(harness.banners[0].body).toBe('(no subject)')
  })

  it('ignores own-mutation echoes without the arrived flag', async () => {
    const harness = createHarness()
    // A mark-read / tag echo: keywords changed, nothing arrived.
    harness.coordinator.onMessageUpdated(arrivalEvent({ arrived: false }))
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })

  it('ignores moves of an existing message (arrived without created)', async () => {
    const harness = createHarness()
    harness.coordinator.onMessageUpdated(arrivalEvent({ created: false }))
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })

  it('ignores messages arriving already read or as drafts', async () => {
    const harness = createHarness()
    // The self-sent copy appended to Sent, and a compose autosave.
    harness.coordinator.onMessageUpdated(arrivalEvent({ keywords: ['$seen'] }))
    harness.coordinator.onMessageUpdated(arrivalEvent({ keywords: ['$draft'] }))
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })

  it('coalesces a burst into one summary banner', async () => {
    const harness = createHarness()
    for (let index = 0; index < 10; index += 1) {
      harness.coordinator.onMessageUpdated(
        arrivalEvent({ seq: index, fromName: `Sender ${index}` }),
      )
    }
    await harness.settle()

    expect(harness.banners).toHaveLength(1)
    expect(harness.banners[0].title).toBe('10 new messages')
    expect(harness.banners[0].body).toContain('Sender 0 — Hello there')
    expect(harness.banners[0].body).toContain('…and 7 more')
  })

  it('suppresses bursts beyond the backfill threshold entirely', async () => {
    const harness = createHarness()
    for (let index = 0; index < 26; index += 1) {
      harness.coordinator.onMessageUpdated(arrivalEvent({ seq: index }))
    }
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })

  it('suppresses banners while the app window is focused', async () => {
    const harness = createHarness()
    harness.focused.current = true
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })

  it('checks focus at flush time, not arrival time', async () => {
    const harness = createHarness()
    harness.focused.current = false
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    harness.focused.current = true
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })

  it('respects the master toggle', async () => {
    const harness = createHarness()
    harness.prefs.current = { newMail: false, sound: true }
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })

  it('carries the sound toggle on the banner', async () => {
    const harness = createHarness()
    harness.prefs.current = { newMail: true, sound: false }
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    await harness.settle()

    expect(harness.banners).toHaveLength(1)
    expect(harness.banners[0].sound).toBe(false)
  })

  it('treats absent preferences as the pane defaults (alerts on)', async () => {
    const harness = createHarness()
    harness.prefs.current = null
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    await harness.settle()

    expect(harness.banners).toHaveLength(1)
  })

  it("suppresses an account's initial-sync backfill", async () => {
    const harness = createHarness()
    harness.initialSyncAccounts.add('primary')
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    harness.coordinator.onMessageUpdated(
      arrivalEvent({ seq: 2, accountId: 'secondary' }),
    )
    await harness.settle()

    // Only the synced account's arrival banners.
    expect(harness.banners).toHaveLength(1)
    expect(harness.banners[0].title).toBe('Ada')
  })

  it('flushes a sustained trickle once the coalescing cap is hit', async () => {
    let clock = 0
    const harness = createHarness({
      now: () => clock,
      maxCoalesceMs: BURST_WINDOW_MS * 4,
    })
    // Each arrival lands inside the sliding window, but the cap forces a
    // flush instead of deferring forever.
    for (let index = 0; index < 6; index += 1) {
      harness.coordinator.onMessageUpdated(arrivalEvent({ seq: index }))
      clock += BURST_WINDOW_MS
    }
    expect(harness.banners).toHaveLength(1)
    expect(harness.banners[0].title).toBe('5 new messages')

    harness.coordinator.dispose()
  })

  it('drops pending arrivals on dispose', async () => {
    const harness = createHarness()
    harness.coordinator.onMessageUpdated(arrivalEvent({}))
    harness.coordinator.dispose()
    await harness.settle()

    expect(harness.banners).toHaveLength(0)
  })
})
