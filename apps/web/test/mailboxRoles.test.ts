import { describe, expect, it } from 'bun:test'

import { Folder, Mail } from 'lucide-react'

import { ALL_MAIL_DEFAULT_KEY, MAILBOX_ROLES } from '../src/domainVocabulary'
import { smartMailboxAccent, smartMailboxFallbackIcon } from '../src/mailboxRoles'

describe('smartMailboxAccent', () => {
  it('keys off the role for role-tagged smart mailboxes (rename-safe)', () => {
    // A role-tagged smart mailbox keeps its accent regardless of display name —
    // the name-based identity smell this replaced broke on rename/locale.
    expect(smartMailboxAccent(MAILBOX_ROLES.Trash, 'Trash')).toBe(
      smartMailboxAccent(MAILBOX_ROLES.Trash, 'My Renamed Trash'),
    )
    expect(smartMailboxAccent(MAILBOX_ROLES.Trash, 'Anything')).toBe(
      'oklch(0.70 0.15 12)',
    )
    expect(smartMailboxAccent(MAILBOX_ROLES.Archive, 'Archive')).toBe(
      'oklch(0.65 0.13 245)',
    )
  })

  it('falls back to the display name for role-less items (All Mail, tags)', () => {
    // No stable role/id to key off — name is the only signal left.
    expect(smartMailboxAccent(null, 'All Mail')).toBe('oklch(0.65 0.13 245)')
    expect(smartMailboxAccent(null, 'Newsletters')).toBe('oklch(0.68 0.08 145)')
    expect(smartMailboxAccent(null, 'unknown')).toBe('oklch(0.60 0.008 70)')
  })
})

describe('smartMailboxFallbackIcon', () => {
  it('keys All Mail off the stable defaultKey, not the display name', () => {
    expect(smartMailboxFallbackIcon(ALL_MAIL_DEFAULT_KEY)).toBe(Mail)
    // A role-tagged smart mailbox (e.g. Inbox) does not get the Mail icon even
    // if it were somehow named "All Mail" — defaultKey drives it, not the name.
    expect(smartMailboxFallbackIcon('inbox')).toBe(Folder)
    expect(smartMailboxFallbackIcon(null)).toBe(Folder)
  })
})
