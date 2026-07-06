import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'

import type { AccountAppearance, AppSettings, Mailbox } from '../src/api/types'
import { SourceSection } from '../src/components/sidebar/SourceSection'
import { queryKeys } from '../src/queryKeys'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

const mkMailbox = (id: string): Mailbox => ({
  id,
  name: id,
  role: null,
  unreadEmails: 0,
  totalEmails: 0,
})

const appearance: AccountAppearance = {
  kind: 'initials',
  initials: 'A',
  colorHue: 10,
}

const settings: AppSettings = {
  defaultAccountId: null,
  cachePolicy: 'auto' as never,
  automationRules: [],
  automationDrafts: [],
  mailboxColors: [],
  tags: [],
  smartMailboxOrder: [],
  accountOrder: [],
  mailboxGroups: [
    { id: 'g-finance', name: 'Finance', mailboxIds: ['receipts'], order: 0 },
  ],
}

const source = {
  id: 'acct',
  name: 'Acct',
  mailboxes: [mkMailbox('inbox'), mkMailbox('receipts')],
}

function renderSection(collapsedGroupIds: ReadonlySet<string>) {
  setRuntimeAdapterForTesting(createFakeRuntimeAdapter())
  const qc = new QueryClient()
  qc.setQueryData(queryKeys.settings, settings)
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
  return render(
    <SourceSection
      source={source}
      appearance={appearance}
      selectedView={null}
      isPaneActive={false}
      collapsed={false}
      collapsedGroupIds={collapsedGroupIds}
      onToggleCollapsed={() => {}}
      onToggleGroupCollapsed={() => {}}
      onOpenAccountSettings={() => {}}
      onSelectSourceMailbox={() => {}}
      onSyncSource={() => {}}
    />,
    { wrapper },
  )
}

describe('SourceSection groups', () => {
  it('renders the Group header, its nested member, and the ungrouped remainder', () => {
    const view = renderSection(new Set())
    // Group header.
    expect(view.getByText('Finance')).toBeTruthy()
    // Ungrouped mailbox renders flat.
    expect(view.getByText('inbox')).toBeTruthy()
    // Grouped member renders (expanded group).
    expect(view.getByText('receipts')).toBeTruthy()
  })

  it('collapsing a Group hides its members but keeps the header + ungrouped rows', () => {
    const view = renderSection(new Set(['g-finance']))
    expect(view.getByText('Finance')).toBeTruthy()
    expect(view.getByText('inbox')).toBeTruthy()
    expect(view.queryByText('receipts')).toBeNull()
  })
})
