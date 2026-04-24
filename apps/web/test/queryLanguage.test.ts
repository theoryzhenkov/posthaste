import { describe, expect, it } from 'bun:test'

import type { MessageSummary, SidebarResponse } from '../src/api/types'
import {
  getQueryCompletions,
  getQueryHelpEntries,
  validateSearchQuery,
} from '../src/queryLanguage'

const sidebar: SidebarResponse = {
  smartMailboxes: [],
  tags: [],
  sources: [
    {
      id: 'primary',
      name: 'Primary Account',
      mailboxes: [
        {
          id: 'archive',
          name: 'Archive',
          role: null,
          unreadEmails: 0,
          totalEmails: 0,
        },
      ],
    },
  ],
}

const message: MessageSummary = {
  id: 'message-1',
  sourceId: 'primary',
  sourceName: 'Primary Account',
  sourceThreadId: 'thread-1',
  conversationId: 'conversation-1',
  subject: 'Welcome',
  fromName: 'Posthaste Author',
  fromEmail: 'author@posthaste.test',
  preview: 'Account creation',
  receivedAt: '2026-04-24T00:00:00Z',
  hasAttachment: false,
  isRead: false,
  isFlagged: false,
  mailboxIds: ['archive'],
  keywords: ['newsletter'],
}

describe('query language completions', () => {
  it('suggests mailbox names for in: value continuations', () => {
    const [completion] = getQueryCompletions('in: Arc', {
      sidebar,
      messages: [],
    })

    expect(completion?.label).toBe('Archive')
    expect(completion?.replacement).toBe('in: Archive')
  })

  it('suggests a new prefix after a non-spaced value token', () => {
    const [completion] = getQueryCompletions('is:unread f', {
      sidebar,
      messages: [],
    })

    expect(completion?.label).toBe('from:')
    expect(completion?.replacement).toBe('is:unread from:')
  })

  it('suggests static state values in declaration order', () => {
    const [completion] = getQueryCompletions('is: u', {
      sidebar,
      messages: [],
    })

    expect(completion?.label).toBe('unread')
    expect(completion?.replacement).toBe('is: unread')
  })

  it('uses cached message metadata for sender and keyword suggestions', () => {
    const sender = getQueryCompletions('from: Post', {
      sidebar,
      messages: [message],
    })
    const tag = getQueryCompletions('tag: news', {
      sidebar,
      messages: [message],
    })

    expect(sender[0]?.replacement).toBe('from: Posthaste Author')
    expect(tag[0]?.replacement).toBe('tag: newsletter')
  })

  it('shows query help entries from help-like input', () => {
    expect(getQueryHelpEntries('query help').length).toBeGreaterThan(0)
  })

  it('validates every generated value completion replacement', () => {
    const contexts = [
      'in: Arc',
      'source: Primary',
      'is: r',
      'has: att',
      'tag: news',
      'from: Post',
      'newer: 1',
      'older: 1',
      'date:',
      'before:',
      'after:',
    ]

    for (const input of contexts) {
      const completions = getQueryCompletions(input, {
        sidebar,
        messages: [message],
        now: new Date('2026-04-24T12:00:00Z'),
      })
      expect(completions.length, input).toBeGreaterThan(0)
      for (const completion of completions) {
        expect(
          validateSearchQuery(completion.replacement),
          completion.replacement,
        ).toEqual({ state: 'valid' })
      }
    }
  })

  it('does not treat incomplete prefixes as valid filters', () => {
    expect(validateSearchQuery('is:')).toMatchObject({ state: 'incomplete' })
    expect(validateSearchQuery('from:')).toMatchObject({ state: 'incomplete' })
  })

  it('accepts all static is: and has: values emitted by the UI', () => {
    for (const query of [
      'is:unread',
      'is:read',
      'is:seen',
      'is:flagged',
      'is:unflagged',
      'is:attachment',
      'has:attachment',
    ]) {
      expect(validateSearchQuery(query), query).toEqual({ state: 'valid' })
    }
  })
})
