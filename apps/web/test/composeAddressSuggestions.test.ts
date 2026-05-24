import { describe, expect, it } from 'bun:test'

import type { AccountOverview, ConversationPage } from '../src/api/types'
import {
  buildRecipientSuggestionOptions,
  filterAddressSuggestions,
  insertAddressSuggestion,
} from '../src/composeAddressSuggestions'

const account = {
  id: 'work',
  name: 'Work',
  fullName: 'Work Person',
  emailPatterns: ['work@example.com', '*@example.org'],
} as AccountOverview

const conversations: ConversationPage = {
  nextCursor: null,
  items: [
    {
      id: 'c1',
      subject: 'Hello',
      preview: null,
      fromName: 'Ada Lovelace',
      fromEmail: 'ada@example.net',
      latestReceivedAt: '2026-05-24T00:00:00Z',
      unreadCount: 0,
      messageCount: 1,
      sourceIds: ['work'],
      sourceNames: ['Work'],
      latestMessage: { sourceId: 'work', messageId: 'm1' },
      latestSourceName: 'Work',
      hasAttachment: false,
      isFlagged: false,
    },
    {
      id: 'c2',
      subject: 'Duplicate',
      preview: null,
      fromName: 'Ada L.',
      fromEmail: 'ADA@example.net',
      latestReceivedAt: '2026-05-23T00:00:00Z',
      unreadCount: 0,
      messageCount: 1,
      sourceIds: ['work'],
      sourceNames: ['Work'],
      latestMessage: { sourceId: 'work', messageId: 'm2' },
      latestSourceName: 'Work',
      hasAttachment: false,
      isFlagged: false,
    },
  ],
}

describe('compose address suggestions', () => {
  it('combines account addresses and recent correspondents', () => {
    const suggestions = buildRecipientSuggestionOptions(
      [account],
      [conversations],
    )

    expect(suggestions.map((suggestion) => suggestion.email)).toEqual([
      'work@example.com',
      'ada@example.net',
    ])
  })

  it('filters against the active recipient token', () => {
    const suggestions = buildRecipientSuggestionOptions(
      [account],
      [conversations],
    )

    expect(
      filterAddressSuggestions(suggestions, 'other@example.com, ada'),
    ).toEqual([expect.objectContaining({ email: 'ada@example.net' })])
  })

  it('inserts a selected suggestion as the current recipient token', () => {
    const [suggestion] = buildRecipientSuggestionOptions([account], [])

    expect(insertAddressSuggestion('one@example.com, wo', suggestion)).toBe(
      'one@example.com, Work Person <work@example.com>, ',
    )
  })
})
