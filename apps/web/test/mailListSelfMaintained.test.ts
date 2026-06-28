import { describe, expect, it } from 'bun:test'

import type { RuntimeMessagePageScope } from '../src/runtime/types'
import {
  isMailListSelfMaintained,
  resolveMailListPredicate,
  type MailListPredicateContext,
} from '../src/runtime/mailListSelfMaintained'

const sourceMailbox = (mailboxId: string | null): RuntimeMessagePageScope => ({
  kind: 'source-mailbox',
  sourceId: 'primary',
  mailboxId,
})

// "All Inboxes" (default-inbox) + "All Mail" (default-all-mail) smart mailboxes,
// with the inbox role mailbox in two accounts.
const ctx: MailListPredicateContext = {
  smartMailboxDefaultKey: (id) =>
    ({
      'default-inbox': 'inbox',
      'default-all-mail': 'all-mail',
      'user-rule': null,
    })[id],
  mailboxesForRole: (role) =>
    role === 'inbox' ? ['inbox-a', 'inbox-b'] : [],
}

describe('resolveMailListPredicate', () => {
  it('intersects all accounts for a role smart mailbox (All Inboxes)', () => {
    expect(
      resolveMailListPredicate(
        { kind: 'smart-mailbox', smartMailboxId: 'default-inbox' },
        'date',
        ctx,
      ),
    ).toEqual({ inMailboxes: ['inbox-a', 'inbox-b'] })
  })

  it('maps the All Mail smart mailbox (empty rule) to `all`', () => {
    expect(
      resolveMailListPredicate(
        { kind: 'smart-mailbox', smartMailboxId: 'default-all-mail' },
        'date',
        ctx,
      ),
    ).toBe('all')
  })

  it('is a one-element set for a concrete source mailbox', () => {
    expect(resolveMailListPredicate(sourceMailbox('inbox'), 'date', ctx)).toEqual(
      { inMailboxes: ['inbox'] },
    )
  })

  it('defers a role smart mailbox whose role resolves to no mailbox', () => {
    expect(
      resolveMailListPredicate(
        { kind: 'smart-mailbox', smartMailboxId: 'default-inbox' },
        'date',
        { ...ctx, mailboxesForRole: () => [] },
      ),
    ).toBe('deferred')
  })
})

describe('isMailListSelfMaintained', () => {
  // The option-iii gate: the runtime skips the per-event re-serve only for
  // self-maintained (evaluable) mail-lists. A Deferred mail-list (user
  // smart-mailbox, global, null-mailbox, non-`date`) MUST stay false so the
  // runtime re-serves it — else it stales until reload (the regression).

  it('is self-maintained for a concrete source mailbox under the default date sort', () => {
    expect(isMailListSelfMaintained(sourceMailbox('inbox'), 'date', ctx)).toBe(
      true,
    )
  })

  it('is self-maintained when the sort is omitted (defaults to date)', () => {
    expect(
      isMailListSelfMaintained(sourceMailbox('inbox'), undefined, ctx),
    ).toBe(true)
  })

  it('is self-maintained for a built-in role smart mailbox (All Inboxes)', () => {
    expect(
      isMailListSelfMaintained(
        { kind: 'smart-mailbox', smartMailboxId: 'default-inbox' },
        'date',
        ctx,
      ),
    ).toBe(true)
  })

  it('is deferred for a non-date sort', () => {
    expect(isMailListSelfMaintained(sourceMailbox('inbox'), 'subject', ctx)).toBe(
      false,
    )
    expect(
      isMailListSelfMaintained(sourceMailbox('inbox'), 'relevance', ctx),
    ).toBe(false)
  })

  it('is deferred for a source mailbox with no concrete mailbox id', () => {
    expect(isMailListSelfMaintained(sourceMailbox(null), 'date', ctx)).toBe(false)
  })

  it('is deferred for a user-defined smart mailbox (opaque rule)', () => {
    expect(
      isMailListSelfMaintained(
        { kind: 'smart-mailbox', smartMailboxId: 'user-rule' },
        'date',
        ctx,
      ),
    ).toBe(false)
  })

  it('is deferred for a global scope', () => {
    expect(isMailListSelfMaintained({ kind: 'global' }, 'date', ctx)).toBe(false)
  })
})
