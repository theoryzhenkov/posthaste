import { describe, expect, it } from 'bun:test'

import { isAggregateMessageView } from '../src/components/message-list/model'
import type { SidebarSelection } from '../src/components/Sidebar'

const smartMailbox: SidebarSelection = {
  kind: 'smart-mailbox',
  id: 'unified-inbox',
  name: 'All Inboxes',
}

const sourceMailbox: SidebarSelection = {
  kind: 'source-mailbox',
  sourceId: 'account-1',
  mailboxId: 'inbox',
  name: 'Inbox',
}

describe('isAggregateMessageView (show-source-mailbox default)', () => {
  it('defaults ON for a smart mailbox (unified inbox / saved query), no search', () => {
    expect(isAggregateMessageView(smartMailbox, undefined)).toBe(true)
  })

  it('defaults ON for a global search with no mailbox selected', () => {
    expect(isAggregateMessageView(null, 'invoice')).toBe(true)
  })

  it('defaults ON for a global search with no query trimmed to empty and no view', () => {
    expect(isAggregateMessageView(null, undefined)).toBe(true)
  })

  it('defaults OFF for a single source mailbox with no search query', () => {
    expect(isAggregateMessageView(sourceMailbox, undefined)).toBe(false)
    expect(isAggregateMessageView(sourceMailbox, '')).toBe(false)
    expect(isAggregateMessageView(sourceMailbox, '   ')).toBe(false)
  })

  it('defaults ON for a source mailbox narrowed by a search query', () => {
    expect(isAggregateMessageView(sourceMailbox, 'invoice')).toBe(true)
  })
})
