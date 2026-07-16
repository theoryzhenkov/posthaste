import { describe, expect, it } from 'bun:test'

import type {
  AccountOverview,
  CachedSenderAddress,
  ConversationPage,
} from '../src/api/types'
import {
  buildAddressBookSuggestionOptions,
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

  it('matches case-insensitively and by SUBSTRING (not prefix) over name, email, and label', () => {
    // Root-cause pin for "autocomplete often doesn't work": the filter must
    // find "lovelace" in the middle of an email, a display name typed in the
    // wrong case, etc. — a prefix-only or case-sensitive match would return
    // nothing for all of these.
    const options = buildAddressBookSuggestionOptions([
      {
        sourceId: 'work',
        name: 'Ada Lovelace',
        email: 'ada.lovelace@analytical.example',
        lastUsedAt: '',
      },
    ])

    for (const needle of ['lovelace', 'LOVELACE', 'analytical', 'da Lov']) {
      expect(
        filterAddressSuggestions(options, needle, 8, 'whole'),
      ).toHaveLength(1)
    }
    expect(
      filterAddressSuggestions(options, 'nomatch', 8, 'whole'),
    ).toHaveLength(0)
  })

  it('whole mode filters on the entire value; token mode on the last comma token', () => {
    // A single-value input (a rule condition) must never be comma-tokenized:
    // "Doe, John" is ONE value there. In token mode (compose recipients) the
    // same text filters on the trailing token only.
    const options = buildAddressBookSuggestionOptions([
      {
        sourceId: 'work',
        name: 'Doe, John',
        email: 'john@example.com',
        lastUsedAt: '',
      },
      {
        sourceId: 'work',
        name: 'Jane Roe',
        email: 'jane@example.com',
        lastUsedAt: '',
      },
    ])

    // whole: the full "Doe, John" needle matches only the Doe entry.
    expect(
      filterAddressSuggestions(options, 'Doe, John', 8, 'whole').map(
        (option) => option.email,
      ),
    ).toEqual(['john@example.com'])
    // token: the needle is just "John" — still matches (substring), proving
    // the two modes agree where they should…
    expect(
      filterAddressSuggestions(options, 'Doe, John', 8, 'token').map(
        (option) => option.email,
      ),
    ).toEqual(['john@example.com'])
    // …and differ where tokenization matters: after a picked address plus a
    // comma, token mode offers everything for the fresh (empty) token.
    expect(
      filterAddressSuggestions(options, 'john@example.com, ', 8, 'token'),
    ).toHaveLength(2)
  })

  it('shows the ranked head of the book when nothing is typed yet', () => {
    // Focus-with-empty-input must offer suggestions (the book is rank-ordered
    // server-side), not wait for a first keystroke.
    const options = buildAddressBookSuggestionOptions(
      Array.from({ length: 12 }, (_, i) => ({
        sourceId: 'work',
        name: null,
        email: `c${i}@example.com`,
        lastUsedAt: '',
      })),
    )
    const shown = filterAddressSuggestions(options, '', 8, 'whole')
    expect(shown).toHaveLength(8)
    expect(shown[0].email).toBe('c0@example.com')
  })

  it('maps the server address book, filtering junk and de-duping by email', () => {
    const book: CachedSenderAddress[] = [
      {
        sourceId: 'work',
        name: 'Ada',
        email: 'ada@example.net',
        lastUsedAt: '',
      },
      // duplicate email (different case) — dropped
      {
        sourceId: 'home',
        name: null,
        email: 'ADA@example.net',
        lastUsedAt: '',
      },
      // wildcard pattern — not a concrete address
      { sourceId: 'work', name: null, email: '*@example.com', lastUsedAt: '' },
      {
        sourceId: 'work',
        name: 'Bob',
        email: 'bob@example.com',
        lastUsedAt: '',
      },
    ]

    const options = buildAddressBookSuggestionOptions(book)

    expect(options.map((option) => option.email)).toEqual([
      'ada@example.net',
      'bob@example.com',
    ])
    expect(options[0]).toEqual(
      expect.objectContaining({ name: 'Ada', sourceLabel: 'Address book' }),
    )
  })
})
