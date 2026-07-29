import { describe, expect, test } from 'bun:test'

import { syncProgressView } from './syncProgress'
import type { SyncProgress } from '@/gen'

function progress(overrides: Partial<SyncProgress> = {}): SyncProgress {
  return {
    syncId: 'sync-1',
    trigger: 'poll',
    startedAt: '2026-07-30T08:00:00Z',
    stage: 'fetching',
    detail: 'Fetching mailbox',
    ...overrides,
  }
}

describe('syncProgressView', () => {
  test('IMAP-shaped progress names the mailbox and fills the bar', () => {
    const view = syncProgressView(
      progress({ mailboxName: 'Inbox', mailboxIndex: 2, mailboxCount: 8 }),
    )
    expect(view.label).toBe('Fetching mailbox — Inbox (2 of 8)')
    expect(view.percent).toBe(25)
  })

  test('JMAP-shaped progress keeps the bar indeterminate', () => {
    // JMAP reports stage + detail only; a bar the sync cannot back up would be
    // a lie, so percent stays null.
    const view = syncProgressView(
      progress({ stage: 'discovering', detail: 'Checking for changes' }),
    )
    expect(view.label).toBe('Checking for changes')
    expect(view.percent).toBeNull()
  })

  test('the index is 1-based, so the final mailbox reads as complete', () => {
    const view = syncProgressView(
      progress({ mailboxName: 'Archive', mailboxIndex: 7, mailboxCount: 7 }),
    )
    expect(view.label).toBe('Fetching mailbox — Archive (7 of 7)')
    expect(view.percent).toBe(100)
  })

  test('an empty detail falls back to a phrase for the stage', () => {
    for (const [stage, expected] of [
      ['connecting', 'Connecting'],
      ['discovering', 'Checking for changes'],
      ['planning', 'Planning the sync'],
      ['fetching', 'Fetching mail'],
      ['storing', 'Saving mail'],
      ['waiting', 'Waiting for the server'],
    ] as const) {
      expect(syncProgressView(progress({ stage, detail: '' })).label).toBe(
        expected,
      )
      expect(syncProgressView(progress({ stage, detail: '   ' })).label).toBe(
        expected,
      )
    }
  })

  test('nonsensical counts do not produce a broken bar', () => {
    // Guarding the render, not the server: a zero count has no fraction, and an
    // out-of-range index would overflow the track or run backwards.
    for (const counts of [
      { mailboxIndex: 1, mailboxCount: 0 },
      { mailboxIndex: 0, mailboxCount: 5 },
      { mailboxIndex: 9, mailboxCount: 5 },
    ]) {
      expect(syncProgressView(progress(counts)).percent).toBeNull()
    }
  })

  test('a count without an index still reports no position', () => {
    expect(syncProgressView(progress({ mailboxCount: 5 })).percent).toBeNull()
  })
})
