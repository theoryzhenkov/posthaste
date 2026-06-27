import { describe, expect, it } from 'bun:test'

import type { RuntimeMessagePageScope } from '../src/runtime/types'
import { isMailListSelfMaintained } from '../src/runtime/mailListSelfMaintained'

const sourceMailbox = (mailboxId: string | null): RuntimeMessagePageScope => ({
  kind: 'source-mailbox',
  sourceId: 'primary',
  mailboxId,
})

describe('isMailListSelfMaintained', () => {
  // The option-iii gate: the runtime skips the per-event re-serve only for
  // self-maintained (evaluable) mail-lists. A Deferred mail-list (smart-mailbox,
  // global, null-mailbox, non-`date`) MUST stay false so the runtime re-serves
  // it — else it stales until reload (the regression).

  it('is self-maintained for a concrete source mailbox under the default date sort', () => {
    expect(isMailListSelfMaintained(sourceMailbox('inbox'), 'date')).toBe(true)
  })

  it('is self-maintained when the sort is omitted (defaults to date)', () => {
    expect(isMailListSelfMaintained(sourceMailbox('inbox'), undefined)).toBe(true)
  })

  it('is deferred for a non-date sort', () => {
    expect(isMailListSelfMaintained(sourceMailbox('inbox'), 'subject')).toBe(false)
    expect(isMailListSelfMaintained(sourceMailbox('inbox'), 'relevance')).toBe(false)
  })

  it('is deferred for a source mailbox with no concrete mailbox id', () => {
    expect(isMailListSelfMaintained(sourceMailbox(null), 'date')).toBe(false)
  })

  it('is deferred for a smart-mailbox scope', () => {
    expect(
      isMailListSelfMaintained({ kind: 'smart-mailbox', smartMailboxId: 'all' }, 'date'),
    ).toBe(false)
  })

  it('is deferred for a global scope', () => {
    expect(isMailListSelfMaintained({ kind: 'global' }, 'date')).toBe(false)
  })
})
